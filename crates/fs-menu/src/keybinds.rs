//! Compositor keybinds as launcher rows (keybinds.js): Hyprland's from
//! `hyprctl binds`, with its sourced files and submaps already expanded,
//! and mango's from the `bind*=` lines of its config, which its backend
//! walks with the `source=` files in place.

use crate::node::{Kind, Node};

pub const KEYBINDS_CHORD_PAD_MAX: usize = 24;
pub const KEYBINDS_MAX_RESULTS: usize = 200;

/// A Text item's implicit width drops trailing ASCII spaces, so the chord
/// column pads with U+00A0.
const PAD_CHAR: char = '\u{a0}';

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Bind {
    pub chord: String,
    pub mods: Vec<&'static str>,
    pub key: String,
    pub action: String,
    pub args: Vec<String>,
    pub title: String,
    pub submap: String,
    pub catchall: bool,
}

/// wlroots' modifier bits in the order a chord is written, so 65 reads
/// "SUPER+SHIFT".
const HYPR_MODS: [(i64, &str); 8] =
    [(64, "SUPER"), (4, "CTRL"), (8, "ALT"), (1, "SHIFT"), (2, "CAPS"), (16, "MOD2"), (32, "MOD3"), (128, "MOD5")];

fn hypr_mods(modmask: &str) -> Vec<&'static str> {
    let mask = modmask.trim().parse::<f64>().map(|m| m as i64).unwrap_or(0);
    HYPR_MODS.iter().filter(|(bit, _)| mask & bit != 0).map(|(_, name)| *name).collect()
}

fn bind_from_fields(fields: &[(String, String)]) -> Bind {
    let get = |k: &str| fields.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str());
    let mods = hypr_mods(get("modmask").unwrap_or(""));
    // An empty `key` is a bind written against a raw keycode.
    let mut key = get("key").unwrap_or("").to_string();
    if key.is_empty() {
        key = format!("code:{}", get("keycode").unwrap_or(""));
    }
    let arg = get("arg").unwrap_or("");
    // A Lua bind reports `__lua` and an internal reference number as its arg.
    let lua = get("dispatcher") == Some("__lua");
    let mut chord: Vec<&str> = mods.clone();
    chord.push(&key);
    Bind {
        chord: chord.join("+"),
        mods,
        key: key.clone(),
        action: if lua { String::new() } else { get("dispatcher").unwrap_or("").to_string() },
        args: if arg.is_empty() || lua { Vec::new() } else { vec![arg.to_string()] },
        title: get("description").unwrap_or("").to_string(),
        submap: get("submap").unwrap_or("").to_string(),
        catchall: get("catchall") == Some("true"),
    }
}

fn field_line(line: &str) -> Option<(String, String)> {
    let body = line.trim_start_matches([' ', '\t']);
    if body.len() == line.len() {
        return None;
    }
    let (name, rest) = body.split_once(':')?;
    let mut chars = name.chars();
    let first = chars.next()?;
    if !(first.is_ascii_alphabetic() || first == '_') || !chars.all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return None;
    }
    Some((name.to_string(), rest.strip_prefix(' ').unwrap_or(rest).to_string()))
}

/// The plain-text `hyprctl binds` table, never `-j`: Hyprland 0.56.0's JSON
/// encoder shifts `modmask` onward under the previous key's name and emits
/// `allow_input_capture` with no value, so that reply is not JSON.
///
/// One block per bind, headed by a bare `bind[flags]` line, then one indented
/// `name: value` per line, blocks separated by a blank line. A value may be
/// empty and may hold a colon, so only the first one splits.
pub fn parse_hyprland_binds(text: &str) -> Vec<Bind> {
    let mut out = Vec::new();
    let mut fields: Option<Vec<(String, String)>> = None;
    for raw in text.split('\n') {
        let line = raw.trim_end();
        if line.starts_with("bind") && line[4..].chars().all(|c| c.is_ascii_lowercase()) {
            if let Some(f) = fields.take() {
                out.push(bind_from_fields(&f));
            }
            fields = Some(Vec::new());
            continue;
        }
        let Some(f) = fields.as_mut() else { continue };
        if let Some(pair) = field_line(line) {
            f.push(pair);
        } else if line.is_empty() {
            out.push(bind_from_fields(f));
            fields = None;
        }
    }
    if let Some(f) = fields {
        out.push(bind_from_fields(&f));
    }
    out
}

/// mango's own modifier names, any case; anything else (a `code:` keycode)
/// stays in the chord but names no modifier.
fn mango_mod(token: &str) -> Option<&'static str> {
    match token.to_ascii_lowercase().as_str() {
        "super" | "logo" | "mod4" => Some("SUPER"),
        "ctrl" | "control" => Some("CTRL"),
        "alt" | "mod1" => Some("ALT"),
        "shift" => Some("SHIFT"),
        _ => None,
    }
}

