//! kopuzd, the daemon behind Kopuz and FormalMusic, followed while the
//! active MPRIS player is Kopuz's own (`org.mpris.MediaPlayer2.kopuz`, or
//! `.kopuz.instance<pid>` for a second instance) and its socket is there.
//! One `Subscribe` stream carries the track changes and the favourites'
//! invalidation, so nothing polls; with no Kopuz player active there is no
//! connection and no timer at all.
//!
//! The published row is the daemon's own. It belongs to the playing track
//! only when its title and artist read the same as MPRIS's (`media::State::
//! kopuz`), since the GUI can be playing against another daemon.

use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use async_io::Timer;
use fs_kopuz::{Client, Event, Table, TrackInfo};
use futures_lite::future;

use crate::runtime::Ctx;
use crate::store;

const BASE_BACKOFF: Duration = Duration::from_secs(1);
const MAX_BACKOFF: Duration = Duration::from_secs(30);
/// A unary call to a local socket answers in milliseconds; past this the
/// daemon is wedged and the call is abandoned.
pub const CALL_TIMEOUT: Duration = Duration::from_secs(3);

#[derive(Clone, Debug, Default, PartialEq)]
pub struct State {
    pub connected: bool,
    pub key: String,
    pub title: String,
    pub artist: String,
    pub favorite: bool,
}

impl State {
    /// The daemon's row is the track MPRIS reports: same title and artist,
    /// case and runs of whitespace aside.
    pub fn matches(&self, title: &str, artist: &str) -> bool {
        let norm = |s: &str| s.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase();
        self.connected && !self.key.is_empty() && norm(&self.title) == norm(title) && norm(&self.artist) == norm(artist)
    }
}

/// An MPRIS bus name Kopuz registers.
pub fn is_kopuz(bus: &str) -> bool {
    match bus.strip_prefix("org.mpris.MediaPlayer2.kopuz") {
        Some("") => true,
        Some(rest) => rest.strip_prefix(".instance").is_some_and(|pid| !pid.is_empty() && pid.bytes().all(|b| b.is_ascii_digit())),
        None => false,
    }
}

/// A client for kopuzd's socket, when it is there.
pub fn client() -> Option<Client> {
    fs_kopuz::socket_path().filter(|p| p.exists()).map(Client::new)
}

/// `fut`, or None past `CALL_TIMEOUT`.
pub async fn timed<T>(fut: impl Future<Output = T>) -> Option<T> {
    future::or(async { Some(fut.await) }, async {
        Timer::after(CALL_TIMEOUT).await;
        None
    })
    .await
}

static WANT: OnceLock<async_channel::Sender<bool>> = OnceLock::new();
static WANTED: AtomicBool = AtomicBool::new(false);

/// The UI thread's side, after every media change: whether the active
/// player is Kopuz.
pub fn sync(media: &super::media::State) {
    let want = is_kopuz(&media.active_id());
    if WANTED.swap(want, Ordering::Relaxed) != want
        && let Some(tx) = WANT.get()
    {
        let _ = tx.try_send(want);
    }
}

fn publish(ctx: &Ctx, state: &State) {
    ctx.publish(store::Diff::Media(super::media::Diff::Kopuz(state.clone())));
}

async fn favorite(client: &Client, key: &str) -> bool {
    !key.is_empty() && matches!(timed(client.favorites()).await, Some(Ok(f)) if f.refs.iter().any(|r| r == key))
}

fn row(state: &mut State, row: Option<TrackInfo>) {
    let row = row.unwrap_or_default();
    state.key = row.key;
    state.title = row.title;
    state.artist = row.artist;
}

/// One connection, until the stream ends or fails. True once it got as far
/// as a first player state, so the backoff starts over.
async fn session(ctx: &Ctx, client: &Client) -> bool {
    let Some(Ok(mut events)) = timed(client.subscribe()).await else { return false };
    let Some(Ok(first)) = timed(client.player_state()).await else { return false };
    let mut state = State { connected: true, ..State::default() };
    row(&mut state, first.row);
    state.favorite = favorite(client, &state.key).await;
    publish(ctx, &state);
    loop {
        let before = state.clone();
        match events.next().await {
            Ok(Some(Event::PlayerState(p))) => {
                row(&mut state, p.row);
                if state.key != before.key {
                    state.favorite = favorite(client, &state.key).await;
                }
            }
            Ok(Some(Event::LibraryInvalidated(Table::Favorites))) => state.favorite = favorite(client, &state.key).await,
            Ok(Some(_)) => {}
            Ok(None) | Err(_) => return true,
        }
        if state != before {
            publish(ctx, &state);
        }
    }
}

/// Connected while it can be, retrying on a doubling backoff otherwise.
async fn follow(ctx: &Ctx) {
    let mut backoff = BASE_BACKOFF;
    loop {
        if let Some(client) = client()
            && session(ctx, &client).await
        {
            backoff = BASE_BACKOFF;
        }
        publish(ctx, &State::default());
        Timer::after(backoff).await;
        backoff = (backoff * 2).min(MAX_BACKOFF);
    }
}

pub async fn run(ctx: Ctx) {
    let (tx, rx) = async_channel::unbounded::<bool>();
    let _ = WANT.set(tx);
    let mut want = WANTED.load(Ordering::Relaxed);
    loop {
        if want {
            // The player going (or another one picked) drops the stream.
            want = future::or(
                async {
                    follow(&ctx).await;
                    false
                },
                async {
                    loop {
                        match rx.recv().await {
                            Ok(true) => continue,
                            Ok(false) | Err(_) => return false,
                        }
                    }
                },
            )
            .await;
            publish(&ctx, &State::default());
        } else {
            match rx.recv().await {
                Ok(next) => want = next,
                Err(_) => return,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kopuz_bus_names() {
        assert!(is_kopuz("org.mpris.MediaPlayer2.kopuz"));
        assert!(is_kopuz("org.mpris.MediaPlayer2.kopuz.instance4242"));
        assert!(!is_kopuz("org.mpris.MediaPlayer2.kopuzz"));
        assert!(!is_kopuz("org.mpris.MediaPlayer2.kopuz.instance"));
        assert!(!is_kopuz("org.mpris.MediaPlayer2.mpv"));
    }

    #[test]
    fn a_row_matches_on_title_and_artist_alone() {
        let s = State { connected: true, key: "k".into(), title: "Hello  World".into(), artist: "Ádele".into(), favorite: false };
        assert!(s.matches(" hello world", "ádele"));
        assert!(!s.matches("Hello World", "Someone"));
        assert!(!State { connected: false, ..s.clone() }.matches("Hello World", "Ádele"));
    }
}
