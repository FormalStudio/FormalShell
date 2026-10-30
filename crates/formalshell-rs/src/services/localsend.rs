//! `localsend-cli` as children. `recv` runs while
//! `localsend.receive` is on and the CLI is installed, restarted on a
//! doubling backoff, and each `Recv file` line it logs for a file that is
//! really there raises a RECEIVED toast; files something else drops into
//! the directory raise nothing. `scan` lists the peers, and `send` takes
//! one transfer at a time, its outcome read off stderr's ERROR lines since
//! the exit code alone says nothing about a failed file.

use std::sync::LazyLock;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use async_channel::{Receiver, Sender};
use fs_devices::localsend::{self as model, Peer, RecvEvent};
use fs_info::notifications::Urgency;
use futures_lite::io::BufReader;
use futures_lite::{AsyncBufReadExt, FutureExt};
use serde_json::{Value, json};

use super::notifications::{self, Op};
use super::proc;
use crate::runtime::Ctx;
use crate::store;

const BASE_BACKOFF: Duration = Duration::from_secs(2);
const MAX_BACKOFF: Duration = Duration::from_secs(30);
const SCAN_FRESH_MS: f64 = 20000.0;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct State {
    pub installed: bool,
    pub receiving: bool,
    pub enabled: bool,
    pub alias: String,
    pub dir: String,
    pub scanning: bool,
    pub last_scan_at: f64,
    pub peers: Vec<Peer>,
    pub send_target: String,
    pub last_error: String,
}

pub struct Diff(pub State);

impl State {
    pub fn apply(&mut self, diff: Diff) -> bool {
        let changed = *self != diff.0;
        *self = diff.0;
        changed
    }

    pub fn status(&self) -> String {
        json!({
            "installed": self.installed,
            "receiving": self.receiving,
            "alias": self.alias,
            "dir": self.dir,
            "scanning": self.scanning,
            "lastScanAt": self.last_scan_at as u64,
            "peerCount": self.peers.len(),
            "busy": !self.send_target.is_empty(),
            "sendTarget": self.send_target,
            "lastError": self.last_error,
        })
        .to_string()
    }

    pub fn peers_json(&self) -> String {
        Value::Array(
            self.peers
                .iter()
                .map(|p| json!({"name": p.name, "version": p.version, "ip": p.ip, "port": p.port, "protocol": p.protocol}))
                .collect(),
        )
        .to_string()
    }
}

/// What a clipboard entry sends: a file as is, text staged to one first.
pub enum Payload {
    Path(String),
    Text(String),
}

impl Payload {
    pub fn of(entry: &fs_menu::clipboard::history::Entry) -> Payload {
        match entry.kind {
            fs_menu::clipboard::history::EntryKind::Image => Payload::Path(entry.path.clone().unwrap_or_default()),
            fs_menu::clipboard::history::EntryKind::Text => Payload::Text(entry.text.clone().unwrap_or_default()),
        }
    }
}

pub enum Cmd {
    /// `localsend.receive`, `.alias` and `.dir` as read.
    Config { receive: bool, alias: String, dir: String },
    Scan { force: bool },
    Send { peer: Peer, payload: Payload },
}

static CHANNEL: LazyLock<(Sender<Cmd>, Receiver<Cmd>)> = LazyLock::new(async_channel::unbounded);

/// Callable from any thread.
pub fn command(cmd: Cmd) {
    let _ = CHANNEL.0.try_send(cmd);
}

fn toast(ctx: &Ctx, summary: &str, body: &str, urgency: Urgency) {
    ctx.publish(store::Diff::Notifications(notifications::Diff::Op(Op::Notify(summary.into(), body.into(), urgency))));
}

fn now_ms() -> f64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0.0, |d| d.as_millis() as f64)
}

enum Wake {
    Cmd(Cmd),
    Scanned(String),
    Sent(proc::Done),
    Recv(Option<RecvEvent>),
    /// The receiver of this generation exited; respawn after the backoff.
    RecvGone(u64),
    Retry,
}

