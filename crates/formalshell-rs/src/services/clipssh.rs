//! One `clipssh <alias>` at a time, which ships the
//! clipboard's image to a host over ssh and puts the remote path back on
//! the clipboard. The aliases are clipssh's own `~/.clipssh/aliases`,
//! watched. A send while one runs is refused with a toast rather than
//! queued; the bar's indicator is up for exactly as long as one runs.

use std::path::PathBuf;
use std::sync::LazyLock;
use std::time::Duration;

use async_channel::{Receiver, Sender};
use fs_info::notifications::Urgency;
use fs_menu::providers::{self, ClipsshAlias, ClipsshOutcome};
use futures_lite::{FutureExt, future};

use super::notifications::{self, Op};
use super::proc;
use super::watch::Watch;
use crate::runtime::Ctx;
use crate::store;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct State {
    pub aliases: Vec<ClipsshAlias>,
    /// The alias a send is running to, empty while idle.
    pub target: String,
}

pub struct Diff(pub State);

impl State {
    pub fn apply(&mut self, diff: Diff) -> bool {
        let changed = *self != diff.0;
        *self = diff.0;
        changed
    }
}

pub enum Cmd {
    /// What the clipboard holds now, to the alias.
    Send(String),
    /// This capture put on the clipboard first, then sent.
    SendImage(String, String),
    /// `clipssh.autoSendImages`: a fresh image capture, sent to the alias
    /// `clipssh.alias` resolves to when the key is on.
    AutoImage { configured: String, enabled: bool },
}

static CHANNEL: LazyLock<(Sender<Cmd>, Receiver<Cmd>)> = LazyLock::new(async_channel::unbounded);

/// Callable from any thread.
pub fn command(cmd: Cmd) {
    let _ = CHANNEL.0.try_send(cmd);
}

/// `clipssh.alias`, else the only alias saved; `ask` (or several saved
/// with none named) is none.
pub fn resolve_alias(configured: &str, aliases: &[ClipsshAlias]) -> Option<String> {
    match configured {
        "ask" => None,
        "" => (aliases.len() == 1).then(|| aliases[0].name.clone()),
        name => Some(name.to_owned()),
    }
}

fn aliases_path() -> PathBuf {
    crate::paths::home().join(".clipssh/aliases")
}

fn toast(ctx: &Ctx, summary: &str, body: &str, urgency: Urgency) {
    ctx.publish(store::Diff::Notifications(notifications::Diff::Op(Op::Notify(summary.into(), body.into(), urgency))));
}

enum Wake {
    Aliases,
    Cmd(Cmd),
    Done(proc::Done),
}

pub fn start(ctx: &Ctx) {
    ctx.spawn(run(ctx.clone()));
}

async fn run(ctx: Ctx) {
    let path = aliases_path();
    let read = |p: &PathBuf| providers::clipssh_aliases(&std::fs::read_to_string(p).unwrap_or_default());
    let mut state = State { aliases: read(&path), target: String::new() };
    ctx.publish(store::Diff::Clipssh(Diff(state.clone())));
    let mut watch = Watch::new(path.clone()).ok();
    let (done_tx, done_rx) = async_channel::unbounded::<proc::Done>();
    let mut warned = false;
    loop {
        let changed = async {
            match watch.as_mut() {
                Some(w) => {
                    w.changed().await;
                    Some(Wake::Aliases)
                }
                None => future::pending().await,
            }
        };
        let wake = changed
            .or(async { CHANNEL.1.recv().await.ok().map(Wake::Cmd) })
            .or(async { done_rx.recv().await.ok().map(Wake::Done) })
            .await;
        let Some(wake) = wake else { return };
        let (alias, image) = match wake {
            Wake::Aliases => {
                state.aliases = read(&path);
                ctx.publish(store::Diff::Clipssh(Diff(state.clone())));
                continue;
            }
            Wake::Done(done) => {
                match providers::clipssh_outcome(done.code, &done.stdout, &done.stderr) {
                    ClipsshOutcome::Ok { path } => {
                        let body = if path.is_empty() { "Remote path is on the clipboard".to_owned() } else { format!("{path} is on the clipboard") };
                        toast(&ctx, "CLIPSSH COPIED", &body, Urgency::Normal);
                    }
                    ClipsshOutcome::Failed { error } => toast(&ctx, "CLIPSSH FAILED", &format!("{}: {error}", state.target), Urgency::Critical),
                }
                state.target.clear();
                ctx.publish(store::Diff::Clipssh(Diff(state.clone())));
                continue;
            }
            Wake::Cmd(Cmd::Send(alias)) => (alias, String::new()),
            Wake::Cmd(Cmd::SendImage(alias, path)) if !path.is_empty() => (alias, path),
            Wake::Cmd(Cmd::SendImage(..)) => continue,
            Wake::Cmd(Cmd::AutoImage { configured, enabled }) => {
                if !enabled || !state.target.is_empty() {
                    continue;
                }
                match resolve_alias(&configured, &state.aliases) {
                    Some(a) => (a, String::new()),
                    None => {
                        if !warned {
                            warned = true;
                            let body = if state.aliases.is_empty() {
                                "clipssh.autoSendImages is on with no aliases saved".to_owned()
                            } else {
                                format!("clipssh.autoSendImages is on: set clipssh.alias to one of {} aliases", state.aliases.len())
                            };
                            toast(&ctx, "CLIPSSH NOT SENDING", &body, Urgency::Critical);
                        }
                        continue;
                    }
                }
            }
        };
        if alias.is_empty() {
            continue;
        }
        if !state.target.is_empty() {
            toast(&ctx, "CLIPSSH BUSY", &format!("Still sending to {}", state.target), Urgency::Normal);
            continue;
        }
        state.target = alias.clone();
        ctx.publish(store::Diff::Clipssh(Diff(state.clone())));
        toast(&ctx, "CLIPSSH SENDING", &format!("Clipboard image to {alias}"), Urgency::Normal);
        let argv = if image.is_empty() {
            proc::argv(&["sh", "-c", "exec clipssh \"$1\"", "sh", &alias])
        } else {
            proc::argv(&[
                "sh",
                "-c",
                "wl-copy --type image/png < \"$2\" || { printf \"Error: could not put the image on the clipboard\\n\" >&2; exit 1; }; exec clipssh \"$1\"",
                "sh",
                &alias,
                &image,
            ])
        };
        let tx = done_tx.clone();
        ctx.spawn(async move {
            let done = proc::capture_exit(&argv, Duration::from_secs(3600)).await;
            let _ = tx.send(done).await;
        });
    }
}
