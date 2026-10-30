//! Clipboard history rows, the route-local filter and the paste chord.

use chrono::{DateTime, Local, Timelike};

use crate::clipboard::emoji::is_emoji_only;
use crate::clipboard::history::{Entry, EntryKind};
use crate::node::{ActionEntry, Kind, Node};

/// Local `HH:MM` of a capture time. A legacy entry with no timestamp reads
/// "NaN:NaN", which is what an invalid date formats to.
pub fn captured_at_label(captured_at: Option<i64>) -> String {
    let local = captured_at
        .and_then(DateTime::from_timestamp_millis)
        .map(|utc| utc.with_timezone(&Local));
    match local {
        Some(d) => format!("{:02}:{:02}", d.hour(), d.minute()),
        None => "NaN:NaN".to_string(),
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ClipMode {
    #[default]
    Copy,
    Share,
}

/// Rows for the history entries, in the order given (newest first).
///
/// These are plain "action" nodes, so the launcher's existing activation path
/// handles them with no bespoke node kind. Copy-mode rows dispatch in-process
/// (`@ipc:clipboard.copy:<id>`, the same argument-carrying internal-action
/// shape `clipssh.send:<alias>` uses) rather than spawning an IPC command:
/// nothing puts that binary on a session PATH, so the spawned form was a
/// silent exit 127 everywhere but the VM test rig.
///
/// `paste` is `clipboard.paste`, threaded in so this stays pure. It marks a
/// copy row for the paste hook, which synthesizes the chord into whatever
/// window focus returns to; a share row hands the entry to LocalSend and
/// never touches the focused window.
///
/// `ClipMode::Share` only changes the id prefix, verb and activation.
/// Both providers read the same history list and `tree.nodes` is one flat map
/// keyed by id, so reusing `clipboard.<id>` for share rows would overwrite the
/// real clipboard node's rows.
pub fn clipboard_provider(items: &[Entry], mode: ClipMode, paste: bool) -> Vec<Node> {
    let share = mode == ClipMode::Share;
    let paste_after = !share && paste;
    let id_prefix = if share { "share.history." } else { CLIPBOARD_ENTRY_PREFIX };
    items
        .iter()
        .map(|entry| {
            let is_image = entry.kind == EntryKind::Image;
            let text = entry.text.as_deref().unwrap_or("");
            let emoji_only = !is_image && is_emoji_only(text);
            let path = entry.path.clone().unwrap_or_default();
            let time = captured_at_label(entry.captured_at);
            let label = if is_image {
                "Image".to_string()
            } else if emoji_only {
                fs_js::collapse_spaces(text)
            } else {
                preview_label(text, None)
            };
            Node {
                desc: Some(if is_image { time.clone() } else { String::new() }),
                thumb_source: if is_image { path.clone() } else { String::new() },
                // Shift+Enter's target: the image file this row stands for,
                // "" on text rows and on share rows, whose Shift+Enter has
                // nothing of its own to do.
                alternate: (is_image && !share).then(|| format!("@ipc:clipssh.image:{}:{path}", entry.id)),
                alternate_label: (is_image && !share).then(|| "Send over SSH".to_string()),
                clipssh_path: if is_image && !share { path } else { String::new() },
                // Full untruncated text for the split-pane preview, "" for
                // images. `time` rides every row, since the preview pane's
                // meta line needs a capture time regardless of kind.
                full_text: if is_image { String::new() } else { text.to_string() },
                emoji_only,
                time,
                // The action bar's verb, overriding what the kind alone
                // implies: "Run" describes a clipboard row about as well as
                // it describes an app row.
                verb: Some(if share { "Share" } else if paste_after { "Paste" } else { "Copy" }.to_string()),
                action: Some(if share {
                    share_entry_command(entry)
                } else {
                    format!("@ipc:clipboard.copy:{}", entry.id)
                }),
                paste_after,
                actions: if share {
                    Vec::new()
                } else {
                    vec![ActionEntry {
                        keep_open: Some(true),
                        ..ActionEntry::new("delete", "Delete from history", format!("@ipc:clipboard.remove:{}", entry.id)).destructive()
                    }]
                },
                ..Node::new(format!("{id_prefix}{}", entry.id), label, Kind::Action)
            }
        })
        .collect()
}

const CLIPBOARD_ENTRY_PREFIX: &str = "clipboard.";

/// The history entry id a Clipboard route row stands for.
pub fn clipboard_entry_id(node: &Node) -> Option<&str> {
    node.id.strip_prefix(CLIPBOARD_ENTRY_PREFIX).filter(|_| node.kind == Kind::Action)
}

/// wtype's modifier vocabulary. `wtype -M <name>` was run against each
/// candidate: `super`, `meta`, `control` and `command` all come back "Invalid
/// modifier name", and `logo` is the one that means the windows/command key.
const PASTE_MODIFIERS: &[&str] = &["shift", "capslock", "ctrl", "logo", "win", "alt", "altgr"];

/// wtype argv for a paste chord ("ctrl+v", "ctrl+shift+v", ...): every
/// modifier pressed with `-M` in written order, the key tapped with `-k`,
/// then the modifiers released with `-m` in reverse.
///
/// A name outside the vocabulary is a config typo, and the honest answer is to
/// report it rather than synthesize a keystroke nobody asked for, so a bad
/// chord returns `None`. The chord is configuration and not row data because
/// the right one depends on where focus lands: Ctrl+V everywhere except
/// terminals, which almost universally want Ctrl+Shift+V.
pub fn paste_argv(chord: &str) -> Option<Vec<String>> {
    let lowered = chord.to_lowercase();
    let mut parts: Vec<&str> = lowered.split('+').map(fs_js::trim).filter(|p| !p.is_empty()).collect();
    let key = parts.pop()?;
    let mods = parts;
    if mods.iter().any(|m| !PASTE_MODIFIERS.contains(m)) || PASTE_MODIFIERS.contains(&key) {
        return None;
    }
    let mut argv: Vec<String> = Vec::new();
    for m in &mods {
        argv.extend(["-M".to_string(), (*m).to_string()]);
    }
    argv.extend(["-k".to_string(), key.to_string()]);
    for m in mods.iter().rev() {
        argv.extend(["-m".to_string(), (*m).to_string()]);
    }
    Some(argv)
}

/// Route-local filter for the clipboard and share-history level. Unlike the
/// whole-tree ranking this tests one field per row, `full_text`, falling back
/// to `label` for image rows (whose `full_text` is always ""), so typing here
/// narrows history instead of turning into a global search the moment a query
/// is non-empty. Case-insensitive substring, not fuzzy: the ask is "does this
/// entry contain what I typed".
pub fn clipboard_search<'a>(rows: &'a [Node], query: &str) -> Vec<&'a Node> {
    let q = fs_js::trim(query).to_lowercase();
    if q.is_empty() {
        return rows.iter().collect();
    }
    rows.iter()
        .filter(|row| {
            let hay = if row.full_text.is_empty() { &row.label } else { &row.full_text };
            hay.to_lowercase().contains(&q)
        })
        .collect()
}

