//! Builds a matugen `-c` TOML config from the spec-mandated merge order:
//! user `[config]` verbatim, then the shell's own template blocks, then the
//! user's `[templates.*]` section verbatim, then any drop-in fragments. Also
//! the pure halves of a pinned run: rewriting a template's color expressions
//! to a pinned palette's tones before matugen renders it.

use crate::palette::Pin;
use regex::{Captures, Regex};
use serde_json::{Map, Value};
use std::sync::LazyLock;

static RANK_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\]\s*([0-9]+):\s*(#[0-9a-fA-F]{6})").expect("static rank pattern")
});

/// Rank 0 of matugen's own candidate ranking, read off a `-d` run's stderr:
///
/// ```text
/// ... matugen::color::color] Ranked colors:
/// ... matugen::color::color] 0: #648db8
/// ... matugen::color::color] 1: #908a61
/// ```
///
/// The ranking is material's Score order, so rank 0 is the image's own color;
/// ThemeEngine's header covers why none of the --prefer scalars substitutes
/// for it. Returns `None` when the ranking isn't there to read (a matugen
/// whose debug output moved, a run that died before extraction), which the
/// caller answers with a plain --prefer run rather than by skipping the
/// retheme.
pub fn ranked_source_color(text: &str) -> Option<String> {
    if text.is_empty() {
        return None;
    }
    let mut in_ranking = false;
    for line in text.split('\n') {
        if !in_ranking {
            in_ranking = line.contains("Ranked colors:");
            continue;
        }
        // Anchored on the log prefix's own closing bracket so a timestamp can
        // never be read as a rank.
        let Some(entry) = RANK_RE.captures(line) else {
            continue;
        };
        return if &entry[1] == "0" {
            Some(entry[2].to_lowercase())
        } else {
            None
        };
    }
    None
}

/// The lines from the first `[name]` or `[name.*]` header up to the next
/// header that is not part of that table. `name` is spliced into a regex
/// unescaped.
pub fn extract_section(text: &str, name: &str) -> String {
    if text.is_empty() {
        return String::new();
    }
    let Ok(header_re) = Regex::new(&format!(r"^\[{name}(\.|\])")) else {
        return String::new();
    };
    let lines: Vec<&str> = text.split('\n').collect();
    let Some(start) = lines.iter().position(|l| header_re.is_match(l.trim())) else {
        return String::new();
    };
    let mut end = lines.len();
    for (j, raw) in lines.iter().enumerate().skip(start + 1) {
        let line = raw.trim();
        if line.starts_with('[') && !header_re.is_match(line) {
            end = j;
            break;
        }
    }
    lines[start..end].join("\n")
}

pub fn template_block(name: &str, input_path: &str, output_path: &str) -> String {
    format!("[templates.{name}]\ninput_path = '{input_path}'\noutput_path = '{output_path}'\n")
}

/// The no-wallpaper twin of hyprland-colors.lua.tmpl. matugen only runs
/// against an image, so the fallback palette has to render the same seven
/// keys itself: without this a hyprland.lua dofile-ing the path would read a
/// file that never appears until the first wallpaper is set. Keep the name
/// list and the header in step with the template.
pub const HYPRLAND_VARS: [&str; 7] = [
    "primary",
    "primaryForeground",
    "background",
    "foreground",
    "border",
    "destructive",
    "warning",
];

fn hypr_rgb(value: &str) -> String {
    format!("rgb({})", value.replacen('#', "", 1))
}

pub fn hyprland_colors(palette: &Map<String, Value>) -> String {
    let mut out = String::from(
        "-- Rendered by matugen (ThemeEngine) into ~/.config/hypr/formalshell-colors.lua\n\
         -- on every wallpaper/mode change; `dofile` it from hyprland.lua.\n\
         return {\n",
    );
    for name in HYPRLAND_VARS {
        let value = match palette.get(name) {
            Some(Value::String(s)) => s.clone(),
            Some(other) => other.to_string(),
            None => "undefined".to_string(),
        };
        out.push_str(&format!("  {name} = \"{}\",\n", hypr_rgb(&value)));
    }
    out.push_str("}\n");
    out
}

/// The seven roles [`hyprland_colors`] renders, as `#rrggbb` the way
/// theme.json holds them, for a compositor that takes them over its socket.
pub fn compositor_colors(palette: &Map<String, Value>) -> Map<String, Value> {
    HYPRLAND_VARS.iter().filter_map(|name| Some(((*name).to_owned(), palette.get(*name)?.clone()))).collect()
}

