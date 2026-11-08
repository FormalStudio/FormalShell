//! iPad continuity: padbridge (FormalStudio/padbridge) shares this
//! machine's pointer and keyboard with an iPad over BLE HID, the iPad
//! sitting past one stretch of one edge of the desktop. The shell reads
//! its state off the session bus and asks it to switch, reconnect and move
//! the iPad. `None` while nothing owns padbridge's name, followed live
//! through NameOwnerChanged.

use std::collections::HashMap;
use std::sync::LazyLock;

use async_channel::{Receiver, Sender};
use fs_dbus::{Owner, OwnerWatch};
use fs_types::Edge;
use futures_lite::StreamExt;
use zbus::connection::Builder;
use zbus::message::Type;
use zbus::zvariant::OwnedValue;
use zbus::{Connection, MatchRule, MessageStream};

use crate::runtime::Ctx;
use crate::store;

pub const NAME: &str = "studio.formal.Padbridge";
const PATH: &str = "/studio/formal/Padbridge";
const IFACE: &str = "studio.formal.Padbridge1";
const PROPS: &str = "org.freedesktop.DBus.Properties";

/// The BLE input link.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Link {
    #[default]
    Unpaired,
    Down,
    Up,
}

impl Link {
    fn parse(s: &str) -> Self {
        match s {
            "up" => Link::Up,
            "down" => Link::Down,
            _ => Link::Unpaired,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Link::Unpaired => "unpaired",
            Link::Down => "down",
            Link::Up => "up",
        }
    }
}

/// The stretch of an edge leading onto the iPad, each end a fraction of
/// the edge's length from its top or left end.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Stretch {
    start: f64,
    end: f64,
}

impl Stretch {
    pub const WHOLE: Stretch = Stretch { start: 0.0, end: 1.0 };

    /// padbridge's own rule for `SetPosition`: 0 <= start < end <= 1.
    pub fn new(start: f64, end: f64) -> Option<Self> {
        (0.0 <= start && start < end && end <= 1.0).then_some(Stretch { start, end })
    }

    /// Within a thousandth of the edge at both ends.
    pub fn near(self, start: f64, end: f64) -> bool {
        (self.start - start).abs() < 1e-3 && (self.end - end).abs() < 1e-3
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Ipad {
    pub link: Link,
    /// Input goes to the iPad right now.
    pub active: bool,
    /// The iPad's audio plays on this machine.
    pub audio: bool,
    /// Its Bluetooth alias, empty until padbridge knows it.
    pub name: String,
    pub edge: Edge,
    pub stretch: Stretch,
}

impl Default for Ipad {
    fn default() -> Self {
        Self { link: Link::Unpaired, active: false, audio: false, name: String::new(), edge: Edge::Right, stretch: Stretch::WHOLE }
    }
}

impl Ipad {
    /// The state a tile or a row reads under the name.
    pub fn state_label(&self) -> &'static str {
        match (self.active, self.link) {
            (true, _) => "On iPad",
            (false, Link::Up) => "Connected",
            (false, Link::Down) => "Not connected",
            (false, Link::Unpaired) => "Not paired",
        }
    }

    fn merge(&mut self, props: &HashMap<String, OwnedValue>) {
        let mut start = self.stretch.start;
        let mut end = self.stretch.end;
        for (key, value) in props {
            match key.as_str() {
                "Link" => {
                    if let Ok(s) = <&str>::try_from(value) {
                        self.link = Link::parse(s);
                    }
                }
                "Active" => self.active = bool::try_from(value).unwrap_or(self.active),
                "Audio" => self.audio = bool::try_from(value).unwrap_or(self.audio),
                "Name" => {
                    if let Ok(s) = <&str>::try_from(value) {
                        self.name = s.to_owned();
                    }
                }
                "Edge" => {
                    if let Some(edge) = <&str>::try_from(value).ok().and_then(Edge::parse) {
                        self.edge = edge;
                    }
                }
                "Start" => start = f64::try_from(value).unwrap_or(start),
                "End" => end = f64::try_from(value).unwrap_or(end),
                _ => {}
            }
        }
        // Start and End change in one signal, so a pair that breaks the rule
        // is padbridge's own and is kept as last seen.
        if let Some(stretch) = Stretch::new(start, end) {
            self.stretch = stretch;
        }
    }
}

#[derive(Default)]
pub struct State {
    /// None while padbridge is not on the bus.
    pub bridge: Option<Ipad>,
}

pub struct Diff(pub Option<Ipad>);

impl State {
    pub fn apply(&mut self, diff: Diff) -> bool {
        let changed = self.bridge != diff.0;
        self.bridge = diff.0;
        changed
    }