/// One `bind[slrpc]*=MODS,KEY,FUNC,ARGS...` line of a mango config, after
/// its variables are expanded; `key_mode` is the mode the lines above set.
/// `None` for any other line.
pub fn parse_mango_bind(key: &str, value: &str, key_mode: &str) -> Option<Bind> {
    let flags = key.trim().strip_prefix("bind")?;
    if !flags.chars().all(|c| "slrpc".contains(c)) {
        return None;
    }
    let mut parts = value.splitn(4, ',').map(str::trim);
    let (mods_text, key, action) = (parts.next()?, parts.next()?, parts.next()?);
    if key.is_empty() || action.is_empty() {
        return None;
    }
    let args = parts.next().unwrap_or("");
    let mut mods = Vec::new();
    let mut chord: Vec<String> = Vec::new();
    for token in mods_text.split('+').map(str::trim).filter(|t| !t.is_empty() && !t.eq_ignore_ascii_case("none")) {
        match mango_mod(token) {
            Some(name) => {
                mods.push(name);
                chord.push(name.to_owned());
            }
            None => chord.push(token.to_owned()),
        }
    }
    chord.push(key.to_owned());
    let submap = if key_mode == "default" { String::new() } else { key_mode.to_owned() };
    Some(Bind {
        chord: chord.join("+"),
        mods,
        key: key.to_owned(),
        action: action.to_owned(),
        args: if args.is_empty() { Vec::new() } else { vec![args.to_owned()] },
        title: String::new(),
        submap,
        catchall: false,
    })
}

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
                actions: vec![crate::node::ActionEntry::new(
                    "copy-chord",
                    "Copy chord",
                    format!("wl-copy -- {}", crate::actions::shell_quote(&bind.chord)),
                )],
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

    const TABLE: &str = "bind\n\tlocked: false\n\tmodmask: 65\n\tsubmap: \n\tkey: slash\n\tkeycode: 0\n\tcatchall: false\n\tdispatcher: exec\n\targ: formalshell-ipc call menu toggle\n\nbindd\n\tmodmask: 64\n\tkey: \n\tkeycode: 36\n\tdescription: Open: the menu\n\tdispatcher: __lua\n\targ: 12\n\nbindel\n\tmodmask: 0\n\tkey: XF86AudioRaiseVolume\n\tdispatcher: exec\n\targ: wpctl set-volume @DEFAULT_AUDIO_SINK@ 5%+\n";

    #[test]
    fn parses_the_table() {
        let binds = parse_hyprland_binds(TABLE);
        assert_eq!(binds.len(), 3);
        assert_eq!(binds[0].chord, "SUPER+SHIFT+slash");
        assert_eq!(binds[0].mods, ["SUPER", "SHIFT"]);
        assert_eq!(describe_action(&binds[0]), "exec formalshell-ipc call menu toggle");
        assert_eq!(binds[1].chord, "SUPER+code:36");
        assert_eq!(binds[1].action, "");
        assert!(binds[1].args.is_empty());
        assert_eq!(describe_action(&binds[1]), "Open: the menu");
        assert_eq!(binds[2].chord, "XF86AudioRaiseVolume");
        assert!(parse_hyprland_binds("").is_empty());
        assert!(parse_hyprland_binds("garbage\n\tkey: x").is_empty());
    }

    #[test]
    fn ranks_and_pads() {
        let binds = parse_hyprland_binds(TABLE);
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
        assert_eq!(rows_for(Some(Ok(&parse_hyprland_binds(""))), "")[0].id, "keybinds.nobinds");
        assert_eq!(rows_for(Some(Ok(&parse_hyprland_binds(TABLE))), "").len(), 3);
    }

    #[test]
    fn mango_bind_lines() {
        let b = parse_mango_bind("bind", "SUPER+shift, Return ,spawn,foot -e sh -c 'a,b'", "default").unwrap();
        assert_eq!(b.chord, "SUPER+SHIFT+Return");
        assert_eq!(b.mods, ["SUPER", "SHIFT"]);
        assert_eq!(describe_action(&b), "spawn foot -e sh -c 'a,b'");
        assert_eq!(b.submap, "");
        let b = parse_mango_bind("bindl", "NONE,XF86AudioMute,spawn,wpctl", "resize").unwrap();
        assert_eq!((b.chord.as_str(), b.submap.as_str()), ("XF86AudioMute", "resize"));
        let b = parse_mango_bind("bind", "code:64+alt,code:24,killclient", "default").unwrap();
        assert_eq!((b.chord.as_str(), b.mods.as_slice()), ("code:64+ALT+code:24", ["ALT"].as_slice()));
        assert!(b.args.is_empty());
        assert!(parse_mango_bind("mousebind", "SUPER,btn_left,moveresize,curmove", "default").is_none());
        assert!(parse_mango_bind("bindx", "SUPER,a,killclient", "default").is_none());
        assert!(parse_mango_bind("bind", "SUPER,a", "default").is_none());
    }
}
