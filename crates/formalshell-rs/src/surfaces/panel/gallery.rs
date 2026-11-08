//! The dev sheet, every shared widget drawn
//! against the live theme. A panel as wide as the output, opened by the
//! `gallery` target rather than `panel`. The controls column takes the
//! place of an auth prompt sample, which lands with the lock
//! (R6), so every control a panel builds from is on the sheet.

use fs_types::{Edge, Percent};

use super::{Effect, Panel, View};
use crate::store::Topic;
use crate::ui::el::{CellState, Opt, Ring};
use crate::ui::{El, Event, Ink, Size, Type, Variant, Weight, What, w};

#[derive(Default)]
pub struct Gallery {
    switch: bool,
    pick: usize,
    segment: usize,
    level: f64,
}

impl Gallery {
    pub fn new() -> Self {
        Self { switch: true, pick: 1, segment: 0, level: 0.6 }
    }
}

impl Panel for Gallery {
    fn id(&self) -> &'static str {
        "gallery"
    }

    fn title(&self, _: &View) -> String {
        "DEV GALLERY".into()
    }

    fn reads(&self) -> &'static [Topic] {
        &[Topic::Theme]
    }

    fn width(&self, v: &View) -> f64 {
        (v.output.0 - v.theme.space.panel_padding * 2.0).round()
    }

    fn body(&self, v: &View) -> El {
        let t = v.theme;
        let s = &t.space;
        let control = |title: &str, parts: Vec<El>| w::panel_control(s, w::panel_heading(s, title), w::column(s.row_gap, parts));
        let controls = w::column(
            0.0,
            vec![
                control(
                    "Button",
                    vec![
                        w::row(
                            s.sm,
                            [Variant::Default, Variant::Outline, Variant::Ghost, Variant::Selected, Variant::Destructive]
                                .iter()
                                .zip(["Default", "Outline", "Ghost", "Selected", "Delete"])
                                .map(|(v, n)| w::button(n).variant(*v).on("noop"))
                                .collect(),
                        ),
                        w::row(
                            s.sm,
                            vec![
                                w::icon_text_button("refresh-cw", "With icon").variant(Variant::Outline).on("noop"),
                                w::button("Disabled").enabled(false),
                                w::icon_button("x").tip("IconButton").on("noop"),
                            ],
                        ),
                    ],
                ),
                control(
                    "Switch",
                    vec![w::row(
                        s.sm,
                        vec![
                            w::switch(self.switch).on("switch"),
                            w::switch(!self.switch).on("switch"),
                            w::switch(true).enabled(false),
                            w::switch(false).enabled(false),
                        ],
                    )],
                ),
                control(
                    "ButtonGroup",
                    vec![
                        w::group(
                            vec![Opt::new("Saver").icon("leaf"), Opt::new("Balanced").icon("scale"), Opt::new("Performance").icon("zap")],
                            self.pick,
                            true,
                        )
                        .on("pick"),
                    ],
                ),
                control("Segmented", vec![w::segmented(vec!["Dark".into(), "Light".into()], self.segment).on("segment")]),
                control("Track", vec![w::slider(self.level).on("level"), w::track(1.0 / 1.5).notch(1.0 / 1.5)]),
                control("Ring", vec![w::row(s.sm, vec![w::ring(Ring::Progress(0.4)), w::ring(Ring::Done), w::ring(Ring::Failed)])]),
                control(
                    "Input",
                    vec![
                        w::input("", "Placeholder", false, None),
                        w::input("formaltest", "", true, None),
                        w::input("hunter2", "", false, Some("Wrong password")),
                    ],
                ),
                control(
                    "Sparkline",
                    vec![w::sparkline(
                        (0..40).map(|i| 0.5 + 0.35 * ((i as f64) / 4.0).sin()).collect(),
                        (0..40).map(|i| 0.3 + 0.2 * ((i as f64) / 6.0).cos()).collect(),
                        1.0,
                        60,
                    )],
                ),
            ],
        )
        .fill();

        let states: [(&str, fn(&mut CellState)); 7] = [
            ("Rest", |_| {}),
            ("Hovered", |s| s.hovered = true),
            ("Cursor", |s| s.cursor = true),
            ("Selected", |s| s.selected = true),
            ("Active", |s| s.active = true),
            ("Destructive", |s| s.destructive = true),
            ("Warning", |s| s.warning = true),
        ];
        let mut cells = vec![w::panel_heading(s, "Cell")];
        cells.extend(states.iter().map(|(name, f)| w::cell(w::label(*name)).cell_state(|s| f(s))));
        let mut rows = vec![w::panel_heading(s, "Launcher row / token")];
        rows.extend([("Rest", false, false), ("Hovered", true, false), ("Selected", false, true)].into_iter().map(|(name, hovered, selected)| {
            let weight = if selected { Weight::Semibold } else { Weight::Normal };
            w::cell(w::row(s.lg, vec![w::icon("terminal").size(Type::Heading), w::text(name).weight(weight)]))
                .ghost()
                .cell_state(|c| {
                    c.row = true;
                    c.hovered = hovered;
                    c.selected = selected;
                })
                .height(fs_theme::tokens::LAUNCHER.row_height)
        }));
        rows.push(w::row(s.sm, vec![w::token(s, &["Clipboard"]), w::text("query")]).pad_start(s.lg));
        let span = s.xl * 5.0;
        let depth = s.xl * 3.0;
        let run = s.xl * 2.0;
        // A Flow in the sheet: the two along a line, then the two down one.
        let pair = |a: Edge, b: Edge| w::row(s.section_gap, vec![w::shoulders(a, span, depth, run), w::shoulders(b, span, depth, run)]).top();
        let joins = w::column(s.section_gap, vec![pair(Edge::Top, Edge::Bottom), pair(Edge::Left, Edge::Right)]);
        let cell_column = w::column(
            s.section_gap,
            vec![
                w::column(s.row_gap, cells),
                w::column(0.0, rows),
                w::panel_heading(s, "Dim / unavailable"),
                w::column(s.row_gap, vec![w::panel_heading(s, "Shoulders / joined to the bar"), joins]),
            ],
        )
        .fill();

        let f = &t.font_size;
        let mut types = vec![w::panel_heading(s, "Type scale")];
        for (name, px, ty) in [
            ("caption", f.caption, Type::Caption),
            ("bodySmall", f.body_small, Type::BodySmall),
            ("body", f.body, Type::Body),
            ("subtitle", f.subtitle, Type::Subtitle),
            ("title", f.title, Type::Title),
            ("heading", f.heading, Type::Heading),
            ("display", f.display, Type::Display),
            ("displayLarge", f.display_large, Type::DisplayLarge),
            ("hero", f.display_large * 1.5, Type::Hero),
        ] {
            let row = w::row(s.sm, vec![w::section_label(s, &format!("{name} {px}"), None, false), w::text("Aa").size(ty)]);
            types.push(w::cell(row).ghost());
        }
        let type_column = w::column(s.row_gap, types).fill();
        // A rig has no battery and no radio: these samples, built from the
        // panels' own rows, are where their populated look is read.
        let samples = w::column(
            s.row_gap,
            vec![
                w::panel_heading(s, "PowerFlow / sample snapshot, not live"),
                w::flow(super::power::flow_view(&fs_system::power::flow::sample_flow(), true)),
                w::panel_heading(s, "Battery / sample, not live"),
                w::column(0.0, vec![w::panel_text(s, super::power::battery_card(s, &battery_sample())), super::power::profiles(s, 0, 0, true)]),
                w::panel_heading(s, "Wi-Fi / sample, not live"),
                wifi_sample(s),
            ],
        )
        .width(Size::Px(s.panel_width));

        let mut seps = vec![w::panel_heading(s, "Separator")];
        seps.push(w::column(s.row_gap, vec![w::panel_heading(s, "Full bleed"), w::separator()]));
        seps.push(w::column(s.row_gap, vec![w::panel_heading(s, "Inset"), w::separator_inset(s.control_padding_x)]).pad_top(s.section_gap - s.row_gap));
        seps.push(
            w::column(s.row_gap, vec![w::panel_heading(s, "Vertical"), w::separator_vertical().pad_start(s.control_padding_x)])
                .pad_top(s.section_gap - s.row_gap),
        );
        let keycaps = w::row(
            s.md,
            vec![w::chord(s, "Enter"), w::chord_keys(s, &["Up", "Down"]), w::chord(s, "Shift+Enter"), w::chord(s, "Super+Alt+Space"), w::chord(s, "Esc")],
        )
        .pad_start(s.control_padding_x);
        let mut swatches = vec![w::panel_heading(s, "Color tokens")];
        for key in fs_theme::palette::COLOR_KEYS {
            let c = t.colors.get(key);
            let row = w::row(
                s.sm,
                vec![w::swatch(c, s.xl + s.md, f.caption + s.sm, t.radii.sm), w::section_label(s, &format!("{key} {}", c.to_hex()), None, false)],
            );
            swatches.push(w::cell(row).ghost());
        }
        let token_column = w::column(
            s.section_gap,
            vec![
                w::column(s.row_gap, seps),
                w::column(s.row_gap, vec![w::panel_heading(s, "Keycap"), keycaps]),
                w::column(s.row_gap, swatches),
                w::column(
                    s.row_gap,
                    vec![
                        w::panel_heading(s, "Metalabel"),
                        w::panel_heading(s, "Metalabel / caption"),
                        w::section_label(s, "Metalabel / subtitle", None, true).size(Type::Subtitle),
                    ],
                ),
            ],
        )
        .fill();

        let quick = w::column(s.row_gap, vec![w::panel_heading(s, "Quick settings / sample, not live"), quick_sample(s)]).width(Size::Px(s.panel_width_wide));
        let top = w::row(s.section_gap, vec![controls, cell_column, quick, samples, token_column]).fill().top();

        let mut steps = Vec::new();
        for (name, px) in [("xxs", s.xxs), ("xs", s.xs), ("sm", s.sm), ("md", s.md), ("lg", s.lg), ("xl", s.xl), ("xxl", s.xxl), ("huge", s.huge)] {
            let row = w::row(
                s.sm,
                vec![w::section_label(s, &format!("{name} {px}"), None, false), w::swatch(t.colors.get("foreground"), px, f.caption, t.radii.sm)],
            );
            steps.push(w::cell(row).ghost());
        }
        let spacing = w::column(s.row_gap, vec![w::panel_heading(s, "Spacing scale"), w::grid(3, 0.0, steps)]).fill();
        let marquee_w = (v.output.0 - s.panel_padding * 4.0 - s.section_gap * 2.0) / 3.0;
        let marquee = w::column(
            s.row_gap,
            vec![
                w::panel_heading(s, "MarqueeText"),
                w::cell(w::column(s.xxs, vec![w::section_label(s, "Fits / never moves", None, false), w::marquee("A title that fits", marquee_w)])).ghost(),
                w::cell(w::column(
                    s.xxs,
                    vec![
                        w::section_label(s, "Overflows / scrolls", None, false),
                        w::marquee("A title too long for the width it was given, which is what makes it scroll", marquee_w / 2.0),
                    ],
                ))
                .ghost(),
            ],
        )
        .fill();
        let surfaces = w::column(
            s.row_gap,
            vec![
                w::panel_heading(s, "Surfaces"),
                w::panel_heading(s, "Panel / this surface"),
                w::cell(w::section_label(s, "Tooltip / real tooltipText", None, false).ink(Ink::Fg))
                    .interactive()
                    .tip("Tooltip / the real card, loaded by this cell"),
                w::panel_heading(s, "Tooltip opens under the row above, over this panel"),
            ],
        )
        .fill();
        let bottom = w::row(s.section_gap, vec![spacing, marquee, surfaces, type_column]).fill().top();
        w::column(s.section_gap, vec![top, bottom])
    }

    fn event(&mut self, ev: &Event, _fx: &mut Effect) {
        match (ev.on.as_str(), &ev.what) {
            ("switch", What::Toggle(_)) => self.switch = !self.switch,
            ("pick", What::Pick(i)) => self.pick = *i,
            ("segment", What::Pick(i)) => self.segment = *i,
            ("level", What::Fraction(f)) => self.level = *f,
            _ => {}
        }
    }
}

