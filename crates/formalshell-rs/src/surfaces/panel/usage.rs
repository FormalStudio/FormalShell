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
use crate::ui::{El, Event, Ink, w};

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
        let mut parts = Vec::new();
        if let Some(top) = peak(u) {
            parts.push(w::hero(
                s,
                w::Hero {
                    glyph: "gauge".into(),
                    title: "Usage".into(),
                    meta: top.label.clone(),
                    readout: format!("{}%", (top.percent * 100.0).round()),
                    trailing: None,
                    rail: Some(top.percent),
                    rail_on: None,
                },
            ));
        } else if !u.claude_enabled && !u.codex_enabled {
            parts.push(w::panel_empty(s, "Usage disabled"));
        }

        let now = now_ms() as f64;
        let rows = |rows: &[UsageRow], first: usize| -> Vec<El> {
            rows.iter()
                .enumerate()
                .map(|(i, r)| {
                    let mut values = Vec::new();
                    if !r.resets_at.is_empty() {
                        values.push(w::caption(format_reset(now, &r.resets_at)).mono());
                    }
                    let figure = w::panel_value(format!("{}%", (r.percent * 100.0).round()));
                    values.push(if r.percent >= 0.9 { figure.ink(Ink::Destructive) } else { figure });
                    w::panel_meter(s, None, &r.label, values, Some(w::track(r.percent))).stop(format!("row{}", first + i))
                })
                .collect()
        };

        if u.claude_enabled {
            let title = if u.claude_tier.is_empty() { "Claude".to_owned() } else { format!("Claude / {}", u.claude_tier) };
            let empty = if u.claude == Claude::Ok { "No data".to_owned() } else { claude_hint(u) };
            parts.push(w::panel_section(s, &title, rows(claude_rows(u), 0), &empty));
        }
        if u.codex_enabled {
            let title = if u.codex_tier.is_empty() { "Codex".to_owned() } else { format!("Codex / {}", u.codex_tier) };
            let empty = if u.codex == Codex::Ok { "No data" } else { codex_hint(u) };
            parts.push(w::panel_section(s, &title, rows(codex_rows(u), claude_rows(u).len()), empty));
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
