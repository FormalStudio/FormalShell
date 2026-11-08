//! MPRIS client on zbus. Executor-agnostic: no task is spawned here. The
//! shell drives [`Mpris::next`] on its own executor and turns the returned
//! changes into store diffs; [`Controls`] is the cloneable command side.

mod proxy;
mod state;

use std::future::pending;
use std::time::Instant;

use futures_util::future::{self, AbortHandle, Abortable, Either};
use futures_util::stream::{self, BoxStream, SelectAll, StreamExt};
use zbus::fdo::{DBusProxy, IntrospectableProxy, PropertiesProxy};
use zbus::names::InterfaceName;
use zbus::zvariant::{OwnedValue, Value};
use zbus::{Connection, Result};

use proxy::{OBJECT_PATH, PLAYER_IFACE, PlayerProxy, ROOT_IFACE, RootProxy};
pub use state::{LoopStatus, Metadata, PlaybackStatus, PlayerState, Volume};

const BUS_PREFIX: &str = "org.mpris.MediaPlayer2.";

enum Input {
    Props { name: String, root: bool },
    Seeked { name: String, position_us: i64 },
}

struct Entry {
    state: PlayerState,
    abort: AbortHandle,
}

pub struct Mpris {
    conn: Connection,
    owners: zbus::fdo::NameOwnerChangedStream,
    players: Vec<Entry>,
    inputs: SelectAll<BoxStream<'static, Input>>,
    /// A player appeared, left or changed since the last [`Mpris::next`].
    dirty: bool,
}

impl Mpris {
    /// Subscribes to name changes first, then lists the names already on the
    /// bus, so a player starting in between is never missed. Players found
    /// at startup are in [`Mpris::players`] on return.
    pub async fn connect(conn: &Connection) -> Result<Self> {
        let dbus = DBusProxy::new(conn).await?;
        let owners = dbus.receive_name_owner_changed().await?;
        let mut me = Mpris {
            conn: conn.clone(),
            owners,
            players: Vec::new(),
            inputs: SelectAll::new(),
            dirty: false,
        };
        let mut names: Vec<String> = dbus
            .list_names()
            .await?
            .into_iter()
            .map(|n| n.to_string())
            .filter(|n| n.starts_with(BUS_PREFIX))
            .collect();
        names.sort();
        for name in names {
            me.add_player(name).await;
        }
        Ok(me)
    }

    pub fn controls(&self) -> Controls {
        Controls { conn: self.conn.clone() }
    }

    /// Players in the order they appeared.
    pub fn players(&self) -> impl Iterator<Item = &PlayerState> {
        self.players.iter().map(|e| &e.state)
    }

    pub fn player(&self, bus_name: &str) -> Option<&PlayerState> {
        self.players.iter().map(|e| &e.state).find(|s| s.bus_name == bus_name)
    }

    /// Waits until a player appeared, left or changed (read [`Mpris::players`] for the
    /// result). `None` once the bus connection is gone.
    pub async fn next(&mut self) -> Option<()> {
        loop {
            if std::mem::take(&mut self.dirty) {
                return Some(());
            }
            let wake = {
                let owner = self.owners.next();
                let input = async {
                    match self.inputs.next().await {
                        Some(i) => i,
                        // SelectAll ends at once while empty; wait for an owner change instead.
                        None => pending().await,
                    }
                };
                futures_util::pin_mut!(owner, input);
                let r = future::select(owner, input).await;
                match r {
                    Either::Left((o, _)) => Either::Left(o),
                    Either::Right((i, _)) => Either::Right(i),
                }
            };
            match wake {
                Either::Left(None) => return None,
                Either::Left(Some(sig)) => {
                    let Ok(args) = sig.args() else { continue };
                    let name = args.name().to_string();
                    if !name.starts_with(BUS_PREFIX) {
                        continue;
                    }
                    if args.new_owner().is_some() && self.index(&name).is_some() {
                        // Name moved to a new process: the old state is stale.
                        self.remove_player(&name);
                    }
                    if args.new_owner().is_some() {
                        self.add_player(name).await;
                    } else {
                        self.remove_player(&name);
                    }
                }
                Either::Right(input) => match input {
                    Input::Props { name, root } => self.refresh(&name, root).await,
                    Input::Seeked { name, position_us } => {
                        if let Some(i) = self.index(&name) {
                            self.players[i].state.set_base(position_us, Instant::now());
                            self.dirty = true;
                        }
                    }
                },
            }
        }
    }

