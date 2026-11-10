//! The clipssh route: `~/.clipssh/aliases` parsing, clipssh's own output
//! contract, and the alias rows.

use std::sync::LazyLock;

use regex::Regex;

use crate::actions::IpcCall;
use crate::node::{Kind, Node};

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ClipsshAlias {
    pub name: String,
    pub target: String,
}

/// `name=user@host` lines (clipssh's own alias store; its `alias_add` rejects
/// `=` and whitespace in names). Malformed or blank lines are skipped: the file
/// is clipssh's own state, not input this shell owns validating. Targets keep
/// everything after the FIRST `=`.
pub fn clipssh_aliases(text: &str) -> Vec<ClipsshAlias> {
    let mut out = Vec::new();
    for line in text.split('\n') {
        let trimmed = fs_js::trim(line);
        if trimmed.is_empty() {
            continue;
        }
        let Some(eq) = trimmed.find('=') else { continue };
        if eq == 0 || eq == trimmed.len() - 1 {
            continue;
        }
        let name = &trimmed[..eq];
        if name.chars().any(fs_js::is_space) {
            continue;
        }
        out.push(ClipsshAlias { name: name.to_string(), target: trimmed[eq + 1..].to_string() });
    }
    out
}

#[derive(Clone, Debug, PartialEq)]
pub enum ClipsshOutcome {
    /// A completed transfer; `path` is the remote path, "" when unreadable.
    Ok { path: String },
    Failed { error: String },
}

static ANSI_COLOR: LazyLock<Regex> = LazyLock::new(|| Regex::new("\u{1b}\\[[0-9;]*m").expect("static regex"));
// JavaScript's `.` skips the four line terminators and its multiline `$`
// stops at any of them; CRLF mode gives the regex crate the `\r` half.
static UPLOADED: LazyLock<Regex> =
    LazyLock::new(|| Regex::new("(?mR)^\\s*Uploaded:\\s*([^\n\r\u{2028}\u{2029}]+?)\\s*$").expect("static regex"));
static ERROR_LINE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new("(?mR)^\\s*Error:\\s*([^\n\r\u{2028}\u{2029}]+?)\\s*$").expect("static regex"));

/// clipssh's own output contract (v1.0.0): a completed transfer exits 0 and
/// prints "Uploaded: <remote path>", every refusal exits non-zero and prints
/// "Error: <reason>" on stderr, and both lines are wrapped in ANSI color, so
/// the runs are stripped before anything is read out of them. 127 is the
/// shell's own answer for a clipssh that is not installed, which is worth
/// saying plainly rather than reporting as an empty failure. The service turns
/// this into what the user sees; the only thing decided here is what actually
/// happened.
pub fn clipssh_outcome(exit_code: i32, stdout: &str, stderr: &str) -> ClipsshOutcome {
    let out = ANSI_COLOR.replace_all(stdout, "");
    let err = ANSI_COLOR.replace_all(stderr, "");
    if exit_code == 0 {
        let path = UPLOADED.captures(&out).map(|c| c[1].to_string()).unwrap_or_default();
        return ClipsshOutcome::Ok { path };
    }
    if exit_code == 127 {
        return ClipsshOutcome::Failed { error: "clipssh is not installed".to_string() };
    }
    if let Some(reason) = ERROR_LINE.captures(&err) {
        return ClipsshOutcome::Failed { error: reason[1].to_string() };
    }
    // No line in clipssh's own shape: fall back to whatever it did say, and
    // only then to the bare code, so a failure never reports as nothing.
    let last = err.split('\n').map(fs_js::trim).rfind(|l| !l.is_empty());
    ClipsshOutcome::Failed {
        error: last.map_or_else(|| format!("clipssh exited with code {exit_code}"), str::to_string),
    }
}

/// Alias rows for the clipssh route: Enter hands the alias to the clipssh
/// service (`@ipc:` dispatch, so the shell runs clipssh itself rather than
/// spawning it through the compositor and losing sight of it). The service
/// owns saying so while it is in flight and when it lands, so these rows carry
/// no toast of their own. An empty store renders one dim note row whose desc is
/// the exact add command, not a bare shrug.
pub fn clipssh_rows(aliases: &[ClipsshAlias]) -> Vec<Node> {
    if aliases.is_empty() {
        return vec![Node {
            desc: Some("clipssh alias add <name> <user@host>".to_string()),
            ..Node::note("clipssh.empty", "No aliases")
        }];
    }
    aliases
        .iter()
        .map(|a| Node {
            desc: Some(a.target.clone()),
            action: Some(IpcCall::new("clipssh", "send").arg(&a.name).into()),
            ..Node::new(format!("clipssh.{}", a.name), a.name.clone(), Kind::Action)
        })
        .collect()
}