#[derive(Debug, Clone, Default)]
pub struct BuildOpts {
    pub shell_template_dir: String,
    pub state_dir: String,
    pub home_dir: String,
    pub user_config_text: Option<String>,
    pub drop_in_texts: Vec<String>,
}

pub fn build_config(opts: &BuildOpts) -> String {
    let mut parts: Vec<String> = Vec::new();
    let tpl = &opts.shell_template_dir;
    let state = &opts.state_dir;
    let home = &opts.home_dir;
    let user_text = opts.user_config_text.as_deref().filter(|t| !t.is_empty());

    // matugen hard-rejects a config file with no top-level [config] table
    // ("missing field `config`"), so this must always emit one, the user's
    // verbatim section if they have one, otherwise a bare header. A fresh
    // install with no ~/.config/matugen/config.toml must still produce a
    // config matugen will run.
    let user_config = user_text
        .map(|t| extract_section(t, "config"))
        .unwrap_or_default();
    parts.push(if user_config.is_empty() {
        "[config]".to_string()
    } else {
        user_config
    });

    parts.push(template_block(
        "formalshell",
        &format!("{tpl}/theme.json.tmpl"),
        &format!("{state}/theme.json.tmp"),
    ));
    parts.push(template_block(
        "formalshell-hyprland",
        &format!("{tpl}/hyprland-colors.lua.tmpl"),
        &format!("{state}/formalshell-colors.lua.tmp"),
    ));

    // App-facing palettes, written straight to their final config paths: only
    // theme.json and the Hyprland colours need the .tmp + rename dance (the
    // shell watches one, Hyprland dofiles the other on reload, so neither can
    // afford a torn read); GTK and Qt apps read these at launch, so a direct
    // matugen write is fine. gtk.css imports formalshell-colors.css; the
    // qt{5,6}ct.conf color_scheme_path points at colors/matugen.conf.
    parts.push(template_block(
        "formalshell-gtk3",
        &format!("{tpl}/gtk-colors.css.tmpl"),
        &format!("{home}/.config/gtk-3.0/formalshell-colors.css"),
    ));
    parts.push(template_block(
        "formalshell-gtk4",
        &format!("{tpl}/gtk-colors.css.tmpl"),
        &format!("{home}/.config/gtk-4.0/formalshell-colors.css"),
    ));
    parts.push(template_block(
        "formalshell-qt5ct",
        &format!("{tpl}/qtct-colors.conf.tmpl"),
        &format!("{home}/.config/qt5ct/colors/matugen.conf"),
    ));
    parts.push(template_block(
        "formalshell-qt6ct",
        &format!("{tpl}/qtct-colors.conf.tmpl"),
        &format!("{home}/.config/qt6ct/colors/matugen.conf"),
    ));

    if let Some(text) = user_text {
        let user_templates = extract_section(text, "templates");
        if !user_templates.is_empty() {
            parts.push(user_templates);
        }
    }

    for text in &opts.drop_in_texts {
        if !text.is_empty() {
            parts.push(text.clone());
        }
    }

    parts.join("\n")
}

// A pinned palette (Flexoki, zenbones) can't be handed to matugen: its scheme
// always grows out of one source colour, so a run seeded with the pin's blue
// still renders every template as a single-hue Material scheme and a terminal
// whose ANSI slots read primary/secondary/tertiary comes out blue where it
// asked for green, yellow and magenta. So the pin rewrites the templates
// instead. Each {{colors.*}} and {{base16.*}} value expression is replaced
// with the literal tone in the format it asked for, and matugen renders the
// rewritten copy. Everything else in the file is matugen's: {{image}},
// {{mode}}, post_hook, output_path, and the templates the user's own config
// declares.
//
// A filter pipeline survives the rewrite only on a `.hex` value, as
// `{{ "#4385be" | to_color | <filters> }}`: matugen 4.1.0 rejects a colour
// filter applied straight to a string (ParseError::ColorFilterOnString) and
// `to_color` renders hex whatever went in, so an rgb/hsl/hex_stripped value
// under a filter would come out in the wrong syntax and, wrapped in an
// `rgb(...)` the way a hyprland config wraps hex_stripped, would break the
// file it lands in. Those keep matugen's own value and are named in
// `skipped` instead. No template in the tree uses one.