    fn index(&self, name: &str) -> Option<usize> {
        self.players.iter().position(|e| e.state.bus_name == name)
    }

    fn remove_player(&mut self, name: &str) {
        if let Some(i) = self.index(name) {
            self.players.remove(i).abort.abort();
            self.dirty = true;
        }
    }

    async fn add_player(&mut self, name: String) {
        if self.index(&name).is_some() {
            return;
        }
        let Ok((state, streams)) = self.load(&name).await else { return };
        let (abort, reg) = AbortHandle::new_pair();
        self.inputs.push(Abortable::new(streams, reg).boxed());
        self.players.push(Entry { state, abort });
        self.dirty = true;
    }

    /// Reads the whole player and opens its change streams. A player whose
    /// Player interface cannot be read is not a usable player and is skipped.
    async fn load(&self, name: &str) -> Result<(PlayerState, BoxStream<'static, Input>)> {
        let props = props_proxy(&self.conn, name).await?;
        let seeked = PlayerProxy::builder(&self.conn)
            .destination(name.to_owned())?
            .cache_properties(zbus::proxy::CacheProperties::No)
            .build()
            .await?
            .receive_seeked()
            .await?;
        let changed = props.receive_properties_changed().await?;

        let mut state = PlayerState::new(name.to_owned(), Instant::now());
        if let Ok(root) = props.get_all(iface(ROOT_IFACE)).await {
            state.apply_root(&root);
        }
        state.apply_player(&props.get_all(iface(PLAYER_IFACE)).await?);
        apply_position(&mut state, read_position(&self.conn, name).await);
        state.volume_writable = volume_writable(&self.conn, name).await;

        let n1 = name.to_owned();
        let n2 = name.to_owned();
        let props_stream = changed.map(move |sig| Input::Props {
            name: n1.clone(),
            root: sig
                .args()
                .map(|a| a.interface_name().as_str() == ROOT_IFACE)
                .unwrap_or(false),
        });
        let seeked_stream = seeked.filter_map(move |sig| {
            let n = n2.clone();
            async move {
                sig.args()
                    .ok()
                    .map(|a| Input::Seeked { name: n, position_us: *a.position() })
            }
        });
        Ok((state, stream::select(props_stream, seeked_stream).boxed()))
    }

    /// Re-reads after a `PropertiesChanged`. Position is read again every
    /// time: a status, rate or track change is exactly when the interpolation
    /// base goes stale.
    async fn refresh(&mut self, name: &str, root: bool) {
        let Ok(props) = props_proxy(&self.conn, name).await else { return };
        let (root_props, player_props, pos) = if root {
            (props.get_all(iface(ROOT_IFACE)).await.ok(), None, None)
        } else {
            let p = props.get_all(iface(PLAYER_IFACE)).await.ok();
            (None, p, Some(read_position(&self.conn, name).await))
        };
        let Some(i) = self.index(name) else { return };
        let state = &mut self.players[i].state;
        if let Some(r) = root_props {
            state.apply_root(&r);
        }
        if let Some(p) = player_props {
            state.apply_player(&p);
        }
        if let Some(pos) = pos {
            apply_position(state, pos);
        }
        self.dirty = true;
    }
}

fn apply_position(state: &mut PlayerState, pos: Option<i64>) {
    state.position_supported = pos.is_some();
    if let Some(us) = pos {
        state.set_base(us, Instant::now());
    }
}

fn iface(name: &'static str) -> InterfaceName<'static> {
    InterfaceName::from_static_str_unchecked(name)
}

async fn props_proxy(conn: &Connection, name: &str) -> Result<PropertiesProxy<'static>> {
    PropertiesProxy::builder(conn)
        .destination(name.to_owned())?
        .path(OBJECT_PATH)?
        .build()
        .await
}

