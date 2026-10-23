//! The signed-in account as the hero with the total
//! awaiting it, then Pull requests and Issues sections of rows, each the
//! repo slug in mono over the title. The cursor spans both lists, pull
//! requests first, and Enter or a click opens the row's url through
//! xdg-open and closes the panel. `gh` missing or failing is "No gh", a
//! failed login "No auth", before the first answer "Loading", and an empty
//! list a "None" row, never stale or invented rows.

use super::{Effect, Panel, View};
use crate::services::compositor;
use crate::services::info::github::{Poll, Row};
use crate::services::info::kick;
use crate::services::wants::{Source, Want};
use crate::store::Topic;
use crate::ui::{El, Event, Ink, Weight, w};

pub struct Github {
    _want: Want,
}

impl Github {
    pub fn new() -> Self {
        Self { _want: Want::new(Source::Github) }
    }
}

fn open(url: &str, fx: &mut Effect) {
    compositor::spawn(&["xdg-open".to_owned(), url.to_owned()]);
    fx.close = true;
}

impl Panel for Github {
    fn id(&self) -> &'static str {
        "github"
    }

    fn title(&self, _: &View) -> String {
        "GitHub".into()
    }

    fn icon(&self, _: &View) -> String {
        "git-branch".into()
    }

    fn subtitle(&self, _: &View) -> String {
        "Activity".into()
    }

    fn status(&self, v: &View) -> String {
        match v.store.info.github.poll {
            Poll::Unknown => "loading".into(),
            Poll::Missing | Poll::Error => "no gh".into(),
            Poll::NoAuth => "no auth".into(),
            Poll::Ok => "signed in".into(),
        }
    }

    fn accent(&self) -> super::Accent {
        super::Accent::Tertiary
    }

    fn reads(&self) -> &'static [Topic] {
        &[Topic::Info]
    }

    fn opened(&mut self) {
        kick(Source::Github);
    }

    fn body(&self, v: &View) -> El {
        let s = &v.theme.space;
        let g = &v.store.info.github;
        let label = |text: &str| w::section_label(s, text, None, true);
        let mut parts = Vec::new();
        match g.poll {
            Poll::Unknown => parts.push(label("Loading")),
            Poll::Missing | Poll::Error => parts.push(label("No gh")),
            Poll::NoAuth => parts.push(label("No auth")),
            Poll::Ok => {}
        }
        if g.poll != Poll::Ok {
            return w::column(s.section_gap, parts);
        }
        parts.push(w::hero(
            s,
            w::Hero {
                glyph: "git-branch".into(),
                title: if g.login.is_empty() { "GitHub".into() } else { g.login.clone() },
                meta: "Open items".into(),
                readout: (g.prs + g.issues).to_string(),
                trailing: None,
                rail: None,
                rail_on: None,
                tab: "open".into(),
            },
        ));
        let section = |title: &str, count: u64, rows: &[Row], first: usize| {
            let mut head = vec![w::heading(s, title, Some(count as usize), true)];
            if rows.is_empty() {
                head.push(label("None"));
            }
            let cells = rows
                .iter()
                .enumerate()
                .map(|(i, r)| {
                    let mut lines = Vec::new();
                    if !r.repo.is_empty() {
                        lines.push(w::caption(r.repo.clone()).mono().ink(Ink::Dim).elide());
                    }
                    lines.push(w::label(r.title.clone()).weight(Weight::Medium).elide());
                    w::cell(w::column(s.xxs, lines).fill()).ghost().interactive().stop(format!("row{}", first + i)).on(format!("open:{}", r.url))
                })
                .collect();
            head.push(w::column(0.0, cells));
            w::column(s.row_gap, head)
        };
        parts.push(section("Pull requests", g.prs, &g.pr_rows, 0));
        parts.push(section("Issues", g.issues, &g.issue_rows, g.pr_rows.len()));
        w::column(s.section_gap, parts)
    }

    fn event(&mut self, ev: &Event, fx: &mut Effect) {
        if let Some(url) = ev.on.strip_prefix("open:") {
            open(url, fx);
        }
    }

    fn activate(&mut self, stop: &str, fx: &mut Effect) {
        let g = &fx.store.info.github;
        let Some(i) = stop.strip_prefix("row").and_then(|i| i.parse::<usize>().ok()) else { return };
        let row = if i < g.pr_rows.len() { g.pr_rows.get(i) } else { g.issue_rows.get(i - g.pr_rows.len()) };
        if let Some(url) = row.map(|r| r.url.clone()) {
            open(&url, fx);
        }
    }
}