pub async fn run(ctx: Ctx) {
    let host = proc::capture(&proc::argv(&["hostname"]), Duration::from_secs(5)).await.stdout.trim().to_owned();
    let installed = proc::capture(&proc::argv(&["sh", "-c", "command -v localsend-cli >/dev/null 2>&1"]), Duration::from_secs(5)).await.code == 0;
    let mut s = State { installed, ..State::default() };
    let (tx, rx) = async_channel::unbounded::<Wake>();
    let mut recv: Option<async_process::Child> = None;
    let mut backoff = BASE_BACKOFF;
    let mut config: Option<(bool, String, String)> = None;
    let mut generation: u64 = 0;
    ctx.publish(store::Diff::Localsend(Diff(s.clone())));
    loop {
        let wake = async { CHANNEL.1.recv().await.ok().map(Wake::Cmd) }.or(async { rx.recv().await.ok() }).await;
        let Some(wake) = wake else { return };
        match wake {
            Wake::Cmd(Cmd::Config { receive, alias, dir }) => {
                let alias = if alias.is_empty() { if host.is_empty() { "FormalShell".to_owned() } else { host.clone() } } else { alias };
                let dir = if dir.is_empty() { format!("{}/Downloads", std::env::var("HOME").unwrap_or_default()) } else { dir };
                let next = (receive, alias.clone(), dir.clone());
                if config.as_ref() == Some(&next) {
                    continue;
                }
                config = Some(next);
                (s.enabled, s.alias, s.dir) = (receive, alias, dir);
                recv = None;
                generation += 1;
                s.receiving = false;
                if receive && s.installed {
                    backoff = BASE_BACKOFF;
                    recv = spawn_recv(&ctx, &s, generation, tx.clone());
                    s.receiving = recv.is_some();
                }
            }
            Wake::Retry => {
                if s.enabled && s.installed && recv.is_none() {
                    generation += 1;
                    recv = spawn_recv(&ctx, &s, generation, tx.clone());
                    s.receiving = recv.is_some();
                }
            }
            Wake::RecvGone(g) if g != generation => continue,
            Wake::RecvGone(_) => {
                recv = None;
                s.receiving = false;
                if s.enabled && s.installed {
                    let (t, wait) = (tx.clone(), backoff);
                    backoff = (backoff * 2).min(MAX_BACKOFF);
                    ctx.spawn(async move {
                        async_io::Timer::after(wait).await;
                        let _ = t.send(Wake::Retry).await;
                    });
                }
            }
            Wake::Recv(Some(RecvEvent::Error { message })) => s.last_error = message,
            Wake::Recv(Some(RecvEvent::Received { file, .. })) => {
                let path = std::path::Path::new(&s.dir).join(&file);
                if path.is_file() {
                    let open = |key: &str, label: &str, target: &str| notifications::LocalAction {
                        key: key.into(),
                        label: label.into(),
                        argv: vec!["xdg-open".into(), target.into()],
                    };
                    let actions = vec![open("open", "Open", &path.to_string_lossy()), open("reveal", "Show in Folder", &s.dir)];
                    ctx.publish(store::Diff::Notifications(notifications::Diff::Op(Op::NotifyRunning(
                        "LOCALSEND RECEIVED".into(),
                        file,
                        Urgency::Normal,
                        actions,
                    ))));
                }
                continue;
            }
            Wake::Recv(_) => continue,
            Wake::Cmd(Cmd::Scan { force }) => {
                if !s.installed || s.scanning {
                    continue;
                }
                if !force && !s.peers.is_empty() && now_ms() - s.last_scan_at < SCAN_FRESH_MS {
                    continue;
                }
                s.scanning = true;
                let t = tx.clone();
                ctx.spawn(async move {
                    let done = proc::capture(&proc::argv(&["localsend-cli", "scan", "-t", "4"]), Duration::from_secs(30)).await;
                    let _ = t.send(Wake::Scanned(done.stdout)).await;
                });
            }
            Wake::Scanned(out) => {
                s.peers = model::parse_scan(&out);
                s.last_scan_at = now_ms();
                s.scanning = false;
            }
            Wake::Cmd(Cmd::Send { peer, payload }) => {
                if !s.send_target.is_empty() {
                    toast(&ctx, "LOCALSEND BUSY", &format!("Still sending to {}", s.send_target), Urgency::Normal);
                    continue;
                }
                let path = match payload {
                    Payload::Path(p) => p,
                    Payload::Text(text) => {
                        let dir = std::path::PathBuf::from(std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".into())).join("formalshell");
                        let file = dir.join(format!("localsend-text-{}.txt", now_ms() as u64));
                        let staged = std::fs::create_dir_all(&dir).and_then(|_| std::fs::write(&file, text));
                        if staged.is_err() {
                            toast(&ctx, "LOCALSEND FAILED", "Could not stage the clipboard text to send", Urgency::Critical);
                            continue;
                        }
                        file.to_string_lossy().into_owned()
                    }
                };
                s.send_target = peer.name.clone();
                toast(&ctx, "LOCALSEND SENDING", &format!("1 item to {}", peer.name), Urgency::Normal);
                let (t, argv) = (tx.clone(), proc::argv(&["localsend-cli", "send", "--ip", &peer.ip, "-f", &path]));
                ctx.spawn(async move {
                    let done = proc::capture_err(&argv, Duration::from_secs(3600)).await;
                    let _ = t.send(Wake::Sent(done)).await;
                });
            }
            Wake::Sent(done) => {
                let outcome = model::send_outcome(done.code, &done.stderr);
                if outcome.ok {
                    toast(&ctx, "LOCALSEND SENT", &format!("Delivered to {}", s.send_target), Urgency::Normal);
                } else {
                    let reason = outcome
                        .failed
                        .first()
                        .map(|f| if f.error.is_empty() { f.msg.clone() } else { f.error.clone() })
                        .unwrap_or_else(|| format!("localsend-cli exited {}", done.code));
                    toast(&ctx, "LOCALSEND FAILED", &format!("{}: {reason}", s.send_target), Urgency::Critical);
                    s.last_error = reason;
                }
                s.send_target.clear();
            }
        }
        ctx.publish(store::Diff::Localsend(Diff(s.clone())));
    }
}

/// `localsend-cli recv`, dying with the shell, its stderr read line by line.
fn spawn_recv(ctx: &Ctx, s: &State, generation: u64, tx: Sender<Wake>) -> Option<async_process::Child> {
    let mut child = crate::services::proc::command("setpriv")
        .args(["--pdeathsig", "TERM", "--", "localsend-cli", "recv", "-n", &s.alias, "-d", &s.dir])
        .stdin(async_process::Stdio::null())
        .stdout(async_process::Stdio::null())
        .stderr(async_process::Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .ok()?;
    let err = child.stderr.take()?;
    ctx.spawn(async move {
        let mut lines = BufReader::new(err).lines();
        while let Some(Ok(line)) = futures_lite::StreamExt::next(&mut lines).await {
            if tx.send(Wake::Recv(model::parse_recv_line(&line))).await.is_err() {
                return;
            }
        }
        let _ = tx.send(Wake::RecvGone(generation)).await;
    });
    Some(child)
}
