//! Model for the agents service. No IO, so the reply parsing and the poll
//! command can be tested head-on against fixture strings rather than a live
//! T3 Code server.
//!
//! Each host in `workspaces.agentHosts` runs a T3 Code server and ships
//! `~/.local/bin/t3-agents` (the formalspec repo), which prints that server's
//! thread state as one JSON line every two seconds. The T3 Code desktop app
//! is one window connected to every server, so each of its windows carries
//! the worst state across all hosts.

use serde_json::Value;

use fs_js as js;

/// The desktop app's Wayland app id (its desktop file is
/// `com.t3tools.T3Code.desktop`).
pub const APP_ID: &str = "com.t3tools.T3Code";

/// Whether a window belongs to the T3 Code desktop app.
pub fn is_app(app_id: &str, initial_class: &str) -> bool {
    app_id == APP_ID || initial_class == APP_ID
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Badge {
    None,
    Done,
    Working,
    Blocked,
}

impl Badge {
    pub fn as_str(self) -> &'static str {
        match self {
            Badge::None => "",
            Badge::Done => "done",
            Badge::Working => "working",
            Badge::Blocked => "blocked",
        }
    }

    pub fn parse(s: &str) -> Badge {
        match s {
            "blocked" => Badge::Blocked,
            "working" => Badge::Working,
            "done" => Badge::Done,
            _ => Badge::None,
        }
    }
}

/// blocked > working > done > nothing: the one badge a set of threads rolls
/// up to, worst status wins.
pub fn aggregate(threads: &[Value]) -> Badge {
    threads
        .iter()
        .map(|t| Badge::parse(t.get("status").and_then(Value::as_str).unwrap_or("")))
        .max()
        .unwrap_or(Badge::None)
}

/// One `t3-agents` line (`{"environment": ..., "threads": [{status, ...}]}`)
/// to its badge, or `None` on anything that isn't that shape: bad JSON, the
/// script's own `null` for an unreadable database, a reply with no
/// `threads` array.
pub fn parse_line(line: Option<&str>) -> Option<Badge> {
    let line = line?;
    if js::trim(line).is_empty() {
        return None;
    }
    let parsed: Value = serde_json::from_str(line).ok()?;
    Some(aggregate(parsed.get("threads")?.as_array()?))
}

/// The argv for one host's poll. `t3-agents` loops by itself, so this is one
/// long-lived ssh per host. BatchMode makes a prompt fail instead of hanging,
/// ServerAlive* notices a dead link rather than leaving it half-open, and
/// `-n` hands the remote an EOF: the service holds the child's stdin open for
/// its whole life, and a remote ~/.bashrc that starts an interactive shell
/// would otherwise sit reading it. The path is relative to the remote home,
/// which sshd starts in, so no login shell has to expand `~`.
pub fn poll_command(host: &str) -> Vec<String> {
    [
        "ssh",
        "-n",
        "-o",
        "BatchMode=yes",
        "-o",
        "ClearAllForwardings=yes",
        "-o",
        "ConnectTimeout=10",
        "-o",
        "ServerAliveInterval=15",
        "-o",
        "ServerAliveCountMax=2",
        host,
        ".local/bin/t3-agents",
    ]
    .map(str::to_owned)
    .to_vec()
}