fn rgb_parts(hex: &str) -> [i64; 3] {
    let h = hex.replacen('#', "", 1);
    let byte = |from: usize| {
        h.get(from..from + 2)
            .and_then(|s| i64::from_str_radix(s, 16).ok())
            .unwrap_or(0)
    };
    [byte(0), byte(2), byte(4)]
}

/// matugen prints hsl rounded to whole degrees and whole percent
/// (`hsl(210, 92%, 80%)`), so this rounds the same way rather than carrying
/// decimals a template would render differently from a non-pinned run.
fn hsl_parts(hex: &str) -> [i64; 3] {
    let rgb = rgb_parts(hex);
    let r = rgb[0] as f64 / 255.0;
    let g = rgb[1] as f64 / 255.0;
    let b = rgb[2] as f64 / 255.0;
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let l = (max + min) / 2.0;
    let d = max - min;
    let mut h = 0.0;
    let mut s = 0.0;
    if d != 0.0 {
        s = if l > 0.5 {
            d / (2.0 - max - min)
        } else {
            d / (max + min)
        };
        h = if max == r {
            ((g - b) / d) % 6.0
        } else if max == g {
            (b - r) / d + 2.0
        } else {
            (r - g) / d + 4.0
        };
        h *= 60.0;
        if h < 0.0 {
            h += 360.0;
        }
    }
    [
        h.round() as i64,
        (s * 100.0).round() as i64,
        (l * 100.0).round() as i64,
    ]
}

pub fn format_color(hex: &str, format: &str) -> Option<String> {
    Some(match format {
        "hex" => hex.to_string(),
        "hex_stripped" => hex.replacen('#', "", 1),
        "rgb" => {
            let c = rgb_parts(hex);
            format!("rgb({}, {}, {})", c[0], c[1], c[2])
        }
        "rgba" => {
            let c = rgb_parts(hex);
            format!("rgba({}, {}, {}, 1)", c[0], c[1], c[2])
        }
        "hsl" => {
            let c = hsl_parts(hex);
            format!("hsl({}, {}%, {}%)", c[0], c[1], c[2])
        }
        "hsla" => {
            let c = hsl_parts(hex);
            format!("hsla({}, {}%, {}%, 1)", c[0], c[1], c[2])
        }
        _ => return None,
    })
}

static EXPR_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"\{\{\s*(colors|base16)\.([A-Za-z0-9_]+)\.(default|light|dark)\.([A-Za-z_]+)\s*(\|[^{}]*?)?\}\}",
    )
    .expect("static expression pattern")
});

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Substituted {
    pub text: String,
    pub substituted: usize,
    pub skipped: Vec<String>,
}

/// Replaces every color expression the pinned palette (a `palette::Pin`) can
/// answer. An expression it cannot (a custom_colors entry the user declared,
/// a format matugen grew since) is left verbatim for matugen to render from
/// its own scheme, and named in `skipped` so the caller can say so once
/// rather than silently shipping one blue value in an otherwise pinned file.
pub fn substitute_pinned(text: &str, mode: &str, pin: &Pin) -> Substituted {
    if text.is_empty() {
        return Substituted {
            text: text.to_string(),
            substituted: 0,
            skipped: Vec::new(),
        };
    }
    let colors = [pin.material_roles("light"), pin.material_roles("dark")];
    let base16 = [pin.base16("light"), pin.base16("dark")];
    let fallback_scheme = if mode == "light" { "light" } else { "dark" };
    let mut count = 0;
    let mut skipped: Vec<String> = Vec::new();
    let out = EXPR_RE.replace_all(text, |caps: &Captures| {
        let (group, name, format) = (&caps[1], &caps[2], &caps[4]);
        let scheme = if &caps[3] == "default" {
            fallback_scheme
        } else {
            &caps[3]
        };
        let idx = usize::from(scheme == "dark");
        let table = if group == "colors" {
            &colors[idx]
        } else {
            &base16[idx]
        };
        let mut key = name;
        if !table.contains_key(key) && group == "base16" {
            // matugen dumps base0a, the matugen-themes templates write
            // base0A; the pinned base16 tables use the upper form.
            if let Some(candidate) = table.keys().find(|c| c.eq_ignore_ascii_case(name)) {
                key = candidate;
            }
        }
        let value = table
            .get(key)
            .and_then(Value::as_str)
            .filter(|v| !v.is_empty());
        let filters = caps.get(5).map(|m| m.as_str());
        let rendered = value.and_then(|v| format_color(v, format));
        match rendered {
            Some(rendered) if !(filters.is_some() && format != "hex") => {
                count += 1;
                match filters {
                    Some(f) => format!("{{{{ \"{rendered}\" | to_color {} }}}}", f.trim()),
                    None => rendered,
                }
            }
            _ => {
                let id = format!("{group}.{name}.{format}");
                if !skipped.contains(&id) {
                    skipped.push(id);
                }
                caps[0].to_string()
            }
        }
    });
    Substituted {
        text: out.into_owned(),
        substituted: count,
        skipped,
    }
}

