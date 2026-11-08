//! Model for the "flake inputs behind upstream" widget and panel: parses a
//! flake.lock, builds the cheapest correct upstream probe per input type,
//! parses each probe's output, and folds the result into one bar label. No
//! IO and no clock.
//!
//! WHAT THIS ANSWERS, precisely: "are my flake inputs behind their upstream
//! refs". That is NOT "does my running system differ from what a rebuild
//! would produce". Do not let the label drift toward the latter.
//!
//! NETWORK COST drives the whole design: stage 1 is free (read flake.lock off
//! disk, no nix invocation) and stage 2 costs one network round trip PER
//! DIRECT INPUT. That is why the panel's poll cadence is hours, not minutes,
//! and why the unauthenticated GitHub rate limit (60 requests/hour/IP)
//! matters: a 403 must land in `unknown`, never in `current`.
//!
//! Rejected alternatives, so nobody re-litigates them:
//! - `nix flake metadata --json <dir>` returns only the LOCKED revs, which
//!   flake.lock already holds verbatim, and copies the flake directory into
//!   the store on every invocation. Strictly more expensive than reading the
//!   file, and it says nothing about upstream.
//! - `nix flake update --output-lock-file <tmp>` is the only pure-nix way to
//!   learn upstream revs, but computing a narHash requires fully fetching
//!   every input source: a ~40MB nixpkgs tarball per poll whenever upstream
//!   moved.
//! - `git ls-remote` against nixpkgs was measured at over 120 seconds during
//!   design recon (twice, including protocol v2 with an explicit refspec)
//!   because nixpkgs advertises ~100k refs. That is why github inputs go
//!   through the API and only type:"git" forges use ls-remote. Unifying the
//!   two probe paths onto ls-remote would wedge the poll.

use std::cmp::Ordering;
use std::collections::HashMap;

use serde_json::Value;

