// Portions from Pond (BSD-3-Clause, Copyright 2026 Butterfly Interaction Inc.)
//! Pond's Information card: the machine in seven numbered rows (OS, host,
//! board, CPU, GPU, memory, disk), each a ring holding its number, a dim
//! label and the value, hairlines between. The Manual button under them
//! opens USAGE.md, and the footer carries FormalShell's version and the
//! distro's mark. The static facts come from one read per open
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
        let os_id = v.store.info.about.as_ref().and_then(|a| a.os_id.clone());
        let mut foot = vec![
            w::text("FormalShell").weight(Weight::Bold),
            w::cell(w::value(format!("v{}", env!("CARGO_PKG_VERSION")))).cell_state(|c| {
                c.chip = true;
                c.small = true;
            }),
            w::spacer(),
        ];
        if let Some(id) = os_id {
            foot.push(w::icon(format!("distro:{id}")).size(Type::Display).ink(Ink::Muted));
        }
        w::panel_text(s, w::column(s.lg, vec![w::column(0.0, rows), manual, w::row(s.sm, foot).fill().pad_top(s.sm)]))
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

fn open_manual(fx: &mut Effect) {
    compositor::spawn(&["xdg-open".to_owned(), MANUAL.to_owned()]);
    fx.close = true;
}
