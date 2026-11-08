//! theme.json's shadcn color roles (spec "Visual system > Color", 2026-08-25
//! redesign), plus the hex format matugen renders them in, and the static
//! zinc values the shell falls back to when theme.json is absent or fails
//! validation.
//!
//! A palette is a `Map` with `mode` first and the [`COLOR_KEYS`] after it, in
//! that order, so `to_string_pretty` reads like the JSON the shell writes.
//! A mode argument is "light" or anything else, which means dark.

use crate::flexoki::{self, from_pairs};
use crate::zenbones;
use regex::Regex;
use serde_json::{Map, Value};
use std::sync::LazyLock;

pub const COLOR_KEYS: [&str; 26] = [
    "background",
    "foreground",
    "card",
    "cardForeground",
    "popover",
    "popoverForeground",
    "primary",
    "primaryForeground",
    "secondary",
    "secondaryForeground",
    "muted",
    "mutedForeground",
    "accent",
    "accentForeground",
    "destructive",
    "destructiveForeground",
    "warning",
    "warningForeground",
    "border",
    "input",
    "ring",
    "chart1",
    "chart2",
    "chart3",
    "chart4",
    "chart5",
];

static HEX_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^#[0-9a-fA-F]{6}$").expect("static hex pattern"));

fn is_hex(value: Option<&Value>) -> bool {
    value
        .and_then(Value::as_str)
        .is_some_and(|s| HEX_RE.is_match(s))
}

/// One static zinc variant per mode (shadcn's own zinc palette, spec's
/// "Color" table verbatim). An unrecognised mode means the dark variant: the
/// seeded first-boot theme.json and the absent-file default both depend on
/// that. `accent` is the neutral hover fill (not the wallpaper colour, which
/// lives in `primary`); chart1..5 reuse primary, secondary, warning and two
/// zinc steps since the static fallback has no matugen container roles to
/// draw the chart ramp from.
pub fn fallback(mode: &str) -> Map<String, Value> {
    if mode == "light" {
        return from_pairs(&[
            ("mode", "light"),
            ("background", "#ffffff"),
            ("foreground", "#09090b"),
            ("card", "#ffffff"),
            ("cardForeground", "#09090b"),
            ("popover", "#ffffff"),
            ("popoverForeground", "#09090b"),
            ("primary", "#18181b"),
            ("primaryForeground", "#fafafa"),
            ("secondary", "#f4f4f5"),
            ("secondaryForeground", "#18181b"),
            ("muted", "#f4f4f5"),
            ("mutedForeground", "#71717a"),
            ("accent", "#f4f4f5"),
            ("accentForeground", "#18181b"),
            ("destructive", "#e7000b"),
            ("destructiveForeground", "#ffffff"),
            ("warning", "#d97706"),
            ("warningForeground", "#ffffff"),
            ("border", "#e4e4e7"),
            ("input", "#e4e4e7"),
            ("ring", "#a1a1aa"),
            ("chart1", "#18181b"),
            ("chart2", "#f4f4f5"),
            ("chart3", "#d97706"),
            ("chart4", "#d4d4d8"),
            ("chart5", "#52525b"),
        ]);
    }
    from_pairs(&[
        ("mode", "dark"),
        ("background", "#09090b"),
        ("foreground", "#fafafa"),
        ("card", "#18181b"),
        ("cardForeground", "#fafafa"),
        ("popover", "#18181b"),
        ("popoverForeground", "#fafafa"),
        ("primary", "#e4e4e7"),
        ("primaryForeground", "#18181b"),
        ("secondary", "#27272a"),
        ("secondaryForeground", "#fafafa"),
        ("muted", "#27272a"),
        ("mutedForeground", "#a1a1aa"),
        ("accent", "#27272a"),
        ("accentForeground", "#fafafa"),
        ("destructive", "#ff6467"),
        ("destructiveForeground", "#fafafa"),
        ("warning", "#fbbf24"),
        ("warningForeground", "#18181b"),
        ("border", "#27272a"),
        ("input", "#3f3f46"),
        ("ring", "#71717a"),
        ("chart1", "#e4e4e7"),
        ("chart2", "#27272a"),
        ("chart3", "#fbbf24"),
        ("chart4", "#3f3f46"),
        ("chart5", "#71717a"),
    ])
}

