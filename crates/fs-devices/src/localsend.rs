//! Pure model for LocalSend (plan at `docs/superpowers/plans/2026-09-28-m75-iphone.md`).
//! Ported from `shell/Localsend/model.js`.
//!
//! The CLI is 0w0mewo/localsend-cli (Go, rev 7865fb1c), picked over the
//! official Rust `cli/` crate, whose headless mode can only send: no `recv`
//! and no `scan`. `scan -t <seconds>` prints one line per discovered peer to stdout
//! under a "Found Devices:" header, or "No device found" to stderr when
//! nothing answered (scan.go's own Fprintf calls). `send`/`recv` log every
//! event through Go's log/slog default handler; [`parse_slog_line`] reads that
//! line shape, not a fixed field list.

use std::collections::BTreeMap;
use std::sync::LazyLock;

use regex::Regex;

use fs_js::{self as js, DOT, SPACE};

/// "\tName: <alias>, Version: <ver>, Address: <ip>:<port>, Protocol: <proto>"
/// (cmd/scan/scan.go's Fprintf), one line per peer, following the "Found
/// Devices:" header line that the loop in [`parse_scan`] simply does not match.
static SCAN_LINE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(r"^\tName: ({DOT}*), Version: ({DOT}*), Address: ([^:]+):([0-9]+), Protocol: ({DOT}*)$")).unwrap()
});

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Peer {
    pub name: String,
    pub version: String,
    pub ip: String,
    pub port: u32,
    pub protocol: String,
}

pub fn parse_scan(stdout: &str) -> Vec<Peer> {
    stdout
        .split('\n')
        .filter_map(|line| {
            let m = SCAN_LINE.captures(line)?;
            Some(Peer {
                name: m[1].into(),
                version: m[2].into(),
                ip: m[3].into(),
                port: m[4].parse().unwrap_or(u32::MAX),
                protocol: m[5].into(),
            })
        })
        .collect()
}

/// Exact-name match against the last scan's results, `None` when the device
/// has gone quiet since (out of range, or the scan simply predates it).
pub fn resolve_peer<'a>(peers: &'a [Peer], name: &str) -> Option<&'a Peer> {
    peers.iter().find(|p| p.name == name)
}

static KV: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(r#"([A-Za-z_][A-Za-z0-9_.]*)=("(?:[^"\\]|\\{DOT})*"|[^{SPACE}]+)"#)).unwrap()
});

// Neither subcommand installs a slog handler, so lines come out of the default
// one, which writes through log/log.go: "<date> <time> <LEVEL> <msg>
// key=value ...". The message is unquoted; values are quoted only when they
// contain a space.
static SLOG: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(r"^[0-9]{{4}}/[0-9]{{2}}/[0-9]{{2}} [0-9]{{2}}:[0-9]{{2}}:[0-9]{{2}} (DEBUG|INFO|WARN|ERROR)(?:[+-][0-9]+)? ({DOT}*)$")).unwrap()
});

static KV_START: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(^| )[A-Za-z_][A-Za-z0-9_.]*=").unwrap());

static ESCAPE: LazyLock<Regex> = LazyLock::new(|| Regex::new(&format!(r"\\({DOT})")).unwrap());

