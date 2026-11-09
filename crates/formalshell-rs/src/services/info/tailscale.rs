//! `tailscale status --json` on a minute. No
//! CLI hides the cell; a daemon that answers badly reads as an error and
//! one that wants a login as its own state. The panel's connection switch
//! runs `tailscale up` or `down` here, one at a time, and the state it
//! leaves rides the same slice.

use std::cell::{Cell, RefCell};
use std::time::Duration;

use fs_devices::tailscale::{self, Status};
use futures_lite::FutureExt;

use super::{changed, idle, kicked, settings};
use crate::runtime::Ctx;
use crate::services::proc::{self, MISSING, TIMED_OUT};
use crate::services::wants::Source;
use crate::store;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Poll {
    #[default]
    Unknown,
    Missing,
    Error,
    NeedsLogin,
    Ok,
}

/// The one `up` or `down` in flight.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Action {
    #[default]
    None,
    Up,
    Down,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct State {
    pub poll: Poll,
    /// Meaningful only while `poll` is `Ok` or `NeedsLogin`.
    pub status: Option<Status>,
    pub action: Action,
    /// "NOT OPERATOR", "FAILED" or "TIMED OUT" until the next attempt.
    pub error: &'static str,
}

pub fn parse(code: i32, stdout: &str) -> State {
    if code == MISSING {
        return State { poll: Poll::Missing, ..State::default() };
    }
    let status = tailscale::parse_status(stdout);
    if !status.ok {
        return State { poll: Poll::Error, ..State::default() };
    }
    let poll = if status.needs_login { Poll::NeedsLogin } else { Poll::Ok };
    State { poll, status: Some(status), ..State::default() }
}

fn read() -> Duration {
    settings().interval("tailscale.intervalMs", 60_000)
}

thread_local! {
    static POLLED: RefCell<State> = RefCell::new(State::default());
    static ACTION: Cell<Action> = const { Cell::new(Action::None) };
    static ERROR: Cell<&'static str> = const { Cell::new("") };
}

/// The last poll, with the action state a poll does not know about.
fn publish(ctx: &Ctx) {
    let state = POLLED.with_borrow(|p| State { action: ACTION.get(), error: ERROR.get(), ..p.clone() });
    ctx.publish(store::Diff::Tailscale(state));
}

async fn poll(ctx: &Ctx) {
    let done = proc::capture(&proc::argv(&["tailscale", "status", "--json"]), Duration::from_secs(30)).await;
    POLLED.with_borrow_mut(|p| *p = parse(done.code, &done.stdout));
    publish(ctx);
}

pub async fn run(ctx: Ctx) {
    let rx = changed();
    let kick = kicked(Source::Tailscale);
    while kick.try_recv().is_ok() {}
    loop {
        let interval = read();
        poll(&ctx).await;
        let asked = async {
            let _ = kick.recv().await;
        };
        asked.or(idle(interval, &rx, &interval, read)).await;
    }
}

/// Connects or disconnects, one at a time. A permission failure reads as
/// NOT OPERATOR: 1.98's own "Access denied" wrapper, whose host-side fix is
/// `tailscale set --operator=$USER`.
pub fn toggle(ctx: &Ctx, running: bool) {
    if ACTION.get() != Action::None {
        return;
    }
    let (verb, action) = if running { ("down", Action::Down) } else { ("up", Action::Up) };
    ACTION.set(action);
    ERROR.set("");
    publish(ctx);
    let task = ctx.clone();
    ctx.spawn(async move {
        let script = format!("command -v tailscale >/dev/null 2>&1 || exit 127; exec tailscale {verb}");
        let done = proc::capture_err(&["sh".into(), "-c".into(), script], Duration::from_secs(15)).await;
        ERROR.set(match done.code {
            0 => "",
            TIMED_OUT => "TIMED OUT",
            MISSING => "FAILED",
            _ if done.stderr.to_lowercase().contains("access denied") => "NOT OPERATOR",
            _ => "FAILED",
        });
        ACTION.set(Action::None);
        publish(&task);
        poll(&task).await;
    });
}

/// Puts an address on the clipboard.
pub fn copy(ctx: &Ctx, text: String) {
    ctx.spawn(async move {
        proc::capture(&["wl-copy".into(), text], Duration::from_secs(5)).await;
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn states() {
        assert_eq!(parse(127, "").poll, Poll::Missing);
        assert_eq!(parse(1, "failed to connect").poll, Poll::Error);
        assert_eq!(parse(0, r#"{"BackendState":"NeedsLogin"}"#).poll, Poll::NeedsLogin);
        let running = parse(0, r#"{"BackendState":"Running"}"#);
        assert!(running.poll == Poll::Ok && running.status.is_some_and(|s| s.running));
    }
}
