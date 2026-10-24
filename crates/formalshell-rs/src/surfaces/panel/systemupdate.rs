//! Are the flake's inputs behind their upstream
//! refs. A hero naming the flake and carrying the summary, an inputs
//! section of rows (name, locked rev in mono, status that goes `warning`
//! on Behind), and a footer pairing an outline Check button with the
//! behind count. It never applies an update. The poll is the one shared
//! service the bar cell reads; this panel asks it to go again on open and
//! on Check.

use fs_info::system_update::{self, PollState};

use super::{Effect, Panel, View};
use crate::services::info::kick;
use crate::services::wants::{Source, Want};
use crate::store::Topic;
use crate::ui::{El, Event, Ink, Type, Variant, Weight, w};

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

    fn icon(&self, _: &View) -> String {
        "package".into()
    }

    fn subtitle(&self, _: &View) -> String {
        "Flake inputs".into()
    }

    fn status(&self, v: &View) -> String {
        v.store.info.update.summary().to_lowercase()
    }

    fn accent(&self) -> super::Accent {
        super::Accent::Tertiary
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
            parts.push(w::section_label(s, &summary, None, true));
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
                    tab: "flake".into(),
                },
            ));
        }

        if !u.inputs.is_empty() {
            let warning = Ink::Color(v.theme.colors.get("warning"));
            let mut head = vec![w::heading(s, "Inputs", Some(u.inputs.len()), true)];
            if ok && counts.unknown > 0 {
                head.push(w::section_label(s, &format!("{} unknown", counts.unknown), None, true));
            }
            let rows = u
                .inputs
                .iter()
                .enumerate()
                .map(|(i, input)| {
                    let status = system_update::row_status(input, &u.heads);
                    let behind = status == "Behind";
                    let row = w::row(
                        s.icon_gap,
                        vec![
                            w::label(&input.name).elide(),
                            w::caption(system_update::short_rev(&input.rev)).mono().ink(Ink::Dim),
                            w::section_label(s, status, None, false).ink(if behind { warning } else { Ink::Dim }),
                        ],
                    )
                    .fill();
                    w::cell(row).ghost().cell_state(|c| c.warning = behind).interactive().stop(format!("input{i}"))
                })
                .collect();
            head.push(w::column(0.0, rows));
            parts.push(w::column(s.row_gap, head));
        }

        let checking = u.poll == PollState::Checking;
        let figure = if ok { counts.behind.to_string() } else { "--".into() };
        parts.push(w::separator());
        parts.push(
            w::row(
                s.xs,
                vec![
                    w::icon_text_button("refresh-cw", if checking { "Checking" } else { "Check" })
                        .variant(Variant::Outline)
                        .enabled(!dir.is_empty() && !checking)
                        .stop("check")
                        .on("check"),
                    w::spacer(),
                    w::text(figure).size(Type::Display).mono().weight(Weight::Semibold),
                    w::section_label(s, "Behind", None, false),
                ],
            )
            .fill(),
        );
        w::column(s.section_gap, parts)
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