use fs_js as js;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct FlakeInput {
    pub name: String,
    pub kind: String,
    pub rev: String,
    pub r#ref: String,
    pub owner: String,
    pub repo: String,
    pub url: String,
    pub last_modified: f64,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Lock {
    pub ok: bool,
    pub inputs: Vec<FlakeInput>,
}

fn is_sha(s: &str) -> bool {
    let hex = |n: usize| s.len() == n && s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'));
    hex(40) || hex(64)
}

fn str_field(node: Option<&Value>, key: &str) -> String {
    match node.and_then(|n| n.get(key)) {
        Some(Value::String(s)) => s.clone(),
        _ => String::new(),
    }
}

/// Name order for the panel's ledger. The names are flake input identifiers,
/// so this follows the locale collation they get from a UI sort: case
/// differences are secondary (lowercase first), punctuation sorts before
/// digits and digits before letters.
fn collate(a: &str, b: &str) -> Ordering {
    fn primary(c: char) -> (u8, u32) {
        const PUNCT: &str = "_-,;:!?.'\"()[]{}@*/\\&#%`^+<=>|~$";
        if let Some(i) = PUNCT.find(c) {
            (0, i as u32)
        } else if c.is_ascii_digit() {
            (1, u32::from(c))
        } else if c.is_alphabetic() {
            (2, c.to_lowercase().next().map_or(u32::from(c), u32::from))
        } else {
            (3, u32::from(c))
        }
    }
    let by_primary = a.chars().map(primary).cmp(b.chars().map(primary));
    by_primary.then_with(|| {
        a.chars()
            .zip(b.chars())
            .map(|(x, y)| y.is_lowercase().cmp(&x.is_lowercase()))
            .find(|o| o.is_ne())
            .unwrap_or(Ordering::Equal)
    })
}

/// Reads nodes[root].inputs, the DIRECT inputs only. A nixpkgs pinned into
/// some dependency by a `follows` is not something the user updates, so the
/// transitive closure is deliberately not walked.
///
/// `ref` lives in `original` for a github input pinned to a branch
/// ("nixos-unstable") and in `locked` for a git input ("refs/heads/master"),
/// verified against this repo's own flake.lock, hence the two-step read. A
/// root input whose value is an array is a follows path pointing at another
/// node the user already sees listed, and is skipped.
///
/// Malformed, absent, or unparsable text returns `ok: false` with no inputs
/// and never fails.
pub fn parse_lock(text: &str) -> Lock {
    let empty = Lock { ok: false, inputs: Vec::new() };
    let raw = js::trim(text);
    if raw.is_empty() {
        return empty;
    }
    let Ok(data) = serde_json::from_str::<Value>(raw) else {
        return empty;
    };
    let Some(nodes) = data.get("nodes").and_then(Value::as_object) else {
        return empty;
    };
    let root_key = data
        .get("root")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .unwrap_or("root");
    let Some(root) = nodes.get(root_key).filter(|n| n.is_object()) else {
        return empty;
    };
    let Some(direct) = root.get("inputs").and_then(Value::as_object) else {
        return Lock { ok: true, inputs: Vec::new() };
    };

    let mut inputs = Vec::new();
    for (name, key) in direct {
        let Some(node) = key.as_str().and_then(|k| nodes.get(k)).filter(|n| n.is_object()) else {
            continue;
        };
        let locked = node.get("locked");
        let original = node.get("original");
        let pick = |key: &str, first: Option<&Value>, second: Option<&Value>| {
            let a = str_field(first, key);
            if a.is_empty() { str_field(second, key) } else { a }
        };
        inputs.push(FlakeInput {
            name: name.clone(),
            kind: pick("type", locked, original),
            rev: str_field(locked, "rev"),
            r#ref: pick("ref", original, locked),
            owner: pick("owner", locked, original),
            repo: pick("repo", locked, original),
            url: pick("url", locked, original),
            last_modified: js::finite_number(locked.and_then(|l| l.get("lastModified")))
                .unwrap_or(0.0),
        });
    }

    // Alphabetical, so the panel's ledger has a stable row order across
    // polls rather than flake.lock's JSON key order.
    inputs.sort_by(|a, b| collate(&a.name, &b.name));
    Lock { ok: true, inputs }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProbeKind {
    None,
    Github,
    Gitlab,
    Git,
}

impl ProbeKind {
    pub fn as_str(self) -> &'static str {
        match self {
            ProbeKind::None => "none",
            ProbeKind::Github => "github",
            ProbeKind::Gitlab => "gitlab",
            ProbeKind::Git => "git",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Probe {
    pub kind: ProbeKind,
    pub argv: Vec<String>,
}

fn strings(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|s| (*s).to_owned()).collect()
}

/// The cheapest correct upstream probe for one input, as an argv the caller
/// hands straight to a process spawn.
///
/// github goes to the commits API with the sha media type, which answers with
/// a bare 40-char sha and is the same endpoint nix's own github fetcher uses
/// to resolve a ref to a rev. Measured at 0.5s for NixOS/nixpkgs.
///
/// git runs `git ls-remote`, measured at 1.0s against git.outfoxxed.me. The
/// `command -v git` guard makes a git-less environment exit 127, which
/// `parse_probe` folds into `unknown` rather than a wrong count. url and ref
/// are passed as positional arguments to sh rather than interpolated into the
/// script, so nothing from the lock file is ever parsed as shell.
///
/// gitlab uses the v4 commits API. UNVERIFIED against a live GitLab; a wrong
/// guess produces an empty rev, which counts as unknown.
///
/// Everything else (path, tarball, indirect, sourcehut) is kind `None` and
/// counts as unknown. Never as current, never as behind.
pub fn probe_command(input: &FlakeInput) -> Probe {
    let none = Probe { kind: ProbeKind::None, argv: Vec::new() };
    let reference = if input.r#ref.is_empty() { "HEAD" } else { input.r#ref.as_str() };

    match input.kind.as_str() {
        "github" => {
            if input.owner.is_empty() || input.repo.is_empty() {
                return none;
            }
            let url = format!(
                "https://api.github.com/repos/{}/{}/commits/{reference}",
                input.owner, input.repo
            );
            let mut argv = strings(&["curl", "-sS", "-f", "-m", "15", "-H", "Accept: application/vnd.github.sha"]);
            argv.push(url);
            Probe { kind: ProbeKind::Github, argv }
        }
        "gitlab" => {
            if input.owner.is_empty() || input.repo.is_empty() {
                return none;
            }
            let url = format!(
                "https://gitlab.com/api/v4/projects/{}/repository/commits/{}",
                js::encode_uri_component(&format!("{}/{}", input.owner, input.repo)),
                js::encode_uri_component(reference)
            );
            let mut argv = strings(&["curl", "-sS", "-f", "-m", "15"]);
            argv.push(url);
            Probe { kind: ProbeKind::Gitlab, argv }
        }
        "git" => {
            if input.url.is_empty() {
                return none;
            }
            let mut argv = strings(&[
                "sh",
                "-c",
                "command -v git >/dev/null 2>&1 || exit 127; exec git ls-remote \"$1\" \"$2\"",
                "systemupdate-probe",
            ]);
            argv.push(input.url.clone());
            argv.push(reference.to_owned());
            Probe { kind: ProbeKind::Git, argv }
        }
        _ => none,
    }
}

/// The upstream rev a probe resolved, or "" for every failure: non-zero exit,
/// empty output, unparsable output, or anything that is not a commit hash. A
/// github 403 (rate limited) exits non-zero under `curl -f` and lands here as
/// "", which `count_behind` then reports as unknown.
pub fn parse_probe(kind: ProbeKind, exit_code: i32, stdout: &str) -> String {
    if exit_code != 0 {
        return String::new();
    }
    let out = js::trim(stdout);
    if out.is_empty() {
        return String::new();
    }
    let rev = match kind {
        ProbeKind::Github => js::split_spaces(out).next().unwrap_or("").to_owned(),
        // "<sha>\t<ref>", one line per matching ref.
        ProbeKind::Git => js::trim(out.split('\n').next().unwrap_or("").split('\t').next().unwrap_or(""))
            .to_owned(),
        ProbeKind::Gitlab => match serde_json::from_str::<Value>(out) {
            Ok(data) => str_field(Some(&data), "id"),
            Err(_) => String::new(),
        },
        ProbeKind::None => String::new(),
    };
    if is_sha(&rev) { rev } else { String::new() }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Counts {
    pub behind: u32,
    pub current: u32,
    pub unknown: u32,
    pub behind_names: Vec<String>,
}

/// `heads` maps input name to the upstream rev `parse_probe` resolved. An
/// input whose head did not resolve to a non-empty rev is `unknown`, and so is
/// an input with no locked rev of its own. Never silently current.
pub fn count_behind(inputs: &[FlakeInput], heads: &HashMap<String, String>) -> Counts {
    let mut counts = Counts::default();
    for input in inputs {
        let head = heads.get(&input.name).map_or("", String::as_str);
        if input.rev.is_empty() || head.is_empty() {
            counts.unknown += 1;
        } else if input.rev == head {
            counts.current += 1;
        } else {
            counts.behind += 1;
            counts.behind_names.push(input.name.clone());
        }
    }
    counts
}

/// Per-row ledger status for the panel, routed through `count_behind` so a row
/// can never disagree with the header count.
pub fn row_status(input: &FlakeInput, heads: &HashMap<String, String>) -> &'static str {
    let counts = count_behind(std::slice::from_ref(input), heads);
    if counts.behind > 0 {
        "Behind"
    } else if counts.current > 0 {
        "Current"
    } else {
        "?"
    }
}

/// The dim locked rev the panel prints beside each input name.
pub fn short_rev(rev: &str) -> String {
    rev.chars().take(7).collect()
}

/// The poll's own stage.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PollState {
    /// No `systemUpdate.flakeDir` configured.
    NoFlake,
    /// The directory has no readable flake.lock.
    NoLock,
    /// Probes in flight.
    Checking,
    /// Every probe failed to reach its forge.
    Offline,
    /// Probes finished.
    Ok,
}

impl PollState {
    /// Anything unrecognized is treated as still resolving, never as up to
    /// date.
    pub fn parse(state: &str) -> PollState {
        match state {
            "noflake" => PollState::NoFlake,
            "nolock" => PollState::NoLock,
            "offline" => PollState::Offline,
            "ok" => PollState::Ok,
            _ => PollState::Checking,
        }
    }
}

/// The bar cell's whole text, so every honest state is one tested string
/// instead of widget branching.
///
/// "Up to date" is only ever returned when nothing is behind AND nothing is
/// unknown, and no zero count is ever printed.
pub fn summary_label(state: PollState, counts: &Counts) -> String {
    match state {
        PollState::NoFlake => return "No flake".into(),
        PollState::NoLock => return "No lock".into(),
        PollState::Checking => return "Checking".into(),
        PollState::Offline => return "No network".into(),
        PollState::Ok => {}
    }
    match (counts.behind, counts.unknown) {
        (0, 0) => "Up to date".into(),
        (0, unknown) => format!("{unknown} ?"),
        (behind, 0) => format!("{behind} behind"),
        (behind, unknown) => format!("{behind} behind / {unknown} ?"),
    }
}
