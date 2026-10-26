//! `formalshell-depth` over the picker directory while
//! `wallpaper.depthGenerate` is on: once when it is turned on, and again
//! after the directory settles from a change. The CLI skips pictures whose
//! map is current, so a run over an unchanged directory only reads mtimes.
//! It writes into dot directories (`.depth/`), which the watch never
//! follows, so its own writes do not start another run.

use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use std::time::Duration;

use async_channel::{Receiver, Sender};
use futures_lite::FutureExt;
use serde_json::{Value, json};

use super::proc;
use crate::runtime::Ctx;
use crate::store;

/// A copy into the directory is a burst of events per file and often
/// several files; one run per burst.
const SETTLE: Duration = Duration::from_secs(2);
/// The model on a CPU takes seconds per picture; a first run over a full
/// directory can take many minutes.
const RUN_TIMEOUT: Duration = Duration::from_secs(3600);

#[derive(Clone, Debug, Default, PartialEq)]
pub struct State {
    pub enabled: bool,
    pub dir: String,
    /// None until a run has said.
    pub installed: Option<bool>,
    pub running: bool,
    /// The pictures the last run wrote a map for.
    pub wrote: Vec<String>,
    pub failed: usize,
    pub last_code: Option<i32>,
}

pub struct Diff(pub State);

impl State {
    pub fn apply(&mut self, diff: Diff) -> bool {
        let changed = *self != diff.0;
        *self = diff.0;
        changed
    }

    /// The finished run wrote `picture`'s map.
    pub fn wrote(&self, picture: &str) -> bool {
        !self.running && self.wrote.iter().any(|w| same_file(w, picture))
    }

    pub fn status(&self) -> Value {
        json!({
            "enabled": self.enabled,
            "dir": self.dir,
            "installed": self.installed,
            "running": self.running,
            "wrote": self.wrote.len(),
            "failed": self.failed,
            "lastCode": self.last_code,
        })
    }
}

fn same_file(a: &str, b: &str) -> bool {
    a == b || matches!((std::fs::canonicalize(a), std::fs::canonicalize(b)), (Ok(x), Ok(y)) if x == y)
}

pub enum Cmd {
    Config { enabled: bool, dir: String },
}

enum Wake {
    Cmd(Cmd),
    Changed(u64),
    Done(proc::Done),
}

static CHANNEL: LazyLock<(Sender<Cmd>, Receiver<Cmd>)> = LazyLock::new(async_channel::unbounded);

/// Callable from any thread.
pub fn command(cmd: Cmd) {
    let _ = CHANNEL.0.try_send(cmd);
}

pub async fn run(ctx: Ctx) {
    let (tx, rx) = async_channel::unbounded::<Wake>();
    let mut s = State::default();
    let mut generation: u64 = 0;
    // Dropped to end the watch on the directory it was made for.
    let mut _stop: Option<Sender<()>> = None;
    let mut pending = false;
    let mut logged = false;
    loop {
        let wake = async { CHANNEL.1.recv().await.ok().map(Wake::Cmd) }.or(async { rx.recv().await.ok() }).await;
        let Some(wake) = wake else { return };
        let want = match wake {
            Wake::Cmd(Cmd::Config { enabled, dir }) => {
                if (enabled, &dir) == (s.enabled, &s.dir) {
                    continue;
                }
                (s.enabled, s.dir) = (enabled, dir);
                generation += 1;
                _stop = None;
                if s.enabled && !s.dir.is_empty() {
                    let (stop_tx, stop_rx) = async_channel::bounded::<()>(1);
                    _stop = Some(stop_tx);
                    ctx.spawn(watch(PathBuf::from(&s.dir), generation, tx.clone(), stop_rx));
                }
                true
            }
            Wake::Changed(g) => g == generation,
            Wake::Done(done) => {
                s.running = false;
                s.last_code = Some(done.code);
                if done.code == proc::MISSING {
                    s.installed = Some(false);
                    if !logged {
                        eprintln!("depth: formalshell-depth is not on PATH, no depth maps");
                        logged = true;
                    }
                } else {
                    s.installed = Some(true);
                    s.wrote = done.stdout.lines().filter_map(|l| l.strip_prefix("wrote ")).map(str::to_owned).collect();
                    s.failed = done.stdout.lines().filter(|l| l.starts_with("failed ")).count();
                    if done.code != 0 {
                        eprintln!("depth: exit {}, {} failed: {}", done.code, s.failed, done.stderr.trim());
                    }
                }
                std::mem::take(&mut pending)
            }
        };
        if want && s.enabled && !s.dir.is_empty() && s.installed != Some(false) {
            if s.running {
                pending = true;
            } else {
                s.running = true;
                s.wrote.clear();
                let (argv, tx) = (proc::argv(&["formalshell-depth", &s.dir]), tx.clone());
                ctx.spawn(async move {
                    let _ = tx.send(Wake::Done(proc::capture(&argv, RUN_TIMEOUT).await)).await;
                });
            }
        }
        ctx.publish(store::Diff::Depth(Diff(s.clone())));
    }
}

