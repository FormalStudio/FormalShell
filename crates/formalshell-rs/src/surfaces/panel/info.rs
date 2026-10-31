// Portions from Pond (BSD-3-Clause, Copyright 2026 Butterfly Interaction Inc.)
//! Pond's Information card: the machine in seven numbered rows (OS, host,
//! board, CPU, GPU, memory, disk), each a ring holding its number, a dim
//! label and the value, hairlines between. The Manual button under them
//! opens USAGE.md, and the footer signs the card: an FS monogram on the
//! wallpaper's colour, the name over its version badge, a field of dots
//! with a wave lit through it, and the distro's own mark and `NAME`. The static facts come from one read per open
//! (`Source::About`), the GPU, memory and disk from the system monitor; a
//! fact the machine does not report says so instead of a guess.

use fs_info::about;

use super::{Effect, Panel, View};
use crate::services::compositor;
use crate::services::wants::{Source, Want};
use crate::store::Topic;
use crate::ui::{El, Event, Ink, Size, Type, Weight, What, w};

const MANUAL: &str = "https://github.com/FormalStudio/FormalShell/blob/main/docs/USAGE.md";

pub struct Info {
    _about: Want,
    _monitor: Want,
}

impl Info {
    pub fn new() -> Self {
        Self { _about: Want::new(Source::About), _monitor: Want::new(Source::Monitor) }
    }
}

/// A value, or why there is none: still reading, or not reported.
enum Fact {
    Reading,
    Missing,
    Value(String),
}

fn fact(read: bool, value: Option<String>) -> Fact {
    match (read, value) {
        (_, Some(v)) => Fact::Value(v),
        (false, None) => Fact::Reading,
        (true, None) => Fact::Missing,
    }
}

fn facts(v: &View) -> [(&'static str, Fact); 7] {
    let a = v.store.info.about.as_ref();
    let m = &v.store.info.monitor;
    let read = a.is_some();
    let sampled = m.sampled_ms.is_some();
    let field = |f: fn(&about::About) -> Option<String>| fact(read, a.and_then(f));
    let mut cards: Vec<&_> = m.cards.iter().collect();
    cards.sort_by_key(|c| !c.discrete);
    let gpu = (!cards.is_empty()).then(|| cards.iter().map(|c| c.name.as_str()).collect::<Vec<_>>().join(", "));
    let memory = m.mem.as_ref().map(|x| about::used_of(x.total_bytes - x.available_bytes, x.total_bytes));
    let disk = m.disk.iter().find(|d| d.mount == "/").map(|d| about::used_of(d.used, d.size));
    [
        ("OS", field(|a| a.os.clone())),
        ("Host", field(|a| a.host.clone())),
        ("Board", field(|a| a.board.clone())),
        ("CPU", field(|a| a.cpu.clone())),
        ("GPU", fact(sampled, gpu)),
        ("Memory", fact(sampled, memory)),
        ("Disk", fact(sampled, disk)),
    ]
}

impl Panel for Info {
    fn id(&self) -> &'static str {
        "info"
    }

    fn title(&self, _: &View) -> String {
        "Information".into()
    }

    fn reads(&self) -> &'static [Topic] {
        &[Topic::Info]
    }

    fn width(&self, v: &View) -> f64 {
        v.theme.space.popup_width_menu
    }

    fn body(&self, v: &View) -> El {
        let s = &v.theme.space;
        let mut rows = Vec::new();
        for (i, (label, f)) in facts(v).into_iter().enumerate() {
            if i > 0 {
                rows.push(w::separator());
            }
            let value = match f {
                Fact::Value(text) => w::text(text).size(Type::Title).weight(Weight::Medium).elide().fill(),
                Fact::Reading => w::text("Reading").ink(Ink::Muted).fill(),
                Fact::Missing => w::text("Not reported").ink(Ink::Muted).fill(),
            };
            let line = w::row(s.xl, vec![w::ordinal(i + 1), w::label(label).ink(Ink::Dim).width(Size::Px(s.huge * 2.0)), value]);
            rows.push(line.fill().height(s.huge + s.lg));
        }
        let manual = w::cell(w::row(s.md, vec![w::icon("book-open"), w::label("Manual").weight(Weight::Medium).fill()]).fill().height(s.huge))
            .cell_state(|c| {
                c.group = true;
                c.inset = Some((s.lg, 0.0));
            })
            .interactive()
            .stop("manual")
            .on("manual");
        w::panel_text(s, w::column(s.lg, vec![w::column(0.0, rows), manual, footer(v).pad_top(s.sm)]))
    }

    fn event(&mut self, ev: &Event, fx: &mut Effect) {
        if let ("manual", What::Click) = (ev.on.as_str(), &ev.what) {
            open_manual(fx);
        }
    }

    fn activate(&mut self, stop: &str, fx: &mut Effect) {
        if stop == "manual" {
            open_manual(fx);
        }
    }
}

/// Which row of the dot field each column lights, low to high and back.
const WAVE: [usize; 11] = [2, 2, 1, 0, 0, 1, 2, 2, 1, 0, 0];

fn footer(v: &View) -> El {
    let s = &v.theme.space;
    let side = s.panel_row * 2.0;
    let letter = |t: &str| w::text(t).size(Type::Display).weight(Weight::Bold).ink(Ink::Primary);
    let primary = v.theme.colors.get("primary");
    let pip = || w::dot(primary, s.sm);
    let monogram = w::cell(
        w::column(
            0.0,
            vec![
                w::row(0.0, vec![letter("F"), w::spacer(), pip()]).fill(),
                w::spacer(),
                w::row(0.0, vec![pip(), w::spacer(), letter("S")]).fill(),
            ],
        )
        .fill()
        .height(side - s.md * 2.0)
        .pad(s.md, s.md, s.md, s.md),
    )
    .cell_state(|c| {
        c.glyph = true;
        c.open = true;
        c.inset = Some((0.0, 0.0));
    })
    .width(Size::Px(side))
    .height(side);

    let badge = w::cell(w::value(format!("v{}", env!("CARGO_PKG_VERSION")))).cell_state(|c| {
        c.chip = true;
        c.small = true;
    });
    let name = w::column(s.xs, vec![w::text("FormalShell").size(Type::Title).weight(Weight::Semibold).elide(), w::row(0.0, vec![badge])]);

    let off = v.theme.colors.get("border");
    let field = w::column(
        s.sm,
        (0..3)
            .map(|r| w::row(s.sm, WAVE.iter().map(|&lit| w::dot(if lit == r { primary } else { off }, s.sm)).collect()))
            .collect(),
    );
    let mut parts = vec![monogram, name, w::spacer(), field, w::spacer()];

    let about = v.store.info.about.as_ref();
    if let Some(id) = about.and_then(|a| a.os_id.clone()) {
        let word = about.and_then(|a| a.os_name.clone()).unwrap_or_default();
        parts.push(w::icon(format!("distro:{id}")).size(Type::DisplayLarge).ink(Ink::Muted));
        parts.push(w::text(word).size(Type::Title).weight(Weight::Semibold).ink(Ink::Muted).elide());
    }
    w::row(s.lg, parts).fill()
}

fn open_manual(fx: &mut Effect) {
    compositor::spawn(&["xdg-open".to_owned(), MANUAL.to_owned()]);
    fx.close = true;
}
