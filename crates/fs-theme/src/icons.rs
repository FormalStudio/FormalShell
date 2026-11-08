//! Icons by name: the Lucide table, and the distro logos the launcher
//! draws. The tables are generated files, read as data.

use std::collections::HashMap;
use std::sync::OnceLock;

use fs_types::Fraction;

const LUCIDE: &str = include_str!("../icons/lucide.js");
const DISTRO: &str = include_str!("../icons/distro.js");

/// The family Lucide's glyphs are drawn in.
pub const LUCIDE_FAMILY: &str = "lucide";
/// The font-logos range Nerd Fonts embeds, where the distro marks resolve.
pub const SYMBOLS_FAMILY: &str = "Symbols Nerd Font";
/// The generic Tux, for a Linux the logo table does not name.
pub const TUX: &str = "\u{F31A}";

/// A glyph and the family it is drawn in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Glyph {
    pub text: &'static str,
    pub family: &'static str,
}

struct Table {
    glyphs: HashMap<&'static str, &'static str>,
    family: &'static str,
}

/// `"name": "\u{XXXX}"` pairs, one per line, as the generators write them.
fn parse(source: &'static str) -> HashMap<&'static str, &'static str> {
    let mut out = HashMap::new();
    for line in source.lines() {
        let line = line.trim();
        let Some(rest) = line.strip_prefix('"') else { continue };
        let Some((name, rest)) = rest.split_once('"') else { continue };
        let Some(start) = rest.find("\"\\u{") else { continue };
        let code = &rest[start + 4..];
        let Some(end) = code.find('}') else { continue };
        let Some(c) = u32::from_str_radix(&code[..end], 16).ok().and_then(char::from_u32) else { continue };
        let text: &'static str = Box::leak(c.to_string().into_boxed_str());
        out.entry(name).or_insert(text);
    }
    out
}

fn lucide() -> &'static Table {
    static T: OnceLock<Table> = OnceLock::new();
    T.get_or_init(|| Table { glyphs: parse(LUCIDE), family: LUCIDE_FAMILY })
}

fn distro() -> &'static HashMap<&'static str, &'static str> {
    static T: OnceLock<HashMap<&'static str, &'static str>> = OnceLock::new();
    T.get_or_init(|| parse(DISTRO))
}

/// A Lucide glyph by name; a name missing from the table is its own
/// `circle-help`.
pub fn glyph(name: &str) -> Glyph {
    let table = lucide();
    let text = table.glyphs.get(name).or_else(|| table.glyphs.get("circle-help")).copied().unwrap_or("?");
    Glyph { text, family: table.family }
}

/// The speaker for a volume's share of its track: none, under half, or more.
pub fn volume(level: Fraction) -> &'static str {
    match level.as_fraction() {
        l if l <= 0.0 => "volume-x",
        l if l < 0.5 => "volume-1",
        _ => "volume-2",
    }
}

/// The real logo for an os-release `ID`, or none.
pub fn distro_logo(id: &str) -> Option<&'static str> {
    distro().get(id.trim().to_lowercase().as_str()).copied()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tables_parse() {
        assert!(lucide().glyphs.len() > 1500);
        assert_eq!(glyph("bell").text, "\u{E059}");
        assert_eq!(glyph("no-such-icon"), glyph("circle-help"));
        assert_eq!(distro_logo("NixOS"), Some("\u{F313}"));
        assert_eq!(distro_logo("plan9"), None);
    }

    #[test]
    fn volume_steps_at_half() {
        let at = |f| volume(Fraction::from_fraction(f));
        assert_eq!([at(0.0), at(0.49), at(0.5), at(1.0)], ["volume-x", "volume-1", "volume-2", "volume-2"]);
    }
}