fn unquote(value: &str) -> String {
    if value.len() >= 2 && value.starts_with('"') && value.ends_with('"') {
        return ESCAPE.replace_all(&value[1..value.len() - 1], "$1").into_owned();
    }
    value.to_string()
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SlogLine {
    pub level: String,
    pub msg: String,
    /// Every key=value pair after the message, so a caller after
    /// `file`/`error`/`remote`/`session` reads it off there.
    pub fields: BTreeMap<String, String>,
}

impl SlogLine {
    fn field(&self, key: &str) -> &str {
        self.fields.get(key).map_or("", String::as_str)
    }
}

/// One send/recv stderr line, or `None` for a line that is not a slog record
/// at all (a stray warning from a dependency, a blank line).
pub fn parse_slog_line(line: &str) -> Option<SlogLine> {
    let m = SLOG.captures(line)?;
    let rest = &m[2];
    let kv_start = KV_START.find(rest).map(|f| f.start());
    let msg = kv_start.map_or(rest, |at| &rest[..at]);
    let mut fields = BTreeMap::new();
    if let Some(at) = kv_start {
        for kv in KV.captures_iter(&rest[at..]) {
            fields.insert(kv[1].to_string(), unquote(&kv[2]));
        }
    }
    Some(SlogLine { level: m[1].into(), msg: js::trim(msg).into(), fields })
}

/// A per-file failure read off an ERROR line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Failure {
    pub msg: String,
    pub file: String,
    pub error: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SendOutcome {
    pub ok: bool,
    pub fatal: bool,
    pub failed: Vec<Failure>,
}

/// `send`'s own exit code is 0 even when a file failed (only
/// `sender.Start()`'s top-level error is fatal, everything inside the per-file
/// loop is a slog.Error that the loop swallows and moves on from,
/// cmd/send/send.go), so the real outcome is read off stderr's ERROR lines,
/// not the exit code alone. `fatal` is the one case the exit code still means
/// something: the top-level error, which never even reaches the per-file loop.
pub fn send_outcome(exit_code: i32, stderr_text: &str) -> SendOutcome {
    let failed: Vec<Failure> = stderr_text
        .split('\n')
        .filter_map(parse_slog_line)
        .filter(|e| e.level == "ERROR")
        .map(|e| {
            let file = match e.field("file") {
                "" => e.field("dir"),
                f => f,
            };
            Failure { msg: e.msg.clone(), file: file.into(), error: e.field("error").into() }
        })
        .collect();
    SendOutcome { ok: exit_code == 0 && failed.is_empty(), fatal: exit_code != 0, failed }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RecvEvent {
    Error { message: String },
    Accepting { remote: String, session: String },
    Received { file: String, session: String },
    Other,
}

/// `recv` logs "Accepting file" when a session opens, and "Recv file" from
/// session/recv.go's SaveFile once the bytes are written and the checksum
/// verified. The file is `<dir>/<file>`, overwriting on a name collision.
pub fn parse_recv_line(line: &str) -> Option<RecvEvent> {
    let e = parse_slog_line(line)?;
    if e.level == "ERROR" {
        let error = e.field("error");
        let suffix = if error.is_empty() { String::new() } else { format!(": {error}") };
        return Some(RecvEvent::Error { message: format!("{}{suffix}", e.msg) });
    }
    if e.msg == "Accepting file" {
        return Some(RecvEvent::Accepting { remote: e.field("remote").into(), session: e.field("session").into() });
    }
    if e.msg == "Recv file" && !e.field("file").is_empty() {
        return Some(RecvEvent::Received { file: e.field("file").into(), session: e.field("session").into() });
    }
    Some(RecvEvent::Other)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Real shapes off 0w0mewo/localsend-cli @ 7865fb1c.
    const SCAN_FOUND: &str = "Found Devices: \n\tName: Kyan's iPhone, Version: 2.0, Address: 192.168.1.42:53317, Protocol: https\n\tName: Cool Mango, Version: 2.0, Address: 192.168.1.50:53317, Protocol: https\n";

    #[test]
    fn parse_scan_finds_every_peer() {
        let peers = parse_scan(SCAN_FOUND);
        assert_eq!(peers.len(), 2);
        assert_eq!(peers[0].name, "Kyan's iPhone");
        assert_eq!(peers[0].ip, "192.168.1.42");
        assert_eq!(peers[0].port, 53317);
        assert_eq!(peers[0].protocol, "https");
        assert_eq!(peers[1].name, "Cool Mango");
        assert_eq!(peers[1].ip, "192.168.1.50");
    }

    // "No device found" lands on stderr, never matches the tab-led peer line,
    // whichever stream it is handed.
    #[test]
    fn parse_scan_empty_on_no_devices() {
        assert_eq!(parse_scan("No device found\n").len(), 0);
        assert_eq!(parse_scan("").len(), 0);
    }

    #[test]
    fn resolve_peer_is_an_exact_name_match() {
        let peers = parse_scan(SCAN_FOUND);
        assert_eq!(resolve_peer(&peers, "Cool Mango").unwrap().ip, "192.168.1.50");
        assert_eq!(resolve_peer(&peers, "Nobody Here"), None);
    }

    #[test]
    fn parse_slog_line_plain_values() {
        let e = parse_slog_line("2026/09/29 13:35:10 INFO Done").unwrap();
        assert_eq!(e.level, "INFO");
        assert_eq!(e.msg, "Done");
    }

    #[test]
    fn parse_slog_line_quoted_values_with_spaces() {
        let e = parse_slog_line(r#"2026/09/29 13:35:10 INFO Start sending file="/tmp/my a.png""#).unwrap();
        assert_eq!(e.level, "INFO");
        assert_eq!(e.msg, "Start sending");
        assert_eq!(e.fields["file"], "/tmp/my a.png");
    }

    #[test]
    fn parse_slog_line_error_with_nested_error_field() {
        let e = parse_slog_line(r#"2026/09/29 13:35:10 ERROR Fail to send error="dial tcp: connection refused""#).unwrap();
        assert_eq!(e.level, "ERROR");
        assert_eq!(e.fields["error"], "dial tcp: connection refused");
    }

    #[test]
    fn parse_slog_line_no_level_is_none() {
        assert_eq!(parse_slog_line("Waitting for receiving files (Ctrl-C to terminate)"), None);
        assert_eq!(parse_slog_line(""), None);
    }

    #[test]
    fn send_outcome_exit0_no_errors_is_ok() {
        let out = send_outcome(0, "2026/09/29 13:35:10 INFO Start sending file=/tmp/a.png\n2026/09/29 13:35:10 INFO Done\n");
        assert!(out.ok);
        assert!(!out.fatal);
        assert_eq!(out.failed.len(), 0);
    }

    // The exact case the plan calls out: exit 0 with a per-file failure buried
    // in an ERROR line, never surfaced by the exit code alone.
    #[test]
    fn send_outcome_exit0_with_per_file_error_is_not_ok() {
        let stderr = "2026/09/29 13:35:10 INFO Start sending file=/tmp/a.png\n\
            2026/09/29 13:35:10 ERROR Fail to add file, skipping... file=/tmp/b.png error=\"no such file or directory\"\n\
            2026/09/29 13:35:10 INFO Done\n";
        let out = send_outcome(0, stderr);
        assert!(!out.ok);
        assert!(!out.fatal);
        assert_eq!(out.failed.len(), 1);
        assert_eq!(out.failed[0].file, "/tmp/b.png");
        assert_eq!(out.failed[0].error, "no such file or directory");
    }

    #[test]
    fn send_outcome_nonzero_exit_is_fatal() {
        let out = send_outcome(1, "2026/09/29 13:35:10 ERROR Fail to send error=\"IP address is required\"\n");
        assert!(!out.ok);
        assert!(out.fatal);
        assert_eq!(out.failed.len(), 1);
    }

    #[test]
    fn parse_recv_line_accepting() {
        let e = parse_recv_line("2026/09/29 13:35:10 INFO Accepting file remote=192.168.1.42 session=abc-123");
        assert_eq!(e, Some(RecvEvent::Accepting { remote: "192.168.1.42".into(), session: "abc-123".into() }));
    }

    #[test]
    fn parse_recv_line_error() {
        let e = parse_recv_line(r#"2026/09/29 13:35:10 ERROR Upload error remote=192.168.1.42 session=abc-123 error="checksum mismatch""#);
        assert_eq!(e, Some(RecvEvent::Error { message: "Upload error: checksum mismatch".into() }));
    }

    #[test]
    fn parse_recv_line_no_level_is_none() {
        assert_eq!(parse_recv_line("Waitting for receiving files (Ctrl-C to terminate)"), None);
    }

    #[test]
    fn parse_recv_line_received() {
        let e = parse_recv_line(r#"2026/09/29 13:35:10 INFO Recv file file="my photo.png" session=abc-123"#);
        assert_eq!(e, Some(RecvEvent::Received { file: "my photo.png".into(), session: "abc-123".into() }));
    }

    #[test]
    fn parse_recv_line_other_for_any_other_record() {
        assert_eq!(parse_recv_line("2026/09/29 13:35:10 INFO Done"), Some(RecvEvent::Other));
        assert_eq!(parse_recv_line("2026/09/29 13:35:10 INFO Recv file session=abc"), Some(RecvEvent::Other));
    }
}