/// matugen expands a leading ~ in input_path/output_path itself; a rewritten
/// copy is read by this shell first, so the same expansion has to happen
/// here.
pub fn expand_home(path: &str, home: &str) -> String {
    if path == "~" {
        return home.to_string();
    }
    match path.strip_prefix('~') {
        Some(rest) if rest.starts_with('/') => format!("{home}{rest}"),
        _ => path.to_string(),
    }
}

// The JS pattern closes on a backreference to the opening quote; the regex
// crate has none, so each quote style is its own alternative.
static INPUT_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?m)^([ \t]*input_path[ \t]*=[ \t]*)(?:'([^'"\n]*)'|"([^'"\n]*)")"#)
        .expect("static input_path pattern")
});

fn input_path<'a>(caps: &'a Captures) -> &'a str {
    caps.get(2)
        .or_else(|| caps.get(3))
        .map_or("", |m| m.as_str())
}

pub fn template_inputs(config_text: &str) -> Vec<String> {
    INPUT_RE
        .captures_iter(config_text)
        .map(|caps| input_path(&caps).to_string())
        .collect()
}

/// Repoints each `[templates.*]` at its rewritten copy, in the order
/// [`template_inputs`] returned them. A mapper answering `None` leaves that
/// entry on its original file, which is what an unreadable template gets:
/// matugen then fails on it exactly as it would have without the pin.
pub fn rewrite_template_inputs(
    config_text: &str,
    mut mapper: impl FnMut(&str, usize) -> Option<String>,
) -> String {
    let mut i = 0;
    INPUT_RE
        .replace_all(config_text, |caps: &Captures| {
            let index = i;
            i += 1;
            match mapper(input_path(caps), index) {
                Some(next) => format!("{}'{next}'", &caps[1]),
                None => caps[0].to_string(),
            }
        })
        .into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opts(
        tpl: &str,
        state: &str,
        home: &str,
        user: Option<&str>,
        drop_ins: &[&str],
    ) -> BuildOpts {
        BuildOpts {
            shell_template_dir: tpl.to_string(),
            state_dir: state.to_string(),
            home_dir: home.to_string(),
            user_config_text: user.map(str::to_string),
            drop_in_texts: drop_ins.iter().map(|s| (*s).to_string()).collect(),
        }
    }

    #[test]
    fn merge_order() {
        let cfg = build_config(&opts(
            "/shell/tpl",
            "/state",
            "/home/u",
            Some("[config]\nreload_apps = true\n[templates.ghostty]\ninput_path = 'x'\n"),
            &["[templates.extra]\ninput_path = 'y'\n"],
        ));
        let i_cfg = cfg.find("reload_apps").unwrap();
        let i_shell = cfg.find("[templates.formalshell]").unwrap();
        let i_user = cfg.find("[templates.ghostty]").unwrap();
        let i_drop = cfg.find("[templates.extra]").unwrap();
        assert!(i_cfg < i_shell && i_shell < i_user && i_user < i_drop);
        assert!(cfg.contains("/state/theme.json.tmp"));
        assert!(cfg.contains("[templates.formalshell-hyprland]"));
        assert!(cfg.contains("/shell/tpl/hyprland-colors.lua.tmpl"));
        assert!(cfg.contains("/state/formalshell-colors.lua.tmp"));
        assert!(!cfg.contains("hyprland-colors.conf"));
        assert!(cfg.contains("/home/u/.config/gtk-3.0/formalshell-colors.css"));
        assert!(cfg.contains("/home/u/.config/gtk-4.0/formalshell-colors.css"));
        assert!(cfg.contains("/home/u/.config/qt5ct/colors/matugen.conf"));
        assert!(cfg.contains("/home/u/.config/qt6ct/colors/matugen.conf"));
    }

    /// matugen hard-rejects a config with no top-level [config] table, so a
    /// fresh install (no ~/.config/matugen/config.toml) must still get a bare
    /// one, ahead of the shell's own template blocks.
    #[test]
    fn no_user_config() {
        let cfg = build_config(&opts("/t", "/s", "/h", None, &[]));
        assert!(cfg.contains("[config]"));
        assert!(cfg.contains("[templates.formalshell]"));
        assert!(cfg.find("[config]").unwrap() < cfg.find("[templates.formalshell]").unwrap());
    }

    #[test]
    fn extract_section() {
        let t = "[config]\na = 1\n[templates.x]\nb = 2\n";
        assert!(super::extract_section(t, "config").contains("a = 1"));
        assert!(super::extract_section(t, "templates").contains("b = 2"));
        assert_eq!(super::extract_section("", "config"), "");
    }

    /// Verbatim stderr from `matugen -d image ... --dry-run` (matugen 4.1.0,
    /// captured on g815 against dark/ARC - Towers.PNG), ANSI runs and all.
    const PROBE_STDERR: &str = concat!(
        "[2026-08-14T13:38:49.145998Z INFO  matugen::color::color] Opening image in dark/ARC - Towers.PNG\n",
        "[2026-08-14T13:38:49.596348Z DEBUG matugen::color::color] Ranked colors:\n",
        "[2026-08-14T13:38:49.596356Z DEBUG matugen::color::color] 0: #648db8 \x1b[37;48;2;100;141;184m  \x1b[0m\n",
        "[2026-08-14T13:38:49.596357Z DEBUG matugen::color::color] 1: #908a61 \x1b[37;48;2;144;138;97m  \x1b[0m\n",
        "[2026-08-14T13:38:49.596361Z DEBUG matugen::color::color] Multiple source colors found, attempting to pick a color by user preference \"Saturation\"\n",
        "[2026-08-14T13:38:49.596362Z DEBUG matugen::color::color] Chose 1\n",
    );

    /// The no-wallpaper path renders these seven itself, so the names, the
    /// order and the rgb() form all have to match hyprland-colors.lua.tmpl.
    /// The `return` and the string quoting are load-bearing: a config reads
    /// this through dofile, so anything that is not a table literal comes back
    /// nil and the whole palette silently falls through to the static
    /// fallback.
    #[test]
    fn hyprland_colors() {
        let palette: Map<String, Value> = [
            ("primary", "#648db8"),
            ("primaryForeground", "#ffffff"),
            ("background", "#09090b"),
            ("foreground", "#fafafa"),
            ("border", "#27272a"),
            ("destructive", "#e7000b"),
            ("warning", "#d97706"),
        ]
        .iter()
        .map(|(k, v)| ((*k).to_string(), Value::String((*v).to_string())))
        .collect();
        let out = super::hyprland_colors(&palette);
        let lines: Vec<&str> = out.trim().split('\n').collect();
        assert_eq!(&lines[0][..2], "--");
        assert_eq!(&lines[1][..2], "--");
        assert_eq!(
            lines[2..].join("\n"),
            "return {\n\
             \x20 primary = \"rgb(648db8)\",\n\
             \x20 primaryForeground = \"rgb(ffffff)\",\n\
             \x20 background = \"rgb(09090b)\",\n\
             \x20 foreground = \"rgb(fafafa)\",\n\
             \x20 border = \"rgb(27272a)\",\n\
             \x20 destructive = \"rgb(e7000b)\",\n\
             \x20 warning = \"rgb(d97706)\",\n\
             }"
        );
    }

    #[test]
    fn ranked_source_color() {
        assert_eq!(
            super::ranked_source_color(PROBE_STDERR).as_deref(),
            Some("#648db8")
        );
    }

    /// No ranking to read means the caller falls back to a --prefer run, so
    /// every one of these has to answer None rather than guess. The last two
    /// are the ones a looser match would get wrong: a timestamp carrying
    /// "0:", and a ranking that starts somewhere other than 0.
    #[test]
    fn ranked_source_color_absent() {
        assert_eq!(super::ranked_source_color(""), None);
        assert_eq!(super::ranked_source_color("Ranked colors:\n"), None);
        assert_eq!(
            super::ranked_source_color("[2026-08-14T10:00:00.1Z INFO x] Opening image\n"),
            None
        );
        assert_eq!(
            super::ranked_source_color("[x] Ranked colors:\n[x] 1: #908a61\n[x] 0: #648db8\n"),
            None
        );
    }
}
