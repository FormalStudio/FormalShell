//! The window closest to its limit as the hero, then a
//! Claude section and a Codex section, each a section label carrying the
//! tier over one row per rate-limit window: the name, the percentage as a
//! `display` mono figure, a track and the reset countdown. A window at or
//! past 90% takes the cell's `destructive` border and ink. The cursor walks
//! both providers' windows, Claude first, and Enter or the header's refresh
//! asks the poll to go again. A provider switched off in settings.json has
//! no section, and one that answered with no windows says "No data".

use fs_info::usage::{UsageRow, format_reset, refresh_hint};

use super::{Effect, Panel, View};
use crate::services::info::monitor::now_ms;
use crate::services::info::usage::{Claude, Codex, State};
use crate::services::info::kick;
use crate::services::wants::{Source, Want};
use crate::store::Topic;
use crate::ui::{El, Event, Ink, Weight, w};

pub struct Usage {
    _want: Want,
}

impl Usage {
    pub fn new() -> Self {
        Self { _want: Want::new(Source::Usage) }
    }
}

fn claude_rows(u: &State) -> &[UsageRow] {
    if u.claude_enabled && u.claude == Claude::Ok { u.claude_rows.as_slice() } else { &[] }
}

fn codex_rows(u: &State) -> &[UsageRow] {
    if u.codex_enabled && u.codex == Codex::Ok { u.codex_rows.as_slice() } else { &[] }
}

fn claude_hint(u: &State) -> String {
    match u.claude {
        Claude::Unknown | Claude::Loading => "Loading".into(),
        Claude::Stale => format!("Stale / {}", refresh_hint(u.refresh)),
        Claude::NoAuth => "No auth".into(),
        Claude::Error => "Error".into(),
        Claude::Ok => String::new(),
    }
}

fn codex_hint(u: &State) -> &'static str {
    match u.codex {
        Codex::Unknown | Codex::Loading => "Loading",
        Codex::Missing => "No Codex",
        Codex::Error => "Error",
        Codex::Ok => "",
    }
}

fn peak(u: &State) -> Option<&UsageRow> {
    claude_rows(u).iter().chain(codex_rows(u)).filter(|r| r.percent.is_finite()).fold(None, |best: Option<&UsageRow>, r| match best {
        Some(b) if b.percent >= r.percent => Some(b),
        _ => Some(r),
    })
}

fn refresh() {
    kick(Source::Usage);
}

impl Panel for Usage {
    fn id(&self) -> &'static str {
        "usage"
    }

    fn title(&self, _: &View) -> String {
        "Usage".into()
    }

    fn reads(&self) -> &'static [Topic] {
        &[Topic::Info]
    }

    fn actions(&self, _: &View) -> Vec<El> {
        vec![w::icon_button("refresh-cw").on("refresh")]
    }

    fn body(&self, v: &View) -> El {
        let s = &v.theme.space;
        let u = &v.store.info.usage;
        let label = |text: &str| w::panel_text(s, w::caption(text.to_owned()).ink(Ink::Muted));
        let top = peak(u);
        let meta = match top {
            Some(r) => r.label.clone(),
            None if !u.claude_enabled && !u.codex_enabled => "Disabled".into(),
            None if (u.claude_enabled && u.claude == Claude::Unknown) || (u.codex_enabled && u.codex == Codex::Unknown) => "Loading".into(),
            None => "No data".into(),
        };
        let mut parts = vec![w::hero(
            s,
            w::Hero {
                glyph: "gauge".into(),
                title: "Usage".into(),
                meta,
                readout: top.map_or_else(String::new, |r| format!("{}%", (r.percent * 100.0).round())),
                trailing: None,
                rail: top.map(|r| r.percent),
                rail_on: None,
            },
        )];

        let now = now_ms() as f64;
        let rows = |rows: &[UsageRow], first: usize| -> El {
            let cells = rows
                .iter()
                .enumerate()
                .map(|(i, r)| {
                    let mut top = vec![w::label(&r.label).weight(Weight::Medium).elide().fill()];
                    if !r.resets_at.is_empty() {
                        top.push(w::caption(format_reset(now, &r.resets_at)).mono().ink(Ink::Muted));
                    }
                    let figure = w::panel_value(format!("{}%", (r.percent * 100.0).round()));
                    top.push(if r.percent >= 0.9 { figure.ink(Ink::Destructive) } else { figure });
                    let top = w::row(s.panel_gap, top).fill().height(s.panel_row_compact).mid();
                    w::panel_cell(s, w::column(0.0, vec![top, w::track(r.percent)]).fill().pad(0.0, 0.0, 0.0, s.panel_gap))
                        .stop(format!("row{}", first + i))
                })
                .collect();
            w::column(0.0, cells)
        };

        if u.claude_enabled {
            let title = if u.claude_tier.is_empty() { "Claude".to_owned() } else { format!("Claude / {}", u.claude_tier) };
            let mut section = vec![w::panel_heading(s, &title)];
            if u.claude != Claude::Ok {
                section.push(label(&claude_hint(u)));
            } else if claude_rows(u).is_empty() {
                section.push(w::panel_empty(s, "No data"));
            }
            section.push(rows(claude_rows(u), 0));
            parts.push(w::column(s.panel_gap, section));
        }
        if u.codex_enabled {
            let title = if u.codex_tier.is_empty() { "Codex".to_owned() } else { format!("Codex / {}", u.codex_tier) };
            let mut section = vec![w::panel_heading(s, &title)];
            if u.codex != Codex::Ok {
                section.push(label(codex_hint(u)));
            } else if codex_rows(u).is_empty() {
                section.push(w::panel_empty(s, "No data"));
            }
            section.push(rows(codex_rows(u), claude_rows(u).len()));
            parts.push(w::column(s.panel_gap, section));
        }
        w::column(0.0, parts)
    }

    fn event(&mut self, ev: &Event, _: &mut Effect) {
        if ev.on == "refresh" {
            refresh();
        }
    }

    fn activate(&mut self, _: &str, _: &mut Effect) {
        refresh();
    }
}