/// Quick settings' pieces off, on, disabled and split, placed by the card's
/// own grid with a menu open under the first row.
fn quick_sample(s: &fs_theme::tokens::Space) -> El {
    use super::quick::grid::{self, Item, Menu};
    let tile = Item { span: 1, height: s.panel_row };
    let tiles = vec![
        (tile, w::quick_split(s, "q:wifi", "wifi", "Wi-Fi", Some("HomeNet-5G"), true, true)),
        (tile, w::quick_split(s, "q:bluetooth", "bluetooth-off", "Bluetooth", None, false, true)),
        (tile, w::quick_toggle(s, "q:dark", "moon", "Dark style", None, true, true)),
        (tile, w::quick_toggle(s, "q:dnd", "bell-off", "Do not disturb", None, false, true)),
        (tile, w::quick_split(s, "q:power", "gauge", "Power mode", Some("power-profiles-daemon is not running"), false, false)),
        (tile, w::quick_toggle(s, "q:night", "lightbulb", "Night light", Some("No gamma control"), false, false)),
        (tile, w::quick_info(s, "q:weather", "cloud-sun", "Weather", Some("14°"))),
        (tile, w::quick_info(s, "q:github", "git-branch", "GitHub", Some("12 / 15"))),
        (Item { span: grid::COLUMNS, height: s.panel_row_compact }, w::quick_slider(s, "q:volume", "volume-2", 0.6, true)),
    ];
    let menu = w::quick_menu(
        s,
        "wifi",
        "Wi-Fi",
        true,
        vec![
            w::panel_row_checked(s, "wifi", "HomeNet-5G", None, Vec::new(), true),
            w::panel_row(s, "wifi", "Office Wifi", None, vec![w::icon("lock").ink(Ink::Muted)]),
        ],
        Some(w::panel_row(s, "", "All networks", None, Vec::new()).on("q:network").stop("q:network")),
        1.0,
    );
    let items: Vec<Item> = tiles.iter().map(|(i, _)| *i).collect();
    // Only where the menu goes is read here: the tree measures its height.
    let placed = grid::place(&items, s.panel_width_wide - s.panel_row_inset * 2.0, s.panel_gap, Some(Menu { under: 0, height: 1.0 }));
    let mut rows: Vec<Vec<El>> = Vec::new();
    for ((_, el), (&row, rect)) in tiles.into_iter().zip(placed.rows.iter().zip(&placed.rects)) {
        if rows.len() <= row {
            rows.push(Vec::new());
        }
        rows[row].push(el.width(Size::Px(rect.width())));
    }
    let mut out: Vec<El> = rows.into_iter().map(|r| w::row(s.panel_gap, r).fill()).collect();
    if placed.menu.is_some() {
        out.insert(1, menu);
    }
    let buttons = ["camera", "settings", "lock", "power"].map(|g| w::quick_button(s, &format!("q:{g}"), g, g));
    let mut system = vec![w::spacer()];
    system.extend(buttons);
    out.insert(0, w::row(s.panel_gap, system).fill());
    out.push(w::quick_background(s, "q:background", "2 background tasks"));
    w::column(s.panel_gap, out).fill().pad(s.panel_row_inset, s.panel_row_inset, s.panel_row_inset, s.panel_row_inset)
}

