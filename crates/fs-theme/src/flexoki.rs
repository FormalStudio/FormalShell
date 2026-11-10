//! Flexoki (stephango.com/flexoki, github.com/kepano/flexoki) as data, and
//! the three views the shell renders it through: theme.json's shadcn roles,
//! the Material roles every matugen template is written against, and the
//! base16 slots a terminal palette is built from.
//!
//! One table, because a Flexoki-pinned wallpaper has to reach three sinks that
//! must agree. theme.json and the Hyprland colours files take the shadcn
//! view directly; every template (the shell's GTK/Qt ones and the user's own)
//! is written against matugen's role names, so a pinned run rewrites those
//! names to [`material_roles`] values before matugen ever sees the file.
//! Without that rewrite matugen renders them from a Material scheme grown out
//! of one blue seed, and a terminal whose ANSI slots read
//! primary/secondary/tertiary comes out monochrome blue.
//!
//! Ramp values are the canonical palette
//! (github.com/kepano/flexoki/tree/main/tailwind), cross-checked against
//! Ghostty's bundled Flexoki Light/Dark. Flexoki's own rule: the 600 stops sit
//! on light backgrounds, the 400 stops on dark ones.

use serde_json::{Map, Value};

pub const BASE: [(&str, &str); 15] = [
    ("paper", "#fffcf0"),
    ("black", "#100f0f"),
    ("50", "#f2f0e5"),
    ("100", "#e6e4d9"),
    ("150", "#dad8ce"),
    ("200", "#cecdc3"),
    ("300", "#b7b5ac"),
    ("400", "#9f9d96"),
    ("500", "#878580"),
    ("600", "#6f6e69"),
    ("700", "#575653"),
    ("800", "#403e3c"),
    ("850", "#343331"),
    ("900", "#282726"),
    ("950", "#1c1b1a"),
];

pub const HUES: [(&str, [(&str, &str); 13]); 8] = [
    (
        "red",
        [
            ("50", "#ffe1d5"),
            ("100", "#ffcabb"),
            ("150", "#fdb2a2"),
            ("200", "#f89a8a"),
            ("300", "#e8705f"),
            ("400", "#d14d41"),
            ("500", "#c03e35"),
            ("600", "#af3029"),
            ("700", "#942822"),
            ("800", "#6c201c"),
            ("850", "#551b18"),
            ("900", "#3e1715"),
            ("950", "#261312"),
        ],
    ),
    (
        "orange",
        [
            ("50", "#ffe7ce"),
            ("100", "#fed3af"),
            ("150", "#fcc192"),
            ("200", "#f9ae77"),
            ("300", "#ec8b49"),
            ("400", "#da702c"),
            ("500", "#cb6120"),
            ("600", "#bc5215"),
            ("700", "#9d4310"),
            ("800", "#71320d"),
            ("850", "#59290d"),
            ("900", "#40200d"),
            ("950", "#27180e"),
        ],
    ),
    (
        "yellow",
        [
            ("50", "#faeec6"),
            ("100", "#f6e2a0"),
            ("150", "#f1d67e"),
            ("200", "#eccb60"),
            ("300", "#dfb431"),
            ("400", "#d0a215"),
            ("500", "#be9207"),
            ("600", "#ad8301"),
            ("700", "#8e6b01"),
            ("800", "#664d01"),
            ("850", "#503d02"),
            ("900", "#3a2d04"),
            ("950", "#241e08"),
        ],
    ),
    (
        "green",
        [
            ("50", "#edeecf"),
            ("100", "#dde2b2"),
            ("150", "#cdd597"),
            ("200", "#bec97e"),
            ("300", "#a0af54"),
            ("400", "#879a39"),
            ("500", "#768d21"),
            ("600", "#66800b"),
            ("700", "#536907"),
            ("800", "#3d4c07"),
            ("850", "#313d07"),
            ("900", "#252d09"),
            ("950", "#1a1e0c"),
        ],
    ),
    (
        "cyan",
        [
            ("50", "#ddf1e4"),
            ("100", "#bfe8d9"),
            ("150", "#a2dece"),
            ("200", "#87d3c3"),
            ("300", "#5abdac"),
            ("400", "#3aa99f"),
            ("500", "#2f968d"),
            ("600", "#24837b"),
            ("700", "#1c6c66"),
            ("800", "#164f4a"),
            ("850", "#143f3c"),
            ("900", "#122f2c"),
            ("950", "#101f1d"),
        ],
    ),
    (
        "blue",
        [
            ("50", "#e1eceb"),
            ("100", "#c6dde8"),
            ("150", "#abcfe2"),
            ("200", "#92bfdb"),
            ("300", "#66a0c8"),
            ("400", "#4385be"),
            ("500", "#3171b2"),
            ("600", "#205ea6"),
            ("700", "#1a4f8c"),
            ("800", "#163b66"),
            ("850", "#133051"),
            ("900", "#12253b"),
            ("950", "#101a24"),
        ],
    ),
    (
        "purple",
        [
            ("50", "#f0eaec"),
            ("100", "#e2d9e9"),
            ("150", "#d3cae6"),
            ("200", "#c4b9e0"),
            ("300", "#a699d0"),
            ("400", "#8b7ec8"),
            ("500", "#735eb5"),
            ("600", "#5e409d"),
            ("700", "#4f3685"),
            ("800", "#3c2a62"),
            ("850", "#31234e"),
            ("900", "#261c39"),
            ("950", "#1a1623"),
        ],
    ),
    (
        "magenta",
        [
            ("50", "#fee4e5"),
            ("100", "#fccfda"),
            ("150", "#f9b9cf"),
            ("200", "#f4a4c2"),
            ("300", "#e47da8"),
            ("400", "#ce5d97"),
            ("500", "#b74583"),
            ("600", "#a02f6f"),
            ("700", "#87285e"),
            ("800", "#641f46"),
            ("850", "#4f1b39"),
            ("900", "#39172b"),
            ("950", "#24131d"),
        ],
    ),
];

