//! One page of settings (`settings:<page>`), opened from the Settings
//! panel's tiles: the catalogue's keys in sections, a switch row or a
//! label row over a segmented choice each. An edit writes an override into
//! state.json through [`overrides`]; a key settings.json sets shows its
//! value with a lock and takes no edit. Up and Down walk the rows, Enter
//! flips a switch or steps a choice round, Left and Right move a choice
//! and apply it, `x` drops the override under the cursor, Backspace goes
//! back to the page that opened it (Bar and Desktop sit under Appearance)
//! or the tiles. A page ends on rows opening the pages under it and the
//! launcher route of its own.

use fs_chrome::bar::panels::PanelId;

use super::{Effect, Nav, Out, Panel, View};
use crate::services::overrides::{self, Control, Key, Page};
use crate::store::Topic;
use crate::ui::{El, Event, Ink, On, What, w};

/// A key's row and control, its reset button, or a link row by its place
/// in [`LINKS`]: each a press and a cursor stop.
#[derive(Clone, Copy)]
enum Press {
    Key(&'static Key),
    Reset(&'static Key),
    Link(usize),
}

impl crate::ui::on::HitId for Press {}

impl std::fmt::Display for Press {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Press::Key(key) => write!(f, "key:{}", key.key),
            Press::Reset(key) => write!(f, "reset:{}", key.key),
            Press::Link(i) => match LINKS[*i].3 {
                To::Panel(panel) => write!(f, "page:{}", panel.as_str()),
                To::Route(route) => write!(f, "route:{route}"),
            },
        }
    }
}

/// Where a link row goes: a page opened in the card, or a route summoned.
#[derive(Clone, Copy, PartialEq)]
enum To {
    Panel(PanelId),
    Route(&'static str),
}

pub struct SettingsPage {
    page: &'static Page,
}

impl SettingsPage {
    pub fn new(id: PanelId) -> Option<Self> {
        let page = overrides::page(id.as_str().strip_prefix("settings:")?)?;
        Some(Self { page })
    }
}

/// The rows past each page's switches: (page, glyph, label, where to).
const LINKS: [(&str, &str, &str, To); 4] = [
    ("appearance", "panel-top", "Bar", To::Panel(PanelId::SettingsBar)),
    ("appearance", "monitor", "Desktop", To::Panel(PanelId::SettingsDesktop)),
    ("appearance", "image", "Wallpaper", To::Route("wallpaper")),
    ("system", "settings", "System actions", To::Route("system")),
];

/// A link row's value: the wallpaper's title on the picker's.
fn link_value(v: &View, to: To) -> Option<String> {
    (to == To::Route("wallpaper"))
        .then(|| crate::services::picker::title(&v.store.picker, &v.store.desktop()))
        .filter(|t| !t.is_empty())
}

/// The link rows on `page`, by their place in [`LINKS`].
fn links(page: &str) -> impl Iterator<Item = usize> + '_ {
    (0..LINKS.len()).filter(move |i| LINKS[*i].0 == page)
}

fn follow(link: usize, fx: &mut Effect) {
    match LINKS[link].3 {
        To::Panel(panel) => fx.push(Out::Panel(panel, None)),
        To::Route(route) => fx.push(Out::Summon(route)),
    }
}

fn choices(key: &Key) -> &'static [(&'static str, &'static str)] {
    match key.control {
        Control::Choice(values, _) => values,
        Control::Switch(_) => &[],
    }
}

/// The trailing mark of a row's name: the lock where settings.json pins
/// it, the reset button where an override moved it.
fn mark(v: &View, key: &'static Key) -> Option<El> {
    if v.store.config.pinned(key.key) {
        return Some(w::icon("lock").ink(Ink::Muted).tip("Set in settings.json"));
    }
    overrides::override_of(v.store, key.key)
        .map(|_| w::icon_button("rotate-ccw").tip("Reset").on(Press::Reset(key)).stop(Press::Reset(key)))
}

/// A key's row in its group; for a choice, its caption over the segmented
/// control, which sits on the card between groups, carrying the key's stop.
fn row(v: &View, key: &'static Key) -> w::Part {
    let s = &v.theme.space;
    let pinned = v.store.config.pinned(key.key);
    let shown = overrides::current(v.store, key);
    let detail = pinned.then(|| "Set in settings.json".to_owned());
    let mut trailing: Vec<El> = mark(v, key).into_iter().collect();
    match key.control {
        Control::Switch(_) => {
            trailing.push(w::switch(shown.as_bool().unwrap_or(false)).enabled(!pinned).on(Press::Key(key)));
            w::Part::Row(w::panel_row(s, "", key.label, detail, trailing).stop(Press::Key(key)))
        }
        Control::Choice(values, _) => {
            let at = values.iter().position(|(x, _)| shown.as_str() == Some(*x));
            if pinned {
                // Whatever settings.json holds, a value the catalogue does not
                // name included, shown as it reads; no control to offer.
                let raw = v.store.config.get(key.key).map(|r| r.as_str().map_or_else(|| r.to_string(), str::to_owned));
                let name = at.map(|i| values[i].1.to_owned()).or(raw).unwrap_or_default();
                trailing.push(w::label(name).ink(Ink::Muted));
                return w::Part::Row(w::panel_row(s, "", key.label, detail, trailing).stop(Press::Key(key)));
            }
            // The label over a segmented choice is a section caption with the
            // current choice at its far end (panel-language spec §6).
            let at = at.unwrap_or(0);
            let names = values.iter().map(|(_, name)| (*name).to_owned()).collect();
            let picker = w::segmented(names, at).fill().on(Press::Key(key)).stop(Press::Key(key));
            let mut label = vec![w::panel_heading_with(s, key.label, values[at].1).fill()];
            // The reset button's glyph ends on the text column, as a
            // header's last button's does.
            let end = w::text_inset(s) - (s.control_height - s.panel_mark) / 2.0;
            label.extend(trailing.into_iter().map(|t| t.pad(0.0, 0.0, end, 0.0)));
            w::Part::Loose(w::panel_control(s, w::row(s.panel_gap, label).fill(), picker))
        }
    }
}

