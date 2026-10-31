//! Are the flake's inputs behind their upstream
//! refs. A hero naming the flake and carrying the summary, an inputs
//! section of rows (name, locked rev in mono, status that goes `warning`
//! on Behind), and an outline Check button under them. It never applies an update. The poll is the one shared
//! service the bar cell reads; this panel asks it to go again on open and
//! on Check.

use fs_info::system_update::{self, PollState};

use super::{Effect, Panel, View};
use crate::services::info::kick;
use crate::services::wants::{Source, Want};
use crate::store::Topic;
use crate::ui::{El, Event, Ink, Variant, w};

pub struct SystemUpdate {
    _want: Want,
}

impl SystemUpdate {
    pub fn new() -> Self {
        Self { _want: Want::new(Source::SystemUpdate) }
    }
}

fn flake_dir<'a>(v: &'a View) -> &'a str {
    v.store.config.str("systemUpdate.flakeDir").unwrap_or("")
}

fn check() {
    kick(Source::SystemUpdate);
}

impl Panel for SystemUpdate {
    fn id(&self) -> &'static str {
        "systemupdate"
    }

    fn title(&self, _: &View) -> String {
        "System update".into()
    }

    fn reads(&self) -> &'static [Topic] {
        &[Topic::Info, Topic::Config]
    }

    fn actions(&self, v: &View) -> Vec<El> {
        vec![w::icon_button("refresh-cw").enabled(!flake_dir(v).is_empty()).on("check")]
    }

    fn opened(&mut self) {
        check();
    }

    fn body(&self, v: &View) -> El {
        let s = &v.theme.space;
        let u = &v.store.info.update;
        let dir = flake_dir(v);
        let counts = u.counts();
        let summary = u.summary();
        let ok = u.poll == PollState::Ok;
        let mut parts = Vec::new();

        if dir.is_empty() {
            parts.push(w::panel_empty(s, summary));
        } else {
            let name = dir.split('/').rfind(|p| !p.is_empty()).unwrap_or("");
            parts.push(w::hero(
                s,
                w::Hero {
                    glyph: "package".into(),
                    title: name.into(),
                    meta: summary.clone(),
                    readout: if ok { counts.behind.to_string() } else { String::new() },
                    trailing: None,
                    rail: None,
                    rail_on: None,
                },
            ));
        }

        if !u.inputs.is_empty() {
            let warning = Ink::Color(v.theme.colors.get("warning"));
            let rows = u
                .inputs
                .iter()
                .enumerate()
                .map(|(i, input)| {
                    let status = system_update::row_status(input, &u.heads);
                    let ink = if status == "Behind" { warning } else { Ink::Muted };
                    let rev = system_update::short_rev(&input.rev);
                    w::panel_row(s, "", &input.name, Some(rev), vec![w::caption(status).ink(ink)]).stop(format!("input{i}"))
                })
                .collect();
            parts.push(w::panel_section_n(s, "Inputs", u.inputs.len(), rows, ""));
            if ok && counts.unknown > 0 {
                parts.push(w::panel_note(s, format!("{} unknown", counts.unknown)));
            }
        }

        let checking = u.poll == PollState::Checking;
        let check = w::icon_text_button("refresh-cw", if checking { "Checking" } else { "Check" })
            .variant(Variant::Outline)
            .enabled(!dir.is_empty() && !checking)
            .fill()
            .height(s.panel_row)
            .stop("check")
            .on("check");
        parts.push(w::panel_text(s, w::column(0.0, vec![check]).pad_top(s.panel_section)));
        w::column(0.0, parts)
    }

    fn event(&mut self, ev: &Event, _: &mut Effect) {
        if ev.on == "check" {
            check();
        }
    }

    fn activate(&mut self, stop: &str, _: &mut Effect) {
        if stop == "check" {
            check();
        }
    }
}
