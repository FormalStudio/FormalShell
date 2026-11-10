//! `localsend-cli` as children. `recv` runs while
//! `localsend.receive` is on and the CLI is installed, restarted on a
//! doubling backoff, and each `Recv file` line it logs for a file that is
//! really there raises a RECEIVED toast; files something else drops into
//! the directory raise nothing. `scan` lists the peers, and `send` takes
//! one transfer at a time, its outcome read off stderr's ERROR lines since
//! the exit code alone says nothing about a failed file.

use std::sync::LazyLock;
use std::time::Duration;

use async_channel::{Receiver, Sender};
use fs_devices::localsend::{self as model, Peer, RecvEvent};
use fs_info::notifications::Urgency;
use futures_lite::FutureExt;
use serde_json::{Value, json};

use super::config::{self, Settings};
use super::notifications::{self, Op};
use super::proc;
use crate::runtime::{Ctx, now_ms};
use crate::settings::{LOCALSEND_ALIAS, LOCALSEND_DIR, LOCALSEND_RECEIVE};
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

    pub fn status(&self) -> Value {
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
    }

    pub fn peers_json(&self) -> Value {
        Value::Array(
            self.peers
                .iter()
                .map(|p| json!({"name": p.name, "version": p.version, "ip": p.ip, "port": p.port, "protocol": p.protocol}))
                .collect(),
        )
    }
}

/// What a clipboard entry sends: a file as is, text staged to one first.
pub enum Payload {
    Path(String),
    Text(String),
}

impl Payload {
    pub fn of(entry: &fs_menu::clipboard::entry::Entry) -> Payload {
        match entry.kind {
            fs_menu::clipboard::entry::EntryKind::Image => Payload::Path(entry.path.clone().unwrap_or_default()),
            fs_menu::clipboard::entry::EntryKind::Text => Payload::Text(entry.text.clone().unwrap_or_default()),
        }
    }
}

pub enum Cmd {
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

/// `localsend.receive`, `.alias` and `.dir` as written.
type Keys = (bool, String, String);

fn keys(settings: &Settings) -> Option<Keys> {
    Some((LOCALSEND_RECEIVE.get(settings), LOCALSEND_ALIAS.get(settings).to_owned(), LOCALSEND_DIR.get(settings).to_owned()))
}

enum Wake {
    Config(Option<Keys>),
    Cmd(Cmd),
    Scanned(String),
    Sent(proc::Done),
    Recv(proc::Event),
}

pub fn start(ctx: &Ctx) {
    ctx.spawn(run(ctx.clone()));
}

async fn run(ctx: Ctx) {
    let host = proc::capture(&proc::argv(&["hostname"]), Duration::from_secs(5)).await.stdout.trim().to_owned();
    let installed = proc::on_path("localsend-cli");
    let mut s = State { installed, ..State::default() };
    let (tx, rx) = async_channel::unbounded::<Wake>();
    // Back to the floor when the settings change, not when it starts.
    let mut recv = proc::Supervised::new(BASE_BACKOFF, MAX_BACKOFF, proc::Reset::Manual);
    let settings = config::subscribe();
    let mut config: Option<Keys> = None;
    ctx.publish(store::Diff::Localsend(Diff(s.clone())));
    loop {
        let wake = async { CHANNEL.1.recv().await.ok().map(Wake::Cmd) }
            .or(async { Some(Wake::Config(config::moved(&settings, &config, keys).await.and_then(|s| keys(&s)))) })
            .or(async { rx.recv().await.ok() })
            .or(async { Some(Wake::Recv(recv.next().await)) })
            .await;
        let Some(wake) = wake else { return };
        match wake {
            Wake::Config(None) => return,
            Wake::Config(Some(next)) => {
                config = Some(next.clone());
                let (receive, alias, dir) = next;
                let alias = if alias.is_empty() { if host.is_empty() { "FormalShell".to_owned() } else { host.clone() } } else { alias };
                let dir = if dir.is_empty() { crate::paths::home().join("Downloads").to_string_lossy().into_owned() } else { dir };
                (s.enabled, s.alias, s.dir) = (receive, alias, dir);
                recv.stop();
                s.receiving = false;
                if receive && s.installed {
                    recv.reset();
                    s.receiving = recv.start(recv_command(&s), proc::Read::Stderr).is_ok();
                }
            }
            Wake::Recv(proc::Event::Retry) => {
                if s.enabled && s.installed && recv.idle() {
                    s.receiving = recv.start(recv_command(&s), proc::Read::Stderr).is_ok();
                }
            }
            Wake::Recv(proc::Event::Exit(_)) => s.receiving = false,
            Wake::Recv(proc::Event::Line(line)) => match model::parse_recv_line(&line) {
                Some(RecvEvent::Error { message }) => s.last_error = message,
                Some(RecvEvent::Received { file, .. }) => {
                    let path = std::path::Path::new(&s.dir).join(&file);
                    if path.is_file() {
                        let open = |key: &str, label: &str, target: &str| notifications::LocalAction {
                            key: key.into(),
                            label: label.into(),
                            run: notifications::Local::Spawn(vec!["xdg-open".into(), target.into()]),
                        };
                        let actions = vec![open("open", "Open", &path.to_string_lossy()), open("reveal", "Show in Folder", &s.dir)];
                        ctx.publish(store::Diff::Notifications(notifications::Diff::Op(Op::NotifyRunning {
                            summary: "LOCALSEND RECEIVED".into(),
                            body: file,
                            urgency: Urgency::Normal,
                            actions,
                            image: String::new(),
                        })));
                    }
                    continue;
                }
                _ => continue,
            },
            Wake::Cmd(Cmd::Scan { force }) => {
                if !s.installed || s.scanning {
                    continue;
                }
                if !force && !s.peers.is_empty() && now_ms() as f64 - s.last_scan_at < SCAN_FRESH_MS {
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
                s.last_scan_at = now_ms() as f64;
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
                        let dir = crate::paths::shell_runtime();
                        let file = dir.join(format!("localsend-text-{}.txt", now_ms()));
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

/// `localsend-cli recv`, its stderr read line by line.
fn recv_command(s: &State) -> async_process::Command {
    let mut cmd = proc::command("localsend-cli");
    cmd.args(["recv", "-n", &s.alias, "-d", &s.dir]).stdin(async_process::Stdio::null()).stdout(async_process::Stdio::null());
    cmd
}