/// Five networks with one connected, for the Wi-Fi rows' populated look
/// on a rig with no radio.
fn wifi_sample(s: &fs_theme::tokens::Space) -> El {
    let nets = [("HomeNet-5G", 90, true, true), ("Office Wifi", 70, true, false), ("Cafe Net", 50, true, false), ("Library Wifi", 40, true, false), ("GuestNet-5G", 20, false, false)];
    let rows = nets
        .iter()
        .map(|&(ssid, signal, secured, connected)| {
            let trailing = if connected {
                vec![w::button("Disconnect").variant(Variant::Ghost)]
            } else if secured {
                vec![w::icon("lock").ink(Ink::Muted)]
            } else {
                Vec::new()
            };
            w::panel_row_with(s, Some(w::panel_chip(s, super::network::signal_glyph(Percent::whole(signal).to_fraction()), connected)), w::label(ssid), None, trailing)
        })
        .collect();
    w::panel_group(s, rows)
}

/// A laptop half way through a charge cycle, for the battery card's
/// populated look on a rig with no battery.
fn battery_sample() -> super::power::Card {
    super::power::Card {
        level: fs_types::Percent::whole(49),
        state: None,
        rate: Some("8.0W".into()),
        time: Some("2h 30m left".into()),
        details: vec!["Health 94%".into(), "52.0 Wh".into(), "Limit 80%".into()],
    }
}