pub const HUE_NAMES: [&str; 8] = [
    "red", "orange", "yellow", "green", "cyan", "blue", "purple", "magenta",
];

/// The colour Flexoki's own site links in, and what a pinned run hands
/// `matugen color hex` in place of the image. Every `{{colors.*}}` a template
/// asks for is rewritten before matugen runs, so this only seeds the roles no
/// rewrite covers (a custom_colors entry the user declared himself).
pub const SOURCE: &str = "#4385be";

/// A key of [`BASE`]; panics on a name the table lacks.
pub fn base(step: &str) -> &'static str {
    BASE.iter()
        .find(|(k, _)| *k == step)
        .map(|(_, v)| *v)
        .unwrap_or_else(|| panic!("flexoki: no base tone {step}"))
}

/// A stop of one of [`HUES`]; panics on a hue or step the table lacks.
pub fn hue(name: &str, step: &str) -> &'static str {
    HUES.iter()
        .find(|(n, _)| *n == name)
        .and_then(|(_, stops)| stops.iter().find(|(k, _)| *k == step))
        .map(|(_, v)| *v)
        .unwrap_or_else(|| panic!("flexoki: no {name} stop {step}"))
}

fn is_dark(mode: &str) -> bool {
    mode != "light"
}

pub(crate) fn from_pairs(pairs: &[(&str, &str)]) -> Map<String, Value> {
    pairs
        .iter()
        .map(|(k, v)| ((*k).to_string(), Value::String((*v).to_string())))
        .collect()
}

/// The mode's own accent stop. Flexoki's terminal ports spend this on ANSI
/// 1-6 and [`alt_stop`] on ANSI 9-14.
pub fn stop(hue_name: &str, mode: &str) -> &'static str {
    hue(hue_name, if is_dark(mode) { "400" } else { "600" })
}

/// The stop the other mode uses.
pub fn alt_stop(hue_name: &str, mode: &str) -> &'static str {
    hue(hue_name, if is_dark(mode) { "600" } else { "400" })
}

