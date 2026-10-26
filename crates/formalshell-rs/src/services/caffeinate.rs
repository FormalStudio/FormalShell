//! What caffeinate reads as: the toggle, the surface holding the Wayland
//! idle inhibitor, the session's idle state off ext-idle-notify, and the
//! `org.freedesktop.ScreenSaver` cookies (`services/inhibit.rs`) holding the
//! same inhibitor. The UI thread owns the first three (they are Wayland
//! objects and events), so the slice is written from there and read by the
//! indicators cell.

use serde_json::json;

#[derive(Clone, Debug, PartialEq)]
pub struct Holder {
    pub cookie: u32,
    pub app: String,
    pub reason: String,
    /// The client's unique bus name.
    pub sender: String,
}

#[derive(Default)]
pub struct State {
    /// The toggle, never persisted: a restart comes back to
    /// `caffeinate.onStartup`.
    pub active: bool,
    /// The inhibitor is created and its surface mapped.
    pub inhibiting: bool,
    /// ext-idle-notify (respecting inhibitors) says the session is idle.
    pub idle: bool,
    /// Live ScreenSaver cookies in cookie order.
    pub holders: Vec<Holder>,
}

pub enum Diff {
    Active(bool),
    Inhibiting(bool),
    Idle(bool),
    Holders(Vec<Holder>),
}

impl State {
    pub fn apply(&mut self, diff: Diff) -> bool {
        let (slot, value) = match diff {
            Diff::Active(v) => (&mut self.active, v),
            Diff::Inhibiting(v) => (&mut self.inhibiting, v),
            Diff::Idle(v) => (&mut self.idle, v),
            Diff::Holders(v) => {
                let changed = self.holders != v;
                self.holders = v;
                return changed;
            }
        };
        let changed = *slot != value;
        *slot = value;
        changed
    }

    /// Whether the inhibitor should be up: the toggle or any cookie.
    pub fn held(&self) -> bool {
        self.active || !self.holders.is_empty()
    }

    /// The IPC `status` reply.
    pub fn status(&self) -> String {
        let holders: Vec<_> =
            self.holders.iter().map(|h| json!({"app": h.app, "reason": h.reason, "sender": h.sender})).collect();
        format!(
            r#"{{"active":{},"inhibiting":{},"isIdle":{},"holders":{}}}"#,
            self.active,
            self.inhibiting,
            self.idle,
            serde_json::Value::Array(holders)
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_reads_like_the_golden_reply() {
        let mut s = State::default();
        assert_eq!(s.status(), r#"{"active":false,"inhibiting":false,"isIdle":false,"holders":[]}"#);
        assert!(s.apply(Diff::Active(true)));
        assert!(!s.apply(Diff::Active(true)));
        assert_eq!(s.status(), r#"{"active":true,"inhibiting":false,"isIdle":false,"holders":[]}"#);
    }

    #[test]
    fn a_cookie_holds_without_the_toggle() {
        let mut s = State::default();
        let h = Holder { cookie: 1, app: "Steam".into(), reason: "game".into(), sender: ":1.5".into() };
        assert!(s.apply(Diff::Holders(vec![h.clone()])));
        assert!(!s.apply(Diff::Holders(vec![h])));
        assert!(s.held() && !s.active);
        assert_eq!(
            s.status(),
            r#"{"active":false,"inhibiting":false,"isIdle":false,"holders":[{"app":"Steam","reason":"game","sender":":1.5"}]}"#
        );
        assert!(s.apply(Diff::Holders(vec![])));
        assert!(!s.held());
    }
}
