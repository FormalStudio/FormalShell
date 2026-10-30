//! The SHARE route's whole dynamic subtree.

use super::clipboard::{captured_at_label, share_entry_command};
use crate::clipboard::history::{Entry as ClipEntry, EntryKind};
use crate::node::{Entries, Entry, Kind};

/// A LocalSend peer from the last scan.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Peer {
    pub name: String,
}

/// LocalSend's own receive state, shown as a status line.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ReceiveStatus {
    pub enabled: bool,
    pub receiving: bool,
    pub alias: String,
    pub dir: String,
}

fn note_entry(label: &str) -> Entry {
    Entry {
        icon: Some(String::new()),
        kind: Some(Kind::Note),
        dim: Some(true),
        ..Entry::labelled(label)
    }
}

/// The SHARE route's whole dynamic subtree: nothing under "share" is declared
/// in `default-menu.jsonc`, because every bit of it depends on live state a
/// static jsonc node cannot express (whether `localsend-cli` is on PATH, its
/// last scan, the clipboard). Insertion order is declaration order, so
/// "share.receive" is built first to keep it ahead of "share.send"'s peer rows
/// regardless of how many there are.
///
/// `installed` (the LocalSend service's flag) decides which of two shapes this
/// builds. CLI present: "Send" lists the last scan's peers, each a folder
/// carrying a "Clipboard" row (the newest TEXT entry, dispatched in-process)
/// and one row per clipboard-history image; "Receive" is a status line, never
/// an action, since `localsend.receive` is a settings.json key the service
/// only reads. CLI absent, GUI (`localsend_app`) the only thing left: one
/// newest-entry Send action, Receive launches the GUI, and the full history
/// picker lives at "share.history" via the "shareHistory" provider.
pub fn share_peer_entries(
    installed: bool,
    peers: &[Peer],
    clipboard_items: &[ClipEntry],
    receive: Option<&ReceiveStatus>,
) -> Entries {
    if !installed {
        return share_fallback_entries(clipboard_items);
    }

    let mut out = Entries::new();
    out.insert("share.receive".into(), share_receive_status(receive));
    out.insert("share.send".into(), Entry::labelled("Send"));
    if peers.is_empty() {
        out.insert("share.send.empty".into(), note_entry("No devices found"));
        return out;
    }
    let text_entry = clipboard_items.iter().find(|e| e.kind != EntryKind::Image);
    let image_entries: Vec<&ClipEntry> = clipboard_items.iter().filter(|e| e.kind == EntryKind::Image).collect();
    for (i, peer) in peers.iter().enumerate() {
        let peer_id = format!("share.send.{i}");
        out.insert(peer_id.clone(), Entry::labelled(&peer.name));
        if text_entry.is_none() && image_entries.is_empty() {
            out.insert(format!("{peer_id}.empty"), note_entry("Nothing to share"));
            continue;
        }
        if let Some(text) = text_entry {
            out.insert(
                format!("{peer_id}.clipboard"),
                Entry::labelled("Clipboard")
                    .with_icon("")
                    .with_action(format!("@ipc:localsend.send:{i}:{}", text.id)),
            );
        }
        for (j, entry) in image_entries.iter().enumerate() {
            out.insert(
                format!("{peer_id}.image.{j}"),
                Entry {
                    desc: Some(captured_at_label(entry.captured_at)),
                    thumb_source: Some(entry.path.clone().unwrap_or_default()),
                    ..Entry::labelled("Image")
                        .with_icon("")
                        .with_action(format!("@ipc:localsend.send:{i}:{}", entry.id))
                },
            );
        }
    }
    out
}

/// A plain status line, never an activatable row: there is nothing for Enter
/// to flip. `dim` is always false on purpose: the launcher's own live-row
/// filter drops a "note" row outright once it sits alongside an actual
/// actionable row (the dim-note shape means "this level has nothing in it",
/// not "here's a muted status line"), so state is read off `desc` instead of
/// dimming it.
fn share_receive_status(receive: Option<&ReceiveStatus>) -> Entry {
    let default = ReceiveStatus::default();
    let r = receive.unwrap_or(&default);
    let desc = if !r.enabled {
        "Off (set localsend.receive: true)".to_string()
    } else if r.receiving {
        format!("Listening as {}, saving to {}", r.alias, r.dir)
    } else {
        "Starting\u{2026}".to_string()
    };
    Entry {
        icon: Some(String::new()),
        kind: Some(Kind::Note),
        dim: Some(false),
        desc: Some(desc),
        ..Entry::labelled("Receive")
    }
}

/// The GUI-only shape. An empty history is the one shape `infer_kind` cannot
/// produce on its own (no action, target or provider to key off), which is
/// what the explicit `kind` and `dim` override exists for: an honest NOTHING
/// TO SHARE row, the same non-activatable shape as a route's own
/// unavailable-state rows.
fn share_fallback_entries(clipboard_items: &[ClipEntry]) -> Entries {
    let mut out = Entries::new();
    out.insert("share.receive".into(), Entry::labelled("Receive").with_action("localsend_app"));
    out.insert(
        "share.history".into(),
        Entry { provider: Some("shareHistory".to_string()), ..Entry::labelled("Pick From History") },
    );
    let send = match clipboard_items.first() {
        // nf-md-clipboard_text
        Some(newest) => Entry::labelled("Send").with_icon("\u{F014D}").with_action(share_entry_command(newest)),
        None => note_entry("Nothing to share"),
    };
    out.insert("share.send".into(), send);
    out
}