/// `dir` and every directory under it but dot ones, the CLI's own walk.
fn dirs(dir: &Path) -> Vec<PathBuf> {
    let mut out = vec![dir.to_path_buf()];
    let mut i = 0;
    while i < out.len() {
        if let Ok(read) = std::fs::read_dir(&out[i]) {
            let mut subs: Vec<PathBuf> = read
                .flatten()
                .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()) && !e.file_name().to_string_lossy().starts_with('.'))
                .map(|e| e.path())
                .collect();
            subs.sort();
            out.extend(subs);
        }
        i += 1;
    }
    out
}

#[cfg(target_os = "linux")]
async fn watch(dir: PathBuf, generation: u64, tx: Sender<Wake>, stop: Receiver<()>) {
    use std::os::fd::AsFd;

    use inotify::{Inotify, WatchMask};

    let Ok(mut inotify) = Inotify::init() else { return };
    let mask = WatchMask::CREATE | WatchMask::DELETE | WatchMask::MOVED_TO | WatchMask::MOVED_FROM | WatchMask::CLOSE_WRITE;
    let Ok(fd) = inotify.as_fd().try_clone_to_owned() else { return };
    let Ok(ready) = async_io::Async::new(fd) else { return };
    let mut buffer = [0u8; 4096];
    let arm = |inotify: &mut Inotify| {
        for d in dirs(&dir) {
            let _ = inotify.watches().add(&d, mask);
        }
    };
    arm(&mut inotify);
    let stopped = || async {
        let _ = stop.recv().await;
        false
    };
    loop {
        if !async { ready.readable().await.is_ok() }.or(stopped()).await {
            return;
        }
        let mut seen = false;
        while let Ok(events) = inotify.read_events(&mut buffer) {
            let mut any = false;
            for e in events {
                any = true;
                seen |= e.name.is_some_and(|n| !n.to_string_lossy().starts_with('.'));
            }
            if !any {
                break;
            }
        }
        if !seen {
            continue;
        }
        if !async {
            async_io::Timer::after(SETTLE).await;
            true
        }
        .or(stopped())
        .await
        {
            return;
        }
        while inotify.read_events(&mut buffer).is_ok_and(|e| e.count() > 0) {}
        arm(&mut inotify);
        if tx.send(Wake::Changed(generation)).await.is_err() {
            return;
        }
    }
}

#[cfg(not(target_os = "linux"))]
async fn watch(_dir: PathBuf, _generation: u64, _tx: Sender<Wake>, _stop: Receiver<()>) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_walk_skips_dot_directories() {
        let root = tempfile::tempdir().unwrap();
        for d in ["Dark", ".depth", "Dark/.depth", "Light/nested"] {
            std::fs::create_dir_all(root.path().join(d)).unwrap();
        }
        let found: Vec<String> = dirs(root.path()).iter().map(|p| p.strip_prefix(root.path()).unwrap().to_string_lossy().into_owned()).collect();
        assert_eq!(found, ["", "Dark", "Light", "Light/nested"]);
    }
}
