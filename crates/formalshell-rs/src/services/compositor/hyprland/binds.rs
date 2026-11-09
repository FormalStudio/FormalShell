//! `hyprctl binds`, with its sourced files and submaps already expanded, as
//! the launcher's keybinds route reads them.

use fs_types::Bind;

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
pub fn parse(text: &str) -> Vec<Bind> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use fs_menu::keybinds::{describe_action, rows_for};

    const TABLE: &str = "bind\n\tlocked: false\n\tmodmask: 65\n\tsubmap: \n\tkey: slash\n\tkeycode: 0\n\tcatchall: false\n\tdispatcher: exec\n\targ: formalshell-ipc call menu toggle\n\nbindd\n\tmodmask: 64\n\tkey: \n\tkeycode: 36\n\tdescription: Open: the menu\n\tdispatcher: __lua\n\targ: 12\n\nbindel\n\tmodmask: 0\n\tkey: XF86AudioRaiseVolume\n\tdispatcher: exec\n\targ: wpctl set-volume @DEFAULT_AUDIO_SINK@ 5%+\n";

    #[test]
    fn parses_the_table() {
        let binds = parse(TABLE);
        assert_eq!(binds.len(), 3);
        assert_eq!(binds[0].chord, "SUPER+SHIFT+slash");
        assert_eq!(binds[0].mods, ["SUPER", "SHIFT"]);
        assert_eq!(describe_action(&binds[0]), "exec formalshell-ipc call menu toggle");
        assert_eq!(binds[1].chord, "SUPER+code:36");
        assert_eq!(binds[1].action, "");
        assert!(binds[1].args.is_empty());
        assert_eq!(describe_action(&binds[1]), "Open: the menu");
        assert_eq!(binds[2].chord, "XF86AudioRaiseVolume");
        assert!(parse("").is_empty());
        assert!(parse("garbage\n\tkey: x").is_empty());
    }

    #[test]
    fn an_empty_table_is_the_no_binds_row() {
        assert_eq!(rows_for(Some(Ok(&parse(""))), "")[0].id, "keybinds.nobinds");
        assert_eq!(rows_for(Some(Ok(&parse(TABLE))), "").len(), 3);
    }
}