type ModeView = fn(&str) -> Map<String, Value>;

/// A palette a wallpaper path can pin the shell to: the three views a pinned
/// matugen run rewrites its templates through (shadcn, material roles,
/// base16) plus the source colour the run hands `matugen color hex` in place
/// of the image, so the shell, theme.json and every app template read one
/// table.
pub struct Pin {
    pub name: &'static str,
    pub pattern: Regex,
    /// Uppercased, no leading `#`.
    pub source: String,
    shadcn_view: ModeView,
    material_roles_view: ModeView,
    base16_view: ModeView,
}

impl Pin {
    pub fn shadcn(&self, mode: &str) -> Map<String, Value> {
        (self.shadcn_view)(mode)
    }

    pub fn material_roles(&self, mode: &str) -> Map<String, Value> {
        (self.material_roles_view)(mode)
    }

    pub fn base16(&self, mode: &str) -> Map<String, Value> {
        (self.base16_view)(mode)
    }
}

/// A wallpaper whose path carries one of these names (any case, the
/// substring test the DMS-era flexoki-pin reconciler used) is pinned to that
/// palette instead of themed off its pixels.
static PINNED: LazyLock<Vec<Pin>> = LazyLock::new(|| {
    let pin = |name: &'static str,
               pattern: &str,
               source: &str,
               shadcn_view: ModeView,
               material_roles_view: ModeView,
               base16_view: ModeView| Pin {
        name,
        pattern: Regex::new(pattern).expect("static pin pattern"),
        source: source.replace('#', "").to_uppercase(),
        shadcn_view,
        material_roles_view,
        base16_view,
    };
    vec![
        pin(
            "flexoki",
            "(?i)flexoki",
            flexoki::SOURCE,
            flexoki::shadcn,
            flexoki::material_roles,
            flexoki::base16,
        ),
        pin(
            "zenbones",
            "(?i)zenbones",
            zenbones::SOURCE,
            zenbones::shadcn,
            zenbones::material_roles,
            zenbones::base16,
        ),
    ]
});

pub fn pinned_palette(wallpaper_path: &str) -> Option<&'static Pin> {
    PINNED.iter().find(|p| p.pattern.is_match(wallpaper_path))
}

/// Per-key backward-tolerant merge: a theme.json written before a key existed
/// (or mid-write with one bad value) falls back to zinc for that key alone,
/// never the whole object, so a live matugen run stays themed everywhere
/// except the one stale/missing field. The fill matches the theme's own mode
/// so a partial light theme.json never flashes dark tokens into a light UI.
pub fn merge_with_fallback(theme: Option<&Value>) -> Map<String, Value> {
    let mode = theme.and_then(|t| t.get("mode")).and_then(Value::as_str);
    let fb = fallback(if mode == Some("light") {
        "light"
    } else {
        "dark"
    });
    let mut merged = Map::new();
    merged.insert(
        "mode".to_string(),
        Value::String(mode.map_or_else(|| fallback_mode(&fb), str::to_string)),
    );
    for key in COLOR_KEYS {
        let value = theme.and_then(|t| t.get(key));
        let picked = if is_hex(value) { value } else { fb.get(key) };
        merged.insert(key.to_string(), picked.cloned().unwrap_or(Value::Null));
    }
    merged
}