    /// The IPC `status` reply.
    pub fn status(&self) -> String {
        let Some(i) = &self.bridge else { return serde_json::json!({ "available": false }).to_string() };
        serde_json::json!({
            "available": true,
            "link": i.link.as_str(),
            "active": i.active,
            "audio": i.audio,
            "name": i.name,
            "edge": i.edge.as_str(),
            "start": i.stretch.start,
            "end": i.stretch.end,
        })
        .to_string()
    }
}

pub enum Cmd {
    Switch,
    Reconnect,
    Position(Edge, Stretch),
}

static CHANNEL: LazyLock<(Sender<Cmd>, Receiver<Cmd>)> = LazyLock::new(async_channel::unbounded);

/// A call for padbridge, already checked against the state the UI holds.
pub fn command(cmd: Cmd) {
    let _ = CHANNEL.0.try_send(cmd);
}

async fn read(conn: &Connection) -> Option<Ipad> {
    let reply = conn.call_method(Some(NAME), PATH, Some(PROPS), "GetAll", &(IFACE,)).await.ok()?;
    let props: HashMap<String, OwnedValue> = reply.body().deserialize().ok()?;
    let mut ipad = Ipad::default();
    ipad.merge(&props);
    Some(ipad)
}

async fn commands(conn: Connection) {
    while let Ok(cmd) = CHANNEL.1.recv().await {
        let call = match cmd {
            Cmd::Switch => conn.call_method(Some(NAME), PATH, Some(IFACE), "Switch", &()).await,
            Cmd::Reconnect => conn.call_method(Some(NAME), PATH, Some(IFACE), "Reconnect", &()).await,
            Cmd::Position(edge, s) => conn.call_method(Some(NAME), PATH, Some(IFACE), "SetPosition", &(edge.as_str(), s.start, s.end)).await,
        };
        if let Err(err) = call {
            eprintln!("ipad: {err}");
        }
    }
}

enum Input {
    Owner(Owner),
    Changed(HashMap<String, OwnedValue>, bool),
}

pub async fn run(ctx: Ctx) {
    let publish = |ipad: Option<Ipad>| ctx.publish(store::Diff::Ipad(Diff(ipad)));
    let conn = match crate::services::bus::private(&ctx, Builder::session()).await {
        Ok(conn) => conn,
        Err(err) => return eprintln!("ipad: {err}"),
    };
    let owners = match OwnerWatch::new(&conn, NAME).await {
        Ok(s) => s,
        Err(err) => return eprintln!("ipad: NameOwnerChanged: {err}"),
    };
    let rule = MatchRule::builder()
        .msg_type(Type::Signal)
        .sender(NAME)
        .and_then(|b| b.path(PATH))
        .and_then(|b| b.interface(PROPS))
        .and_then(|b| b.member("PropertiesChanged"))
        .and_then(|b| b.arg(0, IFACE));
    let changes = match rule {
        Ok(rule) => MessageStream::for_match_rule(rule.build(), &conn, None).await,
        Err(err) => Err(err),
    };
    let changes = match changes {
        Ok(s) => s,
        Err(err) => return eprintln!("ipad: PropertiesChanged: {err}"),
    };
    ctx.spawn(commands(conn.clone()));

    // Subscribed before the first read, so nothing falls between the two.
    let mut shown = read(&conn).await;
    publish(shown.clone());
    let owners = owners.map(|change| Input::Owner(change.owner));
    let changes = changes.filter_map(|m| {
        let (_, changed, invalidated): (String, HashMap<String, OwnedValue>, Vec<String>) = m.ok()?.body().deserialize().ok()?;
        Some(Input::Changed(changed, !invalidated.is_empty()))
    });
    let mut inputs = std::pin::pin!(owners.or(changes));
    while let Some(input) = inputs.next().await {
        let next = match input {
            Input::Owner(Owner::Gone) => None,
            Input::Owner(Owner::Back) => read(&conn).await,
            Input::Changed(changed, false) if shown.is_some() => shown.clone().map(|mut i| {
                i.merge(&changed);
                i
            }),
            Input::Changed(..) => read(&conn).await,
        };
        if next != shown {
            shown = next;
            publish(shown.clone());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use zbus::zvariant::Value;

    fn props(pairs: &[(&str, Value<'static>)]) -> HashMap<String, OwnedValue> {
        pairs.iter().map(|(k, v)| ((*k).to_owned(), OwnedValue::try_from(v.try_clone().unwrap()).unwrap())).collect()
    }

    #[test]
    fn merges_every_property() {
        let mut i = Ipad::default();
        i.merge(&props(&[
            ("Link", Value::from("up")),
            ("Active", Value::from(true)),
            ("Audio", Value::from(true)),
            ("Name", Value::from("FormalPad")),
            ("Edge", Value::from("top")),
            ("Start", Value::from(0.25)),
            ("End", Value::from(0.75)),
        ]));
        assert_eq!(i.link, Link::Up);
        assert!(i.active && i.audio);
        assert_eq!(i.name, "FormalPad");
        assert_eq!(i.edge, Edge::Top);
        assert!(i.stretch.near(0.25, 0.75));
        assert_eq!(i.state_label(), "On iPad");
    }

    #[test]
    fn keeps_a_stretch_that_breaks_the_rule() {
        let mut i = Ipad::default();
        i.merge(&props(&[("Start", Value::from(0.8)), ("End", Value::from(0.2)), ("Edge", Value::from("middle"))]));
        assert_eq!(i.stretch, Stretch::WHOLE);
        assert_eq!(i.edge, Edge::Right);
    }

    #[test]
    fn stretch_takes_padbridges_rule() {
        assert!(Stretch::new(0.0, 1.0).is_some());
        assert!(Stretch::new(0.5, 0.5).is_none());
        assert!(Stretch::new(-0.1, 0.5).is_none());
        assert!(Stretch::new(0.5, 1.1).is_none());
    }

    #[test]
    fn status_reads_absent_and_present() {
        let mut s = State::default();
        assert_eq!(s.status(), r#"{"available":false}"#);
        s.apply(Diff(Some(Ipad { name: "FormalPad".into(), link: Link::Down, ..Ipad::default() })));
        let v: serde_json::Value = serde_json::from_str(&s.status()).unwrap();
        assert_eq!(v["link"], "down");
        assert_eq!(v["edge"], "right");
        assert_eq!(v["end"], 1.0);
    }
}