/// An actually-empty history reads differently from a query that matched
/// nothing, so they carry distinct labels.
pub fn clipboard_empty_row() -> Node {
    Node::note("clipboard.empty", "Clipboard history is empty")
}

pub fn clipboard_no_match_row() -> Node {
    Node::note("clipboard.nomatch", "No matches")
}

/// Single-quotes `value` for a `sh -c` string, escaping embedded single quotes
/// as `'\''` (close the quote, an escaped literal quote, reopen). Clipboard
/// text can contain anything a shell would otherwise interpret, so this is the
/// one place a captured entry's raw content reaches a spawned command.
pub fn shq(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

/// The SHARE route's fallback launch command, used only when `localsend-cli`
/// is not on PATH and the GUI is the one thing left. The binary is
/// `localsend_app` (nixpkgs' `pkgs.localsend`), and upstream's argument
/// handling is narrower than it looks: it skips every argument starting with
/// "-" outright, then keeps only the ones that name an existing file or
/// directory. So `-t`/`--text` are dropped before any check runs, and text,
/// like an image, has to become a real file: written to a `mktemp` `.txt`
/// file and shared by path. Either way this only launches the picker;
/// LocalSend's own GUI still owns starting the transfer to a chosen device.
pub fn share_entry_command(entry: &Entry) -> String {
    if entry.kind == EntryKind::Image {
        return format!("localsend_app {}", shq(entry.path.as_deref().unwrap_or("")));
    }
    format!(
        "tmp=$(mktemp --suffix=.txt) && printf '%s' {} > \"$tmp\" && exec localsend_app \"$tmp\"",
        shq(entry.text.as_deref().unwrap_or(""))
    )
}

/// First non-blank line only, capped at `max_len` UTF-16 units (60 by
/// default). Clipboard captures can be multi-line and arbitrarily long, and a
/// row's label is a single non-wrapping line, so anything longer needs
/// pre-truncating here rather than spilling into the row below it.
pub fn preview_label(text: &str, max_len: Option<usize>) -> String {
    let max_len = max_len.filter(|n| *n > 0).unwrap_or(60);
    let lines: Vec<&str> = text.split('\n').collect();
    let first_line = lines
        .iter()
        .map(|l| fs_js::trim(l))
        .find(|l| !l.is_empty())
        .unwrap_or_else(|| fs_js::trim(text));
    let truncated = fs_js::utf16_len(first_line) > max_len;
    let out = if truncated { fs_js::utf16_prefix(first_line, max_len) } else { first_line.to_string() };
    if truncated || lines.len() > 1 { format!("{out}\u{2026}") } else { out }
}
