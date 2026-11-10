//! AirPlay over UxPlay, owned by the shell behind
//! `airplay.enable`.
//!
//! UxPlay has no MPRIS and no query verb. `active` is whether the `-dacp`
//! file exists (written exactly while a client is connected), metadata and
//! cover art are the `-md` and `-ca` files, all three read through the
//! change signal of their directory. There is no transport control: UxPlay
//! reports no pause state and takes no remote command, so this is a
//! read-only media source.

use std::path::PathBuf;
use std::time::Duration;

use fs_media::airplay::{self, Event};
use futures_lite::FutureExt;
use serde_json::{Value, json};

use super::config::{self, Settings};
use super::watch::Watch;
use crate::runtime::Ctx;
use crate::services::proc;
use crate::settings::{AIRPLAY_ENABLE, AIRPLAY_NAME};
use crate::store;

const BASE_BACKOFF: Duration = Duration::from_secs(2);
const MAX_BACKOFF: Duration = Duration::from_secs(60);

#[derive(Clone, Debug, Default, PartialEq)]
pub struct State {
    pub enabled: bool,
    pub installed: bool,
    pub running: bool,
    pub active: bool,
    pub name: String,
    pub client: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub has_cover: bool,
    pub last_error: String,
}

impl State {
    pub fn status(&self) -> Value {
        json!({
            "enabled": self.enabled,
            "installed": self.installed,
            "running": self.running,
            "active": self.active,
            "name": self.name,
            "client": self.client,
            "title": self.title,
            "artist": self.artist,
            "album": self.album,
            "hasCover": self.has_cover,
            "lastError": self.last_error,
        })
    }
}

pub fn dir() -> PathBuf {
    crate::paths::shell_runtime().join("airplay")
}

fn hostname() -> String {
    std::fs::read_to_string("/proc/sys/kernel/hostname").map(|h| h.trim().to_owned()).unwrap_or_default()
}

/// What a watched file now says.
enum File {
    Dacp(bool),
    Meta(airplay::Metadata),
    Cover(bool),
}

/// Re-reads one file whenever its directory says it may have changed.
fn watch(ctx: &Ctx, path: PathBuf, read: fn(&std::path::Path) -> File, tx: async_channel::Sender<File>) {
    let Ok(mut watch) = Watch::new(path.clone()) else { return };
    ctx.spawn(async move {
        let _ = tx.send(read(&path)).await;
        loop {
            watch.changed().await;
            if tx.send(read(&path)).await.is_err() {
                return;
            }
        }
    });
}

enum Ev {
    Settings(Option<Settings>),
    Child(proc::Event),
    File(Option<File>),
}

/// The keys the receiver runs off.
fn keys(settings: &Settings) -> (bool, String) {
    (AIRPLAY_ENABLE.get(settings), AIRPLAY_NAME.get(settings).to_owned())
}

pub fn start(ctx: &Ctx) {
    ctx.spawn(run(ctx.clone()));
}

async fn run(ctx: Ctx) {
    let settings_rx = config::subscribe();
    let dir = dir();
    let meta = dir.join("meta");
    let cover = dir.join("cover.jpg");
    let dacp = dir.join("dacp");
    // Neither file's directory exists on its own; it is made before uxplay
    // runs and before the watches are placed in it.
    let dir_ready = std::fs::create_dir_all(&dir).is_ok();
    let installed = proc::on_path("uxplay");
    let host = hostname();

    let (tx, files) = async_channel::unbounded::<File>();
    if dir_ready {
        watch(&ctx, dacp.clone(), |p| File::Dacp(p.exists()), tx.clone());
        watch(
            &ctx,
            meta.clone(),
            |p| File::Meta(std::fs::read_to_string(p).map(|t| airplay::parse_metadata(&t)).unwrap_or_default()),
            tx.clone(),
        );
        watch(
            &ctx,
            cover.clone(),
            |p| File::Cover(std::fs::metadata(p).is_ok_and(|m| m.len() > 0 && !airplay::is_placeholder_cover(m.len()))),
            tx.clone(),
        );
    }

    let mut state = State { installed, ..State::default() };
    let mut settings = Settings::default();
    let mut child = proc::Supervised::new(BASE_BACKOFF, MAX_BACKOFF, proc::Reset::OnStart);
    // The three files as last read, shown only while a client is connected:
    // UxPlay never clears `-md` and `-ca` on disconnect.
    let mut dacp_up = false;
    let mut shown_meta = airplay::Metadata::default();
    let mut cover_real = false;
    let mut last = state.clone();
    let mut was_active = false;

    loop {
        state.enabled = AIRPLAY_ENABLE.get(&settings);
        state.name = airplay::resolve_name(AIRPLAY_NAME.get(&settings), &host);
        let should_run = state.enabled && installed && dir_ready;
        if should_run && child.idle() {
            let _ = child.start(uxplay(&state.name, &meta, &cover, &dacp), proc::Read::Stdout);
        } else if !should_run {
            child.stop();
        }
        state.running = child.running();
        state.active = state.running && dacp_up;
        if state.active {
            state.title = shown_meta.title.clone();
            state.artist = shown_meta.artist.clone();
            state.album = shown_meta.album.clone();
            state.has_cover = cover_real;
        } else {
            state.title.clear();
            state.artist.clear();
            state.album.clear();
            state.has_cover = false;
            if !state.running || was_active {
                state.client.clear();
                state.last_error.clear();
            }
        }
        was_active = state.active;
        if state != last {
            last = state.clone();
            ctx.publish(store::Diff::Media(super::media::Diff::Airplay(state.clone())));
        }

        let event = {
            let sets = async { Ev::Settings(config::moved(&settings_rx, &keys(&settings), keys).await) };
            let file = async { Ev::File(files.recv().await.ok()) };
            let run = async { Ev::Child(child.next().await) };
            sets.or(file).or(run).await
        };
        match event {
            Ev::Settings(Some(next)) => settings = next,
            Ev::Settings(None) => return,
            Ev::Child(proc::Event::Line(line)) => match airplay::parse_line(&line) {
                Some(Event::Connected { name, .. }) => {
                    state.client = name;
                    state.last_error.clear();
                }
                Some(Event::Disconnected) => state.client.clear(),
                Some(Event::Error { message }) => state.last_error = message,
                None => {}
            },
            Ev::Child(proc::Event::Exit(_)) => dacp_up = false,
            Ev::Child(proc::Event::Retry) => {}
            Ev::File(Some(File::Dacp(up))) => dacp_up = up,
            Ev::File(Some(File::Meta(m))) => shown_meta = m,
            Ev::File(Some(File::Cover(real))) => cover_real = real,
            Ev::File(None) => return,
        }
    }
}

fn uxplay(name: &str, meta: &std::path::Path, cover: &std::path::Path, dacp: &std::path::Path) -> async_process::Command {
    let mut cmd = proc::command("uxplay");
    cmd.args(["-p", "-n", name, "-nh", "-md"])
        .arg(meta)
        .arg("-ca")
        .arg(cover)
        .arg("-dacp")
        .arg(dacp)
        .stdin(async_process::Stdio::null())
        .stderr(async_process::Stdio::null());
    cmd
}