/// theme.json's shadcn roles. Base tones fill the surfaces (black/b950/b900
/// dark, paper/b50/b100 light), blue is `primary` and `ring`, and chart1..5
/// walk the accents since this palette has real ones to draw from.
pub fn shadcn(mode: &str) -> Map<String, Value> {
    let paper = base("paper");
    let black = base("black");
    if !is_dark(mode) {
        return from_pairs(&[
            ("mode", "light"),
            ("background", paper),
            ("foreground", black),
            ("card", base("50")),
            ("cardForeground", black),
            ("popover", base("100")),
            ("popoverForeground", black),
            ("primary", hue("blue", "600")),
            ("primaryForeground", paper),
            ("secondary", base("100")),
            ("secondaryForeground", black),
            ("muted", base("100")),
            ("mutedForeground", base("600")),
            ("accent", base("150")),
            ("accentForeground", black),
            ("destructive", hue("red", "600")),
            ("destructiveForeground", paper),
            ("warning", hue("orange", "600")),
            ("warningForeground", paper),
            ("border", base("150")),
            ("input", base("150")),
            ("ring", hue("blue", "600")),
            ("chart1", hue("blue", "600")),
            ("chart2", hue("cyan", "600")),
            ("chart3", hue("orange", "600")),
            ("chart4", hue("green", "600")),
            ("chart5", hue("purple", "600")),
        ]);
    }
    from_pairs(&[
        ("mode", "dark"),
        ("background", black),
        ("foreground", base("200")),
        ("card", base("950")),
        ("cardForeground", base("200")),
        ("popover", base("900")),
        ("popoverForeground", base("200")),
        ("primary", hue("blue", "400")),
        ("primaryForeground", black),
        ("secondary", base("900")),
        ("secondaryForeground", base("200")),
        ("muted", base("900")),
        ("mutedForeground", base("500")),
        ("accent", base("850")),
        ("accentForeground", base("200")),
        ("destructive", hue("red", "400")),
        ("destructiveForeground", black),
        ("warning", hue("orange", "400")),
        ("warningForeground", black),
        ("border", base("800")),
        ("input", base("800")),
        ("ring", hue("blue", "400")),
        ("chart1", hue("blue", "400")),
        ("chart2", hue("cyan", "400")),
        ("chart3", hue("orange", "400")),
        ("chart4", hue("green", "400")),
        ("chart5", hue("purple", "400")),
    ])
}

/// Flexoki's own base16 view (the mapping its bat/fish/tmTheme ports use).
/// base06 is absent from the upstream table, so it takes the step between
/// base05 and base07 rather than being invented.
pub fn base16(mode: &str) -> Map<String, Value> {
    let dark = is_dark(mode);
    let pick = |d: &str, l: &str| if dark { base(d) } else { base(l) };
    let mut out = from_pairs(&[
        ("base00", pick("black", "paper")),
        ("base01", pick("950", "50")),
        ("base02", pick("900", "100")),
        ("base03", pick("700", "300")),
        ("base04", pick("500", "600")),
        ("base05", pick("200", "black")),
        ("base06", pick("150", "950")),
        ("base07", pick("100", "900")),
    ]);
    let names = [
        "base08", "base09", "base0A", "base0B", "base0C", "base0D", "base0E", "base0F",
    ];
    for (slot, name) in HUE_NAMES.iter().zip(names) {
        out.insert(
            name.to_string(),
            Value::String(stop(slot, mode).to_string()),
        );
    }
    out
}