fn fallback_mode(fb: &Map<String, Value>) -> String {
    fb.get("mode")
        .and_then(Value::as_str)
        .unwrap_or("dark")
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::fallback as fb;
    use super::*;
    use serde_json::json;

    fn valid_dark() -> Value {
        json!({
            "background": "#09090b", "foreground": "#fafafa",
            "card": "#18181b", "cardForeground": "#fafafa",
            "popover": "#18181b", "popoverForeground": "#fafafa",
            "primary": "#e4e4e7", "primaryForeground": "#18181b",
            "secondary": "#27272a", "secondaryForeground": "#fafafa",
            "muted": "#27272a", "mutedForeground": "#a1a1aa",
            "accent": "#27272a", "accentForeground": "#fafafa",
            "destructive": "#ff6467", "destructiveForeground": "#fafafa",
            "warning": "#fbbf24", "warningForeground": "#18181b",
            "border": "#27272a", "input": "#3f3f46", "ring": "#71717a",
            "chart1": "#e4e4e7", "chart2": "#27272a", "chart3": "#fbbf24",
            "chart4": "#3f3f46", "chart5": "#71717a"
        })
    }

    fn with(base: &Value, edits: &[(&str, Value)]) -> Value {
        let mut v = base.clone();
        for (k, val) in edits {
            v[*k] = val.clone();
        }
        v
    }

    fn s<'a>(m: &'a Map<String, Value>, k: &str) -> &'a str {
        m.get(k)
            .and_then(Value::as_str)
            .unwrap_or_else(|| panic!("missing {k}"))
    }

    fn ok(m: &Map<String, Value>) -> bool {
        COLOR_KEYS.iter().all(|k| is_hex(m.get(*k)))
    }

    #[test]
    fn fallback() {
        let f = fb("dark");
        assert!(ok(&f));
        assert_eq!(s(&f, "mode"), "dark");
        assert_eq!(s(&f, "border"), "#27272a");
        assert_eq!(s(&f, "primaryForeground"), "#18181b");
    }

    /// First-boot seed and the absent-file default both call fallback with no
    /// mode, which has to stay the dark variant; here that is any mode but
    /// "light".
    #[test]
    fn fallback_no_arg_is_dark() {
        assert_eq!(
            serde_json::to_string(&fb("")).unwrap(),
            serde_json::to_string(&fb("dark")).unwrap()
        );
    }

    /// shadcn's own zinc palette (spec "Visual system > Color" table,
    /// 2026-08-25 redesign). primary carries the wallpaper's own color under
    /// matugen; accent is a neutral hover fill, distinct from primary.
    #[test]
    fn fallback_dark_matches_the_spec_table() {
        let f = fb("dark");
        assert_eq!(s(&f, "background"), "#09090b");
        assert_eq!(s(&f, "foreground"), "#fafafa");
        assert_eq!(s(&f, "card"), "#18181b");
        assert_eq!(s(&f, "primary"), "#e4e4e7");
        assert_eq!(s(&f, "primaryForeground"), "#18181b");
        assert_eq!(s(&f, "mutedForeground"), "#a1a1aa");
        assert_eq!(s(&f, "accent"), "#27272a");
        assert_eq!(s(&f, "destructive"), "#ff6467");
        assert_eq!(s(&f, "warning"), "#fbbf24");
        assert_eq!(s(&f, "warningForeground"), "#18181b");
        assert_eq!(s(&f, "border"), "#27272a");
        assert_eq!(s(&f, "ring"), "#71717a");
    }

    #[test]
    fn fallback_light() {
        let f = fb("light");
        assert!(ok(&f));
        assert_eq!(s(&f, "mode"), "light");
        assert_eq!(s(&f, "background"), "#ffffff");
        assert_eq!(s(&f, "foreground"), "#09090b");
        assert_eq!(s(&f, "card"), "#ffffff");
        assert_eq!(s(&f, "primary"), "#18181b");
        assert_eq!(s(&f, "primaryForeground"), "#fafafa");
        assert_eq!(s(&f, "mutedForeground"), "#71717a");
        assert_eq!(s(&f, "accent"), "#f4f4f5");
        assert_eq!(s(&f, "destructive"), "#e7000b");
        assert_eq!(s(&f, "warning"), "#d97706");
        assert_eq!(s(&f, "border"), "#e4e4e7");
        assert_eq!(s(&f, "ring"), "#a1a1aa");
    }

    /// Flexoki on the same role set: black/b950/b900 surfaces, blue-400 as
    /// primary and ring, b800 borders. Every role validates so the static
    /// write path can hand it to theme.json unchanged.
    #[test]
    fn flexoki_dark() {
        let pin = pinned_palette("flexoki").unwrap();
        let f = pin.shadcn("dark");
        assert!(ok(&f));
        assert_eq!(s(&f, "mode"), "dark");
        assert_eq!(s(&f, "background"), "#100f0f");
        assert_eq!(s(&f, "card"), "#1c1b1a");
        assert_eq!(s(&f, "foreground"), "#cecdc3");
        assert_eq!(s(&f, "primary"), "#4385be");
        assert_eq!(s(&f, "ring"), "#4385be");
        assert_eq!(s(&f, "border"), "#403e3c");
        assert_eq!(s(&f, "destructive"), "#d14d41");
        assert_eq!(s(&f, "warning"), "#da702c");
        // The source a pinned matugen run is seeded with is the dark primary
        // itself, so the user's templates and the shell agree on the hue.
        assert_eq!(format!("#{}", pin.source.to_lowercase()), s(&f, "primary"));
    }

    #[test]
    fn flexoki_light() {
        let f = pinned_palette("flexoki").unwrap().shadcn("light");
        assert!(ok(&f));
        assert_eq!(s(&f, "mode"), "light");
        assert_eq!(s(&f, "background"), "#fffcf0");
        assert_eq!(s(&f, "card"), "#f2f0e5");
        assert_eq!(s(&f, "foreground"), "#100f0f");
        assert_eq!(s(&f, "primary"), "#205ea6");
        assert_eq!(s(&f, "ring"), "#205ea6");
        assert_eq!(s(&f, "border"), "#dad8ce");
        assert_eq!(s(&f, "destructive"), "#af3029");
        assert_eq!(s(&f, "warning"), "#bc5215");
    }

    #[test]
    fn flexoki_no_arg_is_dark() {
        let pin = pinned_palette("flexoki").unwrap();
        assert_eq!(
            serde_json::to_string(&pin.shadcn("")).unwrap(),
            serde_json::to_string(&pin.shadcn("dark")).unwrap()
        );
    }

    #[test]
    fn pinned_palette_is_a_case_insensitive_path_substring() {
        let name = |p: &str| pinned_palette(p).map(|x| x.name);
        assert_eq!(name("/w/dark/Moraine_Lake-flexoki.webp"), Some("flexoki"));
        assert_eq!(name("/w/dark/FLEXOKI-dark-orb.png"), Some("flexoki"));
        assert_eq!(name("/w/flexoki/anything.png"), Some("flexoki"));
        assert_eq!(name("/w/dark/ZenBones-pond.png"), Some("zenbones"));
        assert_eq!(name("/w/zenbones/anything.png"), Some("zenbones"));
        assert_eq!(name("/w/dark/wallhaven-yq2zwl.png"), None);
        assert_eq!(name(""), None);
    }

    /// A theme.json written before a role existed: the roles it does carry
    /// pass through untouched, the ones it lacks fall back individually,
    /// never the whole object.
    #[test]
    fn merge_with_fallback_fills_missing_keys() {
        let old = json!({
            "mode": "dark", "background": "#111111", "foreground": "#fafafa",
            "card": "#18181b", "primary": "#e4e4e7", "primaryForeground": "#222222"
        });
        let m = merge_with_fallback(Some(&old));
        assert_eq!(s(&m, "background"), "#111111");
        // primaryForeground itself is present and passes through verbatim
        // even though the dark zinc default differs from it.
        assert_eq!(s(&m, "primaryForeground"), "#222222");
        assert_eq!(s(&m, "border"), "#27272a");
        assert_eq!(s(&m, "mutedForeground"), "#a1a1aa");
        assert_eq!(s(&m, "warning"), "#fbbf24");
        assert_eq!(s(&m, "warningForeground"), "#18181b");
        assert_eq!(s(&m, "destructiveForeground"), "#fafafa");
    }

    /// mode "light", so the bad border falls back to the LIGHT variant's
    /// border: the merge fill matches the theme's own mode.
    #[test]
    fn merge_with_fallback_rejects_bad_hex_per_key() {
        let t = with(
            &valid_dark(),
            &[
                ("mode", json!("light")),
                ("border", json!("not-a-color")),
                ("primaryForeground", json!("#000000")),
            ],
        );
        let m = merge_with_fallback(Some(&t));
        assert_eq!(s(&m, "mode"), "light");
        assert_eq!(s(&m, "border"), "#e4e4e7");
        assert_eq!(s(&m, "primaryForeground"), "#000000");
    }

    #[test]
    fn merge_with_fallback_null_is_full_fallback() {
        let m = merge_with_fallback(None);
        assert_eq!(
            serde_json::to_string(&m).unwrap(),
            serde_json::to_string(&fb("dark")).unwrap()
        );
    }
}