impl Panel for SettingsPage {
    fn title(&self, _: &View) -> String {
        self.page.title.into()
    }

    fn reads(&self) -> &'static [Topic] {
        &[Topic::Config, Topic::State, Topic::Picker]
    }

    fn actions(&self, _: &View) -> Vec<El> {
        vec![w::icon_button("chevron-left").tip("All settings").on("back").stop("back")]
    }

    fn cursor_start(&self) -> Option<On> {
        overrides::keys(self.page.id).next().map(|key| Press::Key(key).into())
    }

    fn body(&self, v: &View) -> El {
        let s = &v.theme.space;
        let mut out: Vec<El> = Vec::new();
        let mut sections: Vec<(&str, Vec<w::Part>)> = Vec::new();
        for key in overrides::keys(self.page.id) {
            if sections.last().is_none_or(|(name, _)| *name != key.section) {
                sections.push((key.section, Vec::new()));
            }
            if let Some((_, parts)) = sections.last_mut() {
                parts.push(row(v, key));
            }
        }
        out.extend(sections.into_iter().map(|(name, parts)| w::panel_runs(s, Some(w::panel_heading(s, name)), parts)));
        let rows: Vec<w::Part> = links(self.page.id)
            .map(|i| {
                let (_, glyph, label, to) = LINKS[i];
                let mut trailing: Vec<El> = link_value(v, to).map(|value| w::label(value).ink(Ink::Muted)).into_iter().collect();
                trailing.push(w::icon("chevron-right").ink(Ink::Muted));
                w::Part::Row(w::panel_row(s, glyph, label, None, trailing).stop(Press::Link(i)).on(Press::Link(i)))
            })
            .collect();
        if !rows.is_empty() {
            out.push(w::panel_runs(s, None, rows));
        }
        w::column(0.0, out)
    }

    fn event(&mut self, ev: &Event, fx: &mut Effect) {
        if ev.on == "back" {
            self.back(fx);
            return;
        }
        let key = match ev.on.get::<Press>() {
            Some(&Press::Link(i)) => return follow(i, fx),
            Some(&Press::Reset(key)) => return wrote(overrides::reset(fx.store, key.key)),
            Some(&Press::Key(key)) => key,
            None => return,
        };
        match (&key.control, &ev.what) {
            (Control::Choice(values, _), What::Pick(i)) => {
                if let Some((value, _)) = values.get(*i) {
                    wrote(overrides::set(fx.store, key.key, (*value).into()));
                }
            }
            (Control::Switch(_), _) => flip(key, fx),
            _ => {}
        }
    }

    fn activate(&mut self, stop: &On, fx: &mut Effect) {
        if stop == "back" {
            self.back(fx);
            return;
        }
        let key = match stop.get::<Press>() {
            Some(&Press::Link(i)) => return follow(i, fx),
            Some(&Press::Reset(key)) => return wrote(overrides::reset(fx.store, key.key)),
            Some(&Press::Key(key)) => key,
            None => return,
        };
        match key.control {
            Control::Switch(_) => flip(key, fx),
            Control::Choice(..) => step(key, 1, true, fx),
        }
    }

    fn step(&mut self, stop: &On, direction: i32, fx: &mut Effect) {
        if let Some(&Press::Key(key)) = stop.get::<Press>() {
            match key.control {
                Control::Choice(..) => step(key, direction, false, fx),
                Control::Switch(_) => {
                    let on = direction > 0;
                    if overrides::current(fx.store, key).as_bool() != Some(on) {
                        wrote(overrides::set(fx.store, key.key, on.into()));
                    }
                }
            }
        }
    }

    fn nav(&self, _: Option<&On>) -> Nav {
        Nav::STEPS
    }

    fn back(&mut self, fx: &mut Effect) {
        fx.push(Out::Panel(self.page.parent.unwrap_or(PanelId::Settings), None));
    }

    fn delete(&mut self, stop: &On, fx: &mut Effect) {
        if let Some(&(Press::Key(key) | Press::Reset(key))) = stop.get::<Press>() {
            wrote(overrides::reset(fx.store, key.key));
        }
    }
}

fn flip(key: &Key, fx: &Effect) {
    let on = overrides::current(fx.store, key).as_bool().unwrap_or(false);
    wrote(overrides::set(fx.store, key.key, (!on).into()));
}

/// The next choice along: the arrows stop at either end, Enter wraps.
fn step(key: &Key, direction: i32, wrap: bool, fx: &Effect) {
    let values = choices(key);
    let n = values.len() as i32;
    let shown = overrides::current(fx.store, key);
    let at = values.iter().position(|(x, _)| shown.as_str() == Some(*x)).unwrap_or(0) as i32;
    let next = if wrap { (at + direction).rem_euclid(n) } else { (at + direction).clamp(0, n - 1) } as usize;
    if next as i32 != at {
        wrote(overrides::set(fx.store, key.key, values[next].0.into()));
    }
}

/// A write the overrides refuse (a key settings.json pins, a value out of
/// range) is logged; the row keeps showing what holds.
fn wrote(r: Result<(), String>) {
    if let Err(e) = r {
        eprintln!("settings: {e}");
    }
}
