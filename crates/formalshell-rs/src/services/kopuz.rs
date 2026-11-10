//! kopuzd, the daemon behind Kopuz and FormalMusic, followed while the
//! active MPRIS player is Kopuz's own (`org.mpris.MediaPlayer2.kopuz`, or
//! `.kopuz.instance<pid>` for a second instance) and its socket is there.
//! One `Subscribe` stream carries the track changes and the favourites'
//! invalidation, so nothing polls; with no Kopuz player active there is no
//! connection and no timer at all.
//!
//! The heart and "don't recommend" are the two actions Kopuz's own player
//! bar offers for the playing track (`components/src/layout/*/bottombar.rs`):
//! the heart on every track, `SetFavorite` flipping it, and "don't
//! recommend" only where the active source takes it, refused while the track
//! is a favourite (one setting on the remote, so it would undo the heart)
//! and skipping to the next track whether or not the daemon took it.
//!
//! The published row is the daemon's own. It belongs to the playing track
//! only when its title and artist read the same as MPRIS's (`media::State::
//! kopuz`), since the GUI can be playing against another daemon.

use std::cell::RefCell;
use std::sync::LazyLock;
use std::time::Duration;

use async_io::Timer;
use fs_kopuz::{Client, Event, Table, TrackInfo, client, timed};
use fs_types::Backoff;
use futures_lite::future;

use crate::runtime::Ctx;
use crate::services::wants::{self, Source, Want};
use crate::store;

const BASE_BACKOFF: Duration = Duration::from_secs(1);
const MAX_BACKOFF: Duration = Duration::from_secs(30);

#[derive(Clone, Debug, Default, PartialEq)]
pub struct State {
    pub connected: bool,
    pub key: String,
    pub title: String,
    pub artist: String,
    pub favorite: bool,
    /// The active source takes "don't recommend".
    pub dont_recommend: bool,
}

/// What the media panel and IPC send for the playing track.
#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    Favorite(String, bool),
    DontRecommend(String),
}

impl State {
    /// The actions Kopuz offers for this track, by their IPC verbs.
    pub fn actions(&self) -> Vec<&'static str> {
        let mut out = vec!["like"];
        if self.dont_recommend {
            out.push("dontRecommend");
        }
        out
    }

    /// The heart: `on`, `off` or `toggle`.
    pub fn like(&self, mode: &str) -> Result<Action, String> {
        let on = match mode {
            "on" => true,
            "off" => false,
            "toggle" => !self.favorite,
            _ => return Err(format!("unknown mode {mode} (on|off|toggle)")),
        };
        Ok(Action::Favorite(self.key.clone(), on))
    }

    pub fn dont_recommend_action(&self) -> Result<Action, String> {
        if !self.dont_recommend {
            return Err("source does not take dontRecommend".into());
        }
        if self.favorite {
            return Err("track is a favorite".into());
        }
        Ok(Action::DontRecommend(self.key.clone()))
    }

    /// The daemon's row is the track MPRIS reports: same title and artist,
    /// case and runs of whitespace aside.
    pub fn matches(&self, title: &str, artist: &str) -> bool {
        let norm = |s: &str| s.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase();
        self.connected && !self.key.is_empty() && norm(&self.title) == norm(title) && norm(&self.artist) == norm(artist)
    }
}

thread_local! {
    static HELD: RefCell<Option<Want>> = const { RefCell::new(None) };
}

/// The UI thread's side, after every media change: the daemon is wanted
/// while the active player is Kopuz.
pub fn sync(media: &super::media::State) {
    let want = fs_kopuz::is_player_bus(&media.active_id());
    HELD.with_borrow_mut(|held| {
        if want != held.is_some() {
            *held = want.then(|| Want::new(Source::Kopuz));
        }
    });
}

static ACTIONS: LazyLock<(async_channel::Sender<Action>, async_channel::Receiver<Action>)> = LazyLock::new(async_channel::unbounded);

/// Callable from any thread. A favourite comes back through
/// `LibraryInvalidated`, so nothing is published for it.
pub fn send(action: Action) {
    let _ = ACTIONS.0.try_send(action);
}

fn publish(ctx: &Ctx, state: &State) {
    ctx.publish(store::Diff::Media(super::media::Diff::Kopuz(state.clone())));
}

async fn favorite(client: &Client, key: &str) -> bool {
    !key.is_empty() && matches!(timed(client.favorites()).await, Some(Ok(f)) if f.refs.iter().any(|r| r == key))
}

async fn dont_recommend(client: &Client) -> bool {
    matches!(timed(client.sources()).await, Some(Ok(list)) if list.active().is_some_and(|s| s.capabilities.dont_recommend))
}

fn act(ctx: &Ctx, action: Action) {
    ctx.spawn(async move {
        let Some(client) = client() else { return eprintln!("kopuz: no socket for {action:?}") };
        let result = match &action {
            Action::Favorite(key, on) => timed(client.set_favorite(key, *on)).await,
            Action::DontRecommend(key) => timed(client.dont_recommend(key)).await,
        };
        match result {
            Some(Ok(())) => {}
            Some(Err(e)) => eprintln!("kopuz: {action:?} failed: {e}"),
            None => eprintln!("kopuz: {action:?} timed out"),
        }
        if matches!(action, Action::DontRecommend(_)) {
            let _ = timed(client.next()).await;
        }
    });
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
    state.dont_recommend = dont_recommend(client).await;
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
            Ok(Some(Event::LibraryInvalidated(Table::Servers))) => state.dont_recommend = dont_recommend(client).await,
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
    let mut backoff = Backoff::new(BASE_BACKOFF, MAX_BACKOFF);
    loop {
        if let Some(client) = client()
            && session(ctx, &client).await
        {
            backoff.reset();
        }
        publish(ctx, &State::default());
        Timer::after(backoff.delay()).await;
    }
}

pub fn start(ctx: &Ctx) {
    ctx.spawn(run(ctx.clone()));
}

async fn run(ctx: Ctx) {
    let actions = ctx.clone();
    ctx.spawn(async move {
        while let Ok(action) = ACTIONS.1.recv().await {
            act(&actions, action);
        }
    });
    let rx = wants::watch(Source::Kopuz);
    let mut want = false;
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
    fn a_row_matches_on_title_and_artist_alone() {
        let s = State { connected: true, key: "k".into(), title: "Hello  World".into(), artist: "Ádele".into(), ..State::default() };
        assert!(s.matches(" hello world", "ádele"));
        assert!(!s.matches("Hello World", "Someone"));
        assert!(!State { connected: false, ..s.clone() }.matches("Hello World", "Ádele"));
    }

    #[test]
    fn like_and_dont_recommend_follow_the_heart() {
        let s = State { key: "k".into(), dont_recommend: true, ..State::default() };
        assert_eq!(s.like("toggle"), Ok(Action::Favorite("k".into(), true)));
        assert_eq!(s.like("off"), Ok(Action::Favorite("k".into(), false)));
        assert!(s.like("x").is_err());
        assert_eq!(s.dont_recommend_action(), Ok(Action::DontRecommend("k".into())));
        let liked = State { favorite: true, ..s.clone() };
        assert_eq!(liked.like("toggle"), Ok(Action::Favorite("k".into(), false)));
        assert!(liked.dont_recommend_action().is_err());
        assert!(State { dont_recommend: false, ..s }.dont_recommend_action().is_err());
    }
}
