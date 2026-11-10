//! A Wi-Fi network as the shell reads it off NetworkManager, the one
//! action in flight on one, and the last one that failed.

use fs_types::Fraction;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Row {
    /// Empty for a hidden network.
    pub ssid: String,
    pub known: bool,
    pub connected: bool,
    pub changing: bool,
    pub secured: bool,
    pub enterprise: bool,
    /// An access point broadcasts it; false only for a saved network.
    pub in_range: bool,
    pub signal: Fraction,
    /// `Security::label`'s name for its class.
    pub security: String,
    /// The device's addresses on it, `address/prefix`, while connected.
    pub addresses: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActionKind {
    Connect,
    Disconnect,
    Forget,
}

impl ActionKind {
    pub fn as_str(self) -> &'static str {
        match self {
            ActionKind::Connect => "connect",
            ActionKind::Disconnect => "disconnect",
            ActionKind::Forget => "forget",
        }
    }

    /// What a row reads while the action runs on it.
    pub fn busy(self) -> &'static str {
        match self {
            ActionKind::Connect => "Connecting",
            ActionKind::Disconnect => "Disconnecting",
            ActionKind::Forget => "Forgetting",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Failure {
    pub ssid: String,
    pub text: String,
    /// Fixed by typing the secret again rather than retrying.
    pub secret: bool,
}

/// Whether connecting to `row` asks for its secret first: a secured
/// network nobody knows, or one whose last attempt failed on the secret,
/// since a wrong password is retyped, never retried.
pub fn asks_secret(row: &Row, failure: Option<&Failure>) -> bool {
    failure.is_some_and(|f| f.ssid == row.ssid && f.secret) || (row.secured && !row.known)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_secret_is_asked_for_a_stranger_or_after_a_wrong_one() {
        let saved = Row { ssid: "Home".into(), known: true, secured: true, ..Row::default() };
        assert!(!asks_secret(&saved, None));
        assert!(asks_secret(&Row { known: false, ..saved.clone() }, None));
        assert!(!asks_secret(&Row { secured: false, known: false, ..saved.clone() }, None));
        let wrong = Failure { ssid: "Home".into(), text: "Wrong password".into(), secret: true };
        assert!(asks_secret(&saved, Some(&wrong)));
        assert!(!asks_secret(&saved, Some(&Failure { ssid: "Cafe".into(), ..wrong.clone() })));
        assert!(!asks_secret(&saved, Some(&Failure { secret: false, ..wrong })));
    }
}
