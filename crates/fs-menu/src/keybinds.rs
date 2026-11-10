//! The compositor's keybinds as launcher rows.

use fs_types::Bind;

use crate::node::{Kind, Node};

pub const KEYBINDS_CHORD_PAD_MAX: usize = 24;
pub const KEYBINDS_MAX_RESULTS: usize = 200;

/// A Text item's implicit width drops trailing ASCII spaces, so the chord
/// column pads with U+00A0.
const PAD_CHAR: char = '\u{a0}';

pub fn describe_action(bind: &Bind) -> String {
    let mut parts = vec![bind.action.clone()];
    parts.extend(bind.args.iter().cloned());
    let text = fs_js::trim(&parts.join(" ")).to_string();
    if text.is_empty() { bind.title.clone() } else { text }
}

const WORD_BREAK: &str = " +-_./";

fn word_start(hay: &str, q: &str) -> bool {
    hay.match_indices(q).any(|(at, _)| at == 0 || hay[..at].chars().next_back().is_some_and(|c| WORD_BREAK.contains(c)))
}

/// Exact chord > chord or action prefix > word start > contains, ties in
/// config order. Never the whole-tree rank: a hundred-odd binds there would
/// drown every root search.
pub fn search<'a>(binds: &'a [Bind], query: &str) -> Vec<&'a Bind> {
    let q = fs_js::trim(query).to_lowercase();
    if q.is_empty() {
        return binds.iter().take(KEYBINDS_MAX_RESULTS).collect();
    }
    let mut results: Vec<(u8, usize, &Bind)> = Vec::new();
    for (i, bind) in binds.iter().enumerate() {
        let chord = bind.chord.to_lowercase();
        let action = describe_action(bind).to_lowercase();
        let hay = format!("{chord} {action} {}", bind.title.to_lowercase());
        let tier = if chord == q {
            4
        } else if chord.starts_with(&q) || action.starts_with(&q) {
            3
        } else if word_start(&hay, &q) {
            2
        } else if hay.contains(&q) {
            1
        } else {
            continue;
        };
        results.push((tier, i, bind));
    }
    results.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    results.into_iter().take(KEYBINDS_MAX_RESULTS).map(|r| r.2).collect()
}

/// Inert "note" rows: a bind acts on the focused window, and at Enter that is
/// the launcher. No `dim`, which belongs to the unavailable rows. The id
/// carries the index because duplicate chords are legal.
pub fn rows(binds: &[Bind], query: &str) -> Vec<Node> {
    let matched = search(binds, query);
    let width = matched.iter().map(|b| fs_js::utf16_len(&b.chord)).max().unwrap_or(0).min(KEYBINDS_CHORD_PAD_MAX);
    matched
        .iter()
        .enumerate()
        .map(|(i, bind)| {
            let n = fs_js::utf16_len(&bind.chord);
            let mut label = bind.chord.clone();
            label.extend(std::iter::repeat_n(PAD_CHAR, width.saturating_sub(n)));
            Node {
                desc: Some(describe_action(bind)),
                actions: vec![crate::node::ActionEntry::new("copy-chord", "Copy chord", crate::actions::copy_run(&bind.chord))],
                ..Node::new(format!("keybinds.{i}.{}", bind.chord), label, Kind::Note)
            }
        })
        .collect()
}

fn note_row(id: &str, label: &str, desc: &str) -> Node {
    Node { desc: Some(desc.into()), ..Node::note(id, label) }
}

pub fn no_binds_row() -> Node {
    note_row("keybinds.nobinds", "No binds", "no binds in the compositor config")
}

pub fn failed_row() -> Node {
    note_row("keybinds.failed", "Binds unavailable", "the compositor's binds could not be read")
}

/// Rows for the keybinds route: `None` before the compositor has
/// answered, `Some(Err(()))` when it failed, else its binds.
pub fn rows_for(reply: Option<Result<&[Bind], ()>>, query: &str) -> Vec<Node> {
    match reply {
        None => Vec::new(),
        Some(Err(())) => vec![failed_row()],
        Some(Ok([])) => vec![no_binds_row()],
        Some(Ok(binds)) => rows(binds, query),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bind(mods: &[&'static str], key: &str, action: &str, arg: &str, title: &str) -> Bind {
        let mut chord = mods.to_vec();
        chord.push(key);
        Bind {
            chord: chord.join("+"),
            mods: mods.to_vec(),
            key: key.into(),
            action: action.into(),
            args: if arg.is_empty() { Vec::new() } else { vec![arg.into()] },
            title: title.into(),
            ..Bind::default()
        }
    }

    fn table() -> Vec<Bind> {
        vec![
            bind(&["SUPER", "SHIFT"], "slash", "exec", "formalshell-ipc call menu toggle", ""),
            bind(&["SUPER"], "code:36", "", "", "Open: the menu"),
            bind(&[], "XF86AudioRaiseVolume", "exec", "wpctl set-volume @DEFAULT_AUDIO_SINK@ 5%+", ""),
        ]
    }

    #[test]
    fn describes_the_action_or_the_title() {
        let binds = table();
        assert_eq!(describe_action(&binds[0]), "exec formalshell-ipc call menu toggle");
        assert_eq!(describe_action(&binds[1]), "Open: the menu");
    }

    #[test]
    fn ranks_and_pads() {
        let binds = table();
        let hit: Vec<&str> = search(&binds, "volume").iter().map(|b| b.key.as_str()).collect();
        assert_eq!(hit, ["XF86AudioRaiseVolume"]);
        assert_eq!(search(&binds, "exec").len(), 2);
        assert_eq!(search(&binds, "super+shift+slash")[0].key, "slash");
        assert_eq!(search(&binds, "menu")[0].key, "slash");
        let r = rows(&binds, "");
        assert_eq!(r.len(), 3);
        assert_eq!(r[0].id, "keybinds.0.SUPER+SHIFT+slash");
        assert_eq!(r[0].label.chars().count(), 20);
        assert!(r[0].label.ends_with('\u{a0}'));
        assert_eq!(r[0].kind, Kind::Note);
        assert_eq!(r[0].dim, None);
    }

    #[test]
    fn end_states() {
        assert!(rows_for(None, "").is_empty());
        assert_eq!(rows_for(Some(Err(())), "")[0].id, "keybinds.failed");
        assert_eq!(rows_for(Some(Ok(&[])), "")[0].id, "keybinds.nobinds");
        assert_eq!(rows_for(Some(Ok(&table())), "").len(), 3);
    }
}
