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
use std::time::{Duration, Instant};

use async_io::Timer;
use fs_media::airplay::{self, Event};
use futures_lite::io::{AsyncBufReadExt, BufReader};
use futures_lite::{FutureExt, StreamExt};
use serde_json::{Value, json};

use super::media::Settings;
use super::watch::Watch;
use crate::runtime::Ctx;
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
    let base = std::env::var("XDG_RUNTIME_DIR").ok().filter(|d| !d.is_empty()).unwrap_or_else(|| "/tmp".into());
    PathBuf::from(base).join("formalshell").join("airplay")
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
    Line(Option<String>),
    Exit,
    Retry,
    File(Option<File>),
}

pub async fn run(ctx: Ctx) {
    let settings_rx = super::media::subscribe();
    let dir = dir();
    let meta = dir.join("meta");
    let cover = dir.join("cover.jpg");
    let dacp = dir.join("dacp");
    // Neither file's directory exists on its own; it is made before uxplay
    // runs and before the watches are placed in it.
    let dir_ready = std::fs::create_dir_all(&dir).is_ok();
    let installed = crate::services::proc::command("sh")
        .args(["-c", "command -v uxplay >/dev/null 2>&1"])
        .status()
        .await
        .is_ok_and(|s| s.success());
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
    let mut child: Option<(async_process::Child, futures_lite::io::Lines<BufReader<async_process::ChildStdout>>)> = None;
    let mut retry_at: Option<Instant> = None;
    let mut backoff = BASE_BACKOFF;
    // The three files as last read, shown only while a client is connected:
    // UxPlay never clears `-md` and `-ca` on disconnect.
    let mut dacp_up = false;
    let mut shown_meta = airplay::Metadata::default();
    let mut cover_real = false;
    let mut last = state.clone();
    let mut was_active = false;

    loop {
        state.enabled = settings.airplay_enable;
        state.name = airplay::resolve_name(&settings.airplay_name, &host);
        let should_run = state.enabled && installed && dir_ready;
        if should_run && child.is_none() && retry_at.is_none() {
            backoff = BASE_BACKOFF;
            child = spawn(&state.name, &meta, &cover, &dacp);
            if child.is_none() {
                retry_at = Some(Instant::now() + backoff);
            }
        } else if !should_run && let Some((mut c, _)) = child.take() {
            let _ = c.kill();
            retry_at = None;
        }
        state.running = child.is_some();
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

        let timer = async {
            match retry_at {
                Some(at) => {
                    Timer::at(at).await;
                    Ev::Retry
                }
                None => std::future::pending().await,
            }
        };
        let event = {
            let sets = async { Ev::Settings(settings_rx.recv().await.ok()) };
            let file = async { Ev::File(files.recv().await.ok()) };
            let base = sets.or(file).or(timer);
            match child.as_mut() {
                Some((c, lines)) => {
                    let line = async { Ev::Line(lines.next().await.and_then(Result::ok)) };
                    let exit = async {
                        let _ = c.status().await;
                        Ev::Exit
                    };
                    base.or(line).or(exit).await
                }
                None => base.await,
            }
        };
        // A closed stdout is ready on every poll and wins the select over the
        // exit future, so the exit is awaited here instead.
        let event = match (event, child.as_mut()) {
            (Ev::Line(None), Some((c, _))) => {
                let _ = c.status().await;
                Ev::Exit
            }
            (Ev::Line(None), None) => Ev::Exit,
            (event, _) => event,
        };
        match event {
            Ev::Settings(Some(next)) => settings = next,
            Ev::Settings(None) => return,
            Ev::Line(Some(line)) => match airplay::parse_line(&line) {
                Some(Event::Connected { name, .. }) => {
                    state.client = name;
                    state.last_error.clear();
                }
                Some(Event::Disconnected) => state.client.clear(),
                Some(Event::Error { message }) => state.last_error = message,
                None => {}
            },
            Ev::Line(None) => unreachable!("a closed stdout became an exit above"),
            Ev::Exit => {
                child = None;
                dacp_up = false;
                if should_run {
                    retry_at = Some(Instant::now() + backoff);
                    backoff = (backoff * 2).min(MAX_BACKOFF);
                }
            }
            Ev::Retry => retry_at = None,
            Ev::File(Some(File::Dacp(up))) => dacp_up = up,
            Ev::File(Some(File::Meta(m))) => shown_meta = m,
            Ev::File(Some(File::Cover(real))) => cover_real = real,
            Ev::File(None) => return,
        }
    }
}

fn spawn(
    name: &str,
    meta: &std::path::Path,
    cover: &std::path::Path,
    dacp: &std::path::Path,
) -> Option<(async_process::Child, futures_lite::io::Lines<BufReader<async_process::ChildStdout>>)> {
    let mut child = crate::services::proc::command("setpriv")
        .args(["--pdeathsig", "TERM", "--", "uxplay", "-p", "-n", name, "-nh", "-md"])
        .arg(meta)
        .arg("-ca")
        .arg(cover)
        .arg("-dacp")
        .arg(dacp)
        .stdin(async_process::Stdio::null())
        .stdout(async_process::Stdio::piped())
        .stderr(async_process::Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .ok()?;
    let lines = BufReader::new(child.stdout.take()?).lines();
    Some((child, lines))
}
