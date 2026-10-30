//! The dev sheet, every shared widget drawn
//! against the live theme. A panel as wide as the output, opened by the
//! `gallery` target rather than `panel`. The controls column takes the
//! place of an auth prompt sample, which lands with the lock
//! (R6), so every control a panel builds from is on the sheet.

use fs_chrome::types::Edge;

use super::{Effect, Panel, View};
use crate::store::Topic;
use crate::ui::el::{CellState, Opt, Ring};
use crate::ui::{El, Event, Ink, Type, Variant, Weight, What, w};

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

fn hex(c: fs_theme::color::Rgba) -> String {
    let [r, g, b, _] = c.to_u8();
    format!("#{r:02x}{g:02x}{b:02x}")
}

impl Panel for Gallery {
    fn id(&self) -> &'static str {
        "gallery"
    }

    fn title(&self, _: &View) -> String {
        "DEV GALLERY".into()
    }

    fn icon(&self, _: &View) -> String {
        String::new()
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
        let label = |text: &str| w::heading(s, text, None, true);

        let controls = w::column(
            s.section_gap,
            vec![
                w::column(
                    s.row_gap,
                    vec![
                        label("Button"),
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
                w::column(
                    s.row_gap,
                    vec![
                        label("Switch"),
                        w::row(s.sm, vec![w::switch(self.switch).on("switch"), w::switch(!self.switch).on("switch")]),
                    ],
                ),
                w::column(
                    s.row_gap,
                    vec![
                        label("ButtonGroup"),
                        w::group(
                            vec![Opt::new("Saver").icon("leaf"), Opt::new("Balanced").icon("scale"), Opt::new("Performance").icon("zap")],
                            self.pick,
                            true,
                        )
                        .on("pick"),
                    ],
                ),
                w::column(
                    s.row_gap,
                    vec![label("Segmented"), w::segmented(vec!["Dark".into(), "Light".into()], self.segment).on("segment")],
                ),
                w::column(
                    s.row_gap,
                    vec![
                        label("Track"),
                        w::slider(self.level).on("level"),
                        w::track(1.0 / 1.5).notch(1.0 / 1.5),
                    ],
                ),
                w::column(
                    s.row_gap,
                    vec![
                        label("Ring"),
                        w::row(s.sm, vec![w::ring(Ring::Progress(0.4)), w::ring(Ring::Done), w::ring(Ring::Failed)]),
                    ],
                ),
                w::column(
                    s.row_gap,
                    vec![
                        label("Input"),
                        w::input("", "Placeholder", false, None),
                        w::input("formaltest", "", true, None),
                        w::input("hunter2", "", false, Some("Wrong password")),
                    ],
                ),
                w::column(
                    s.row_gap,
                    vec![
                        label("Sparkline"),
                        w::sparkline(
                            (0..40).map(|i| 0.5 + 0.35 * ((i as f64) / 4.0).sin()).collect(),
                            (0..40).map(|i| 0.3 + 0.2 * ((i as f64) / 6.0).cos()).collect(),
                            1.0,
                            60,
                        ),
                    ],
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
        let mut cells = vec![label("Cell")];
        cells.extend(states.iter().map(|(name, f)| w::cell(w::label(*name)).cell_state(|s| f(s))));
        let mut rows = vec![label("Launcher row / token")];
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
                label("Dim / unavailable"),
                w::column(s.row_gap, vec![label("Shoulders / joined to the bar"), joins]),
            ],
        )
        .fill();

        let f = &t.font_size;
        let mut types = vec![label("Type scale")];
        for (name, px, ty) in [
            ("caption", f.caption, Type::Caption),
            ("bodySmall", f.body_small, Type::BodySmall),
            ("body", f.body, Type::Body),
            ("subtitle", f.subtitle, Type::Subtitle),
            ("title", f.title, Type::Title),
            ("heading", f.heading, Type::Heading),
            ("display", f.display, Type::Display),
            ("displayLarge", f.display_large, Type::DisplayLarge),
        ] {
            let row = w::row(s.sm, vec![w::section_label(s, &format!("{name} {px}"), None, false), w::text("Aa").size(ty)]);
            types.push(w::cell(row).ghost());
        }
        types.push(
            w::column(
                s.row_gap,
                vec![label("PowerFlow / sample snapshot, not live"), w::flow(super::power::flow_view(&fs_system::power::flow::sample_flow(), true))],
            )
            .pad_top(s.section_gap - s.row_gap),
        );
        let type_column = w::column(s.row_gap, types).fill();

        let mut seps = vec![label("Separator")];
        seps.push(w::column(s.row_gap, vec![label("Full bleed"), w::separator()]));
        seps.push(w::column(s.row_gap, vec![label("Inset"), w::separator_inset(s.control_padding_x)]).pad_top(s.section_gap - s.row_gap));
        seps.push(
            w::column(s.row_gap, vec![label("Vertical"), w::separator_vertical().pad_start(s.control_padding_x)])
                .pad_top(s.section_gap - s.row_gap),
        );
        let keycaps = w::row(
            s.md,
            vec![w::chord(s, "Enter"), w::chord_keys(s, &["Up", "Down"]), w::chord(s, "Shift+Enter"), w::chord(s, "Super+Alt+Space"), w::chord(s, "Esc")],
        )
        .pad_start(s.control_padding_x);
        let mut swatches = vec![label("Color tokens")];
        for key in fs_theme::palette::COLOR_KEYS {
            let c = t.colors.get(key);
            let row = w::row(
                s.sm,
                vec![w::swatch(c, s.xl + s.md, f.caption + s.sm, t.radii.sm), w::section_label(s, &format!("{key} {}", hex(c)), None, false)],
            );
            swatches.push(w::cell(row).ghost());
        }
        let token_column = w::column(
            s.section_gap,
            vec![
                w::column(s.row_gap, seps),
                w::column(s.row_gap, vec![label("Keycap"), keycaps]),
                w::column(s.row_gap, swatches),
                w::column(
                    s.row_gap,
                    vec![
                        label("Metalabel"),
                        label("Metalabel / caption"),
                        w::section_label(s, "Metalabel / subtitle", None, true).size(Type::Subtitle),
                    ],
                ),
            ],
        )
        .fill();

        let top = w::row(s.section_gap, vec![controls, cell_column, type_column, token_column]).fill().top();

        let mut steps = Vec::new();
        for (name, px) in [("xxs", s.xxs), ("xs", s.xs), ("sm", s.sm), ("md", s.md), ("lg", s.lg), ("xl", s.xl), ("xxl", s.xxl), ("huge", s.huge)] {
            let row = w::row(
                s.sm,
                vec![w::section_label(s, &format!("{name} {px}"), None, false), w::swatch(t.colors.get("foreground"), px, f.caption, t.radii.sm)],
            );
            steps.push(w::cell(row).ghost());
        }
        let spacing = w::column(s.row_gap, vec![label("Spacing scale"), w::grid(3, 0.0, steps)]).fill();
        let marquee_w = (v.output.0 - s.panel_padding * 4.0 - s.section_gap * 2.0) / 3.0;
        let marquee = w::column(
            s.row_gap,
            vec![
                label("MarqueeText"),
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
                label("Surfaces"),
                label("Panel / this surface"),
                w::cell(w::section_label(s, "Tooltip / real tooltipText", None, false).ink(Ink::Fg))
                    .interactive()
                    .tip("Tooltip / the real card, loaded by this cell"),
                label("Tooltip opens under the row above, over this panel"),
            ],
        )
        .fill();
        let bottom = w::row(s.section_gap, vec![spacing, marquee, surfaces]).fill().top();
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