/// Whether the Player interface's introspection declares `Volume` with
/// write access; one that declares nothing (Chromium) is read-only.
async fn volume_writable(conn: &Connection, name: &str) -> bool {
    let Ok(proxy) = IntrospectableProxy::builder(conn).destination(name.to_owned()).and_then(|b| b.path(OBJECT_PATH)) else {
        return false;
    };
    let Ok(proxy) = proxy.build().await else { return false };
    proxy.introspect().await.is_ok_and(|xml| declares_writable_volume(&xml))
}

fn declares_writable_volume(xml: &str) -> bool {
    let Some(start) = xml.find(&format!("\"{PLAYER_IFACE}\"")) else { return false };
    let body = &xml[start..];
    let body = &body[..body.find("</interface>").unwrap_or(body.len())];
    body.split("<property").skip(1).map(|p| &p[..p.find('>').unwrap_or(p.len())]).any(|p| {
        p.contains("name=\"Volume\"") && (p.contains("access=\"readwrite\"") || p.contains("access=\"write\""))
    })
}

async fn read_position(conn: &Connection, name: &str) -> Option<i64> {
    let props = props_proxy(conn, name).await.ok()?;
    let v: OwnedValue = props.get(iface(PLAYER_IFACE), "Position").await.ok()?;
    state::position_from(&v)
}

/// Command side. Cheap to clone; each call addresses a player by bus name.
/// Capability gating (`CanGoNext`, `CanControl`, ...) is the caller's, read
/// off [`PlayerState`].
#[derive(Clone)]
pub struct Controls {
    conn: Connection,
}

impl Controls {
    async fn player(&self, name: &str) -> Result<PlayerProxy<'static>> {
        PlayerProxy::builder(&self.conn)
            .destination(name.to_owned())?
            .cache_properties(zbus::proxy::CacheProperties::No)
            .build()
            .await
    }

    pub async fn play_pause(&self, name: &str) -> Result<()> {
        self.player(name).await?.play_pause().await
    }

    pub async fn next(&self, name: &str) -> Result<()> {
        self.player(name).await?.next().await
    }

    pub async fn previous(&self, name: &str) -> Result<()> {
        self.player(name).await?.previous().await
    }

    /// Relative seek, microseconds (negative goes back).
    pub async fn seek(&self, name: &str, offset_us: i64) -> Result<()> {
        self.player(name).await?.seek(offset_us).await
    }

    pub async fn set_shuffle(&self, name: &str, on: bool) -> Result<()> {
        self.set_prop(name, "Shuffle", Value::from(on)).await
    }

    pub async fn set_loop_status(&self, name: &str, status: LoopStatus) -> Result<()> {
        self.set_prop(name, "LoopStatus", Value::from(status.as_str())).await
    }

    pub async fn set_volume(&self, name: &str, volume: Volume) -> Result<()> {
        self.set_prop(name, "Volume", Value::from(volume.get())).await
    }

    pub async fn raise(&self, name: &str) -> Result<()> {
        RootProxy::builder(&self.conn)
            .destination(name.to_owned())?
            .cache_properties(zbus::proxy::CacheProperties::No)
            .build()
            .await?
            .raise()
            .await
    }

    async fn set_prop(&self, name: &str, prop: &str, value: Value<'_>) -> Result<()> {
        props_proxy(&self.conn, name)
            .await?
            .set(iface(PLAYER_IFACE), prop, value)
            .await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::declares_writable_volume;

    #[test]
    fn volume_is_writable_only_where_the_player_interface_says_so() {
        let mpv = r#"<node><interface name="org.mpris.MediaPlayer2.Player"><property name="Rate" type="d" access="readwrite"/><property type="d" name="Volume" access="readwrite"><annotation/></property></interface></node>"#;
        assert!(declares_writable_volume(mpv));
        let read_only = r#"<node><interface name="org.mpris.MediaPlayer2.Player"><property name="Volume" type="d" access="read"/></interface></node>"#;
        assert!(!declares_writable_volume(read_only));
        let elsewhere = r#"<node><interface name="org.mpris.MediaPlayer2.Player"></interface><interface name="x"><property name="Volume" type="d" access="readwrite"/></interface></node>"#;
        assert!(!declares_writable_volume(elsewhere));
        assert!(!declares_writable_volume("<node/>"));
    }
}