/// Every role matugen emits under `colors.*` (matugen 4.1.0, `-j hex` on a
/// bare config lists 50 including source_color), each carrying a real Flexoki
/// tone. The accents follow the shadcn view so a template and the shell never
/// disagree: blue is primary, cyan secondary, orange tertiary (the warning
/// colour), red error. Containers take the hue's 900 stop on dark and its 100
/// stop on light; the `*_fixed` family is mode-independent by definition, so
/// both modes get the same pair.
///
/// Eight hue names ride along past matugen's own list (red, orange, yellow,
/// green, cyan, blue, purple, magenta, and a `_alt` twin each). Material has
/// no green or yellow role, so a terminal template that wants a real ANSI
/// ramp has nowhere else to read one from; declaring the same names under
/// `[config.custom_colors]` in ~/.config/matugen/config.toml gets the same
/// template a wallpaper-harmonised ramp on every other wallpaper.
pub fn material_roles(mode: &str) -> Map<String, Value> {
    let dark = is_dark(mode);
    let container = if dark { "900" } else { "100" };
    let on_container = if dark { "150" } else { "800" };
    let paper = base("paper");
    let black = base("black");
    let on_accent = if dark { black } else { paper };
    let pick = |d: &str, l: &str| if dark { base(d) } else { base(l) };

    let mut roles = Map::new();
    let mut put = |k: &str, v: &str| {
        roles.insert(k.to_string(), Value::String(v.to_string()));
    };

    put("source_color", SOURCE);
    put("surface_tint", stop("blue", mode));

    for (role, accent) in [
        ("primary", "blue"),
        ("secondary", "cyan"),
        ("tertiary", "orange"),
    ] {
        put(role, stop(accent, mode));
        put(&format!("on_{role}"), on_accent);
        put(&format!("{role}_container"), hue(accent, container));
        put(&format!("on_{role}_container"), hue(accent, on_container));
        if role == "primary" {
            put("inverse_primary", alt_stop(accent, mode));
        }
        put(&format!("{role}_fixed"), hue(accent, "150"));
        put(&format!("{role}_fixed_dim"), hue(accent, "400"));
        put(&format!("on_{role}_fixed"), hue(accent, "950"));
        put(&format!("on_{role}_fixed_variant"), hue(accent, "700"));
    }

    put("error", stop("red", mode));
    put("on_error", on_accent);
    put("error_container", hue("red", container));
    put("on_error_container", hue("red", on_container));

    put("background", pick("black", "paper"));
    put("on_background", pick("200", "black"));
    put("surface", pick("black", "paper"));
    put("on_surface", pick("200", "black"));
    put("surface_dim", pick("black", "100"));
    put("surface_bright", pick("800", "paper"));
    put("surface_container_lowest", pick("black", "paper"));
    put("surface_container_low", pick("950", "50"));
    put("surface_container", pick("900", "100"));
    put("surface_container_high", pick("850", "150"));
    put("surface_container_highest", pick("800", "200"));
    put("surface_variant", pick("900", "100"));
    put("on_surface_variant", pick("500", "600"));
    put("inverse_surface", pick("200", "black"));
    put("inverse_on_surface", pick("black", "paper"));

    put("outline", pick("700", "300"));
    put("outline_variant", pick("800", "150"));
    put("scrim", black);
    put("shadow", black);

    for name in HUE_NAMES {
        put(name, stop(name, mode));
        put(&format!("{name}_alt"), alt_stop(name, mode));
    }
    roles
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::matugen;
    use crate::palette;
    use std::collections::HashSet;

    fn s<'a>(m: &'a Map<String, Value>, k: &str) -> &'a str {
        m.get(k)
            .and_then(Value::as_str)
            .unwrap_or_else(|| panic!("missing {k}"))
    }

    fn pin() -> &'static palette::Pin {
        palette::pinned_palette("flexoki").unwrap()
    }

    /// matugen 4.1.0's own role list, read off `matugen -j hex --dry-run
    /// color hex <x>` against a bare config. A role missing from
    /// material_roles() is a template expression a pinned run would leave on
    /// matugen's blue scheme, so the whole list is asserted rather than
    /// sampled.
    const MATUGEN_ROLES: [&str; 50] = [
        "background",
        "error",
        "error_container",
        "inverse_on_surface",
        "inverse_primary",
        "inverse_surface",
        "on_background",
        "on_error",
        "on_error_container",
        "on_primary",
        "on_primary_container",
        "on_primary_fixed",
        "on_primary_fixed_variant",
        "on_secondary",
        "on_secondary_container",
        "on_secondary_fixed",
        "on_secondary_fixed_variant",
        "on_surface",
        "on_surface_variant",
        "on_tertiary",
        "on_tertiary_container",
        "on_tertiary_fixed",
        "on_tertiary_fixed_variant",
        "outline",
        "outline_variant",
        "primary",
        "primary_container",
        "primary_fixed",
        "primary_fixed_dim",
        "scrim",
        "secondary",
        "secondary_container",
        "secondary_fixed",
        "secondary_fixed_dim",
        "shadow",
        "source_color",
        "surface",
        "surface_bright",
        "surface_container",
        "surface_container_high",
        "surface_container_highest",
        "surface_container_low",
        "surface_container_lowest",
        "surface_dim",
        "surface_tint",
        "surface_variant",
        "tertiary",
        "tertiary_container",
        "tertiary_fixed",
        "tertiary_fixed_dim",
    ];

    fn is_hex(v: &str) -> bool {
        v.len() == 7
            && v.starts_with('#')
            && v[1..]
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    }

    #[test]
    fn material_roles_cover_matugen() {
        for mode in ["dark", "light"] {
            let roles = material_roles(mode);
            for role in MATUGEN_ROLES {
                let v = roles.get(role).and_then(Value::as_str);
                assert!(v.is_some(), "{mode} is missing {role}");
                assert!(is_hex(v.unwrap()), "{mode}.{role} is not a hex");
            }
        }
    }

    /// Every value has to come off the ramp, or a pinned run ships a tone
    /// Flexoki does not have.
    #[test]
    fn every_value_is_on_the_ramp() {
        let mut known: HashSet<&str> = HashSet::new();
        for (_, v) in BASE {
            known.insert(v);
        }
        for (_, stops) in HUES {
            for (_, v) in stops {
                known.insert(v);
            }
        }
        for mode in ["dark", "light"] {
            for (role, v) in material_roles(mode) {
                let v = v.as_str().unwrap();
                assert!(known.contains(v), "{mode}.{role} is off-ramp: {v}");
            }
            for (slot, v) in base16(mode) {
                let v = v.as_str().unwrap();
                assert!(known.contains(v), "{mode}.{slot} is off-ramp: {v}");
            }
        }
    }

    #[test]
    fn source_is_blue_400() {
        assert_eq!(SOURCE, hue("blue", "400"));
    }

    /// Material has no green or yellow, so a terminal template reads its ANSI
    /// ramp off these eight instead. The `_alt` twin is the other mode's stop,
    /// which is what Flexoki's own terminal ports spend on ANSI 9-14.
    #[test]
    fn hue_roles() {
        let dark = material_roles("dark");
        let light = material_roles("light");
        assert_eq!(s(&dark, "green"), hue("green", "400"));
        assert_eq!(s(&dark, "green_alt"), hue("green", "600"));
        assert_eq!(s(&light, "green"), hue("green", "600"));
        assert_eq!(s(&light, "green_alt"), hue("green", "400"));
        assert_eq!(s(&dark, "yellow"), hue("yellow", "400"));
    }

    /// The shell's own palette and the templates read one table: primary is
    /// blue, destructive red, warning orange, in both directions.
    #[test]
    fn shadcn_matches_material() {
        for mode in ["dark", "light"] {
            let sh = shadcn(mode);
            let roles = material_roles(mode);
            assert_eq!(s(&sh, "primary"), s(&roles, "primary"));
            assert_eq!(s(&sh, "background"), s(&roles, "surface"));
            assert_eq!(s(&sh, "foreground"), s(&roles, "on_surface"));
            assert_eq!(s(&sh, "destructive"), s(&roles, "error"));
            assert_eq!(s(&sh, "warning"), s(&roles, "tertiary"));
            assert_eq!(s(&sh, "mutedForeground"), s(&roles, "on_surface_variant"));
            assert_eq!(s(&sh, "border"), s(&roles, "outline_variant"));
        }
        let pin = palette::pinned_palette("/walls/Flexoki-dune.png").unwrap();
        assert_eq!(pin.name, "flexoki");
        assert_eq!(
            s(&pin.shadcn("dark"), "primary"),
            s(&shadcn("dark"), "primary")
        );
        assert_eq!(pin.source, "4385BE");
        assert!(palette::pinned_palette("/walls/dune.png").is_none());
    }

    /// Ghostty's bundled Flexoki Dark/Light, slot for slot. A terminal
    /// template reads its 16 off these roles, so this pins the roles rather
    /// than the template: the mode's own hue stop for ANSI 1-6, the other
    /// mode's for 9-14, and the dark scheme for black and bright white in
    /// both modes.
    #[test]
    fn ansi_ramp_matches_the_flexoki_port() {
        let hues = ["red", "green", "yellow", "blue", "magenta", "cyan"];
        let expected: [(&str, [&str; 16]); 2] = [
            (
                "dark",
                [
                    "#100f0f", "#d14d41", "#879a39", "#d0a215", "#4385be", "#ce5d97", "#3aa99f",
                    "#878580", "#575653", "#af3029", "#66800b", "#ad8301", "#205ea6", "#a02f6f",
                    "#24837b", "#cecdc3",
                ],
            ),
            (
                "light",
                [
                    "#100f0f", "#af3029", "#66800b", "#ad8301", "#205ea6", "#a02f6f", "#24837b",
                    "#6f6e69", "#b7b5ac", "#d14d41", "#879a39", "#d0a215", "#4385be", "#ce5d97",
                    "#3aa99f", "#cecdc3",
                ],
            ),
        ];
        for (mode, want) in expected {
            let roles = material_roles(mode);
            let dark = material_roles("dark");
            let mut ramp: Vec<&str> = vec![s(&dark, "surface")];
            for h in hues {
                ramp.push(s(&roles, h));
            }
            ramp.push(s(&roles, "on_surface_variant"));
            ramp.push(s(&roles, "outline"));
            for h in hues {
                ramp.push(s(&roles, &format!("{h}_alt")));
            }
            ramp.push(s(&dark, "on_surface"));
            for i in 0..16 {
                assert_eq!(ramp[i], want[i], "{mode} ANSI {i}");
            }
        }
    }

    #[test]
    fn substitute_formats() {
        let out = matugen::substitute_pinned(
            "a={{colors.surface.default.hex}}\n\
             b={{ colors.primary.default.hex_stripped }}\n\
             c={{colors.error.default.rgb}}\n\
             d={{colors.surface.default.rgba}}\n\
             e={{base16.base0B.default.hex}}\n",
            "dark",
            pin(),
        );
        assert_eq!(out.substituted, 5);
        assert_eq!(out.skipped.len(), 0);
        assert!(out.text.contains("a=#100f0f"));
        assert!(out.text.contains("b=4385be"));
        assert!(out.text.contains("c=rgb(209, 77, 65)"));
        assert!(out.text.contains("d=rgba(16, 15, 15, 1)"));
        // base0B is green in Flexoki's own base16, which is the whole point:
        // a terminal reading it gets green where matugen's scheme has none.
        assert!(out.text.contains("e=#879a39"));
    }

    #[test]
    fn substitute_leaves_matugens_own_keywords() {
        let src = "m={{mode}} i={{image}} u={{colors.made_up.default.hex}}\n";
        let out = matugen::substitute_pinned(src, "dark", pin());
        assert_eq!(out.text, src);
        assert_eq!(out.substituted, 0);
        assert_eq!(out.skipped.len(), 1);
    }

    /// matugen 4.1.0 rejects a colour filter on a bare string, so a filtered
    /// .hex goes through to_color and anything else keeps matugen's value
    /// rather than rendering in the wrong syntax.
    #[test]
    fn substitute_filters() {
        let out = matugen::substitute_pinned(
            "a={{ colors.primary.default.hex | set_lightness: -20.0 }}\n\
             b={{ colors.primary.default.rgb | set_lightness: -20.0 }}\n",
            "dark",
            pin(),
        );
        assert!(
            out.text
                .contains("a={{ \"#4385be\" | to_color | set_lightness: -20.0 }}")
        );
        assert!(
            out.text
                .contains("b={{ colors.primary.default.rgb | set_lightness: -20.0 }}")
        );
        assert_eq!(out.skipped.len(), 1);
    }

    #[test]
    fn scheme_selects_the_table() {
        let out = matugen::substitute_pinned(
            "d={{colors.primary.dark.hex}} l={{colors.primary.light.hex}} \
             x={{colors.primary.default.hex}}",
            "light",
            pin(),
        );
        assert!(out.text.contains("d=#4385be"));
        assert!(out.text.contains("l=#205ea6"));
        assert!(out.text.contains("x=#205ea6"));
    }

    #[test]
    fn template_inputs_round_trip() {
        let cfg = "[templates.a]\ninput_path = '~/.config/matugen/templates/a.tmpl'\n\
                   output_path = '~/a'\n[templates.b]\ninput_path = \"/abs/b.tmpl\"\n";
        let inputs = matugen::template_inputs(cfg);
        assert_eq!(inputs.len(), 2);
        assert_eq!(
            matugen::expand_home(&inputs[0], "/home/u"),
            "/home/u/.config/matugen/templates/a.tmpl"
        );
        assert_eq!(matugen::expand_home(&inputs[1], "/home/u"), "/abs/b.tmpl");
        let out = matugen::rewrite_template_inputs(cfg, |_, index| {
            (index == 0).then(|| "/state/0.tmpl".to_string())
        });
        assert!(out.contains("input_path = '/state/0.tmpl'"));
        assert!(out.contains("input_path = \"/abs/b.tmpl\""));
        // output_path is matugen's to resolve, never repointed.
        assert!(out.contains("output_path = '~/a'"));
    }
}
