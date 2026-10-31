//! One page of settings (`settings:<page>`), opened from the Settings
//! panel's tiles: the catalogue's keys in sections, a switch row or a
//! label row over a segmented choice each. An edit writes an override into
//! state.json through [`overrides`]; a key settings.json sets shows its
//! value with a lock and takes no edit. Up and Down walk the rows, Enter
//! flips a switch or steps a choice round, Left and Right move a choice
//! and apply it, `x` drops the override under the cursor, Backspace goes
//! back to the tiles. A page with a launcher route of its own ends on a
//! row handing over to it.

use super::{Effect, Panel, View};
use crate::services::overrides::{self, Control, Key, Page};
use crate::store::Topic;
use crate::ui::{El, Event, Ink, What, w};

pub struct SettingsPage {
    name: &'static str,
    page: &'static Page,
}

impl SettingsPage {
    pub fn new(name: &'static str) -> Option<Self> {
        let page = overrides::page(name.strip_prefix("settings:")?)?;
        Some(Self { name, page })
    }
}

/// The launcher route past each page's switches: (page, glyph, label, route).
const LINKS: [(&str, &str, &str, &str); 2] =
    [("appearance", "palette", "Theme and wallpaper", "theme"), ("system", "settings", "System actions", "system")];

fn link(page: &str) -> Option<(&'static str, &'static str, &'static str)> {
    LINKS.iter().find(|(p, ..)| *p == page).map(|(_, glyph, label, route)| (*glyph, *label, *route))
}

fn stop(key: &Key) -> String {
    format!("key:{}", key.key)
}

fn key_of(stop: &str) -> Option<&'static Key> {
    overrides::find(stop.strip_prefix("key:").or_else(|| stop.strip_prefix("reset:"))?)
}

fn choices(key: &Key) -> &'static [(&'static str, &'static str)] {
    match key.control {
        Control::Choice(values, _) => values,
        Control::Switch(_) => &[],
    }
}

/// The trailing mark of a row's name: the lock where settings.json pins
/// it, the reset button where an override moved it.
fn mark(v: &View, key: &Key) -> Option<El> {
    if v.store.config.pinned(key.key) {
        return Some(w::icon("lock").ink(Ink::Muted).tip("Set in settings.json"));
    }
    overrides::override_of(v.store, key.key)
        .map(|_| w::icon_button("rotate-ccw").tip("Reset").on(format!("reset:{}", key.key)).stop(format!("reset:{}", key.key)))
}

fn row(v: &View, key: &Key) -> El {
    let s = &v.theme.space;
    let pinned = v.store.config.pinned(key.key);
    let shown = overrides::current(v.store, key);
    let detail = pinned.then(|| "Set in settings.json".to_owned());
    let mut trailing: Vec<El> = mark(v, key).into_iter().collect();
    match key.control {
        Control::Switch(_) => {
            trailing.push(w::switch(shown.as_bool().unwrap_or(false)).enabled(!pinned).on(stop(key)));
            w::panel_row(s, "", key.label, detail, trailing).stop(stop(key))
        }
        Control::Choice(values, _) => {
            let at = values.iter().position(|(x, _)| shown.as_str() == Some(*x));
            if pinned {
                // Whatever settings.json holds, a value the catalogue does not
                // name included, shown as it reads; no control to offer.
                let raw = v.store.config.get(key.key).map(|r| r.as_str().map_or_else(|| r.to_string(), str::to_owned));
                let name = at.map(|i| values[i].1.to_owned()).or(raw).unwrap_or_default();
                trailing.push(w::label(name).ink(Ink::Muted));
                return w::panel_row(s, "", key.label, detail, trailing).stop(stop(key));
            }
            let at = at.unwrap_or(0);
            let mut head = vec![w::label(key.label).elide().fill()];
            head.extend(trailing);
            head.push(w::label(values[at].1).ink(Ink::Muted));
            let names = values.iter().map(|(_, name)| (*name).to_owned()).collect();
            let picker = w::segmented(names, at).fill().on(stop(key));
            let body = w::column(s.panel_gap, vec![w::row(s.panel_gap, head).fill().height(s.panel_row_compact), picker]).fill();
            w::panel_cell(s, body.pad(0.0, 0.0, 0.0, s.panel_row_inset)).stop(stop(key))
        }
    }
}

impl Panel for SettingsPage {
    fn id(&self) -> &'static str {
        self.name
    }

    fn title(&self, _: &View) -> String {
        self.page.title.into()
    }

    fn reads(&self) -> &'static [Topic] {
        &[Topic::Config, Topic::State]
    }

    fn width(&self, v: &View) -> f64 {
        v.theme.space.panel_width_wide
    }

    fn actions(&self, _: &View) -> Vec<El> {
        vec![w::icon_button("chevron-left").tip("All settings").on("back").stop("back")]
    }

    fn cursor_start(&self) -> Option<String> {
        overrides::keys(self.page.id).next().map(stop)
    }

    fn body(&self, v: &View) -> El {
        let s = &v.theme.space;
        let mut sections: Vec<(&str, Vec<El>)> = Vec::new();
        for key in overrides::keys(self.page.id) {
            let el = row(v, key);
            match sections.last_mut() {
                Some((name, rows)) if *name == key.section => rows.push(el),
                _ => sections.push((key.section, vec![el])),
            }
        }
        let mut out: Vec<El> = sections.into_iter().map(|(name, rows)| w::panel_section(s, name, rows)).collect();
        if let Some((glyph, label, _)) = link(self.page.id) {
            let row = w::panel_row(s, glyph, label, None, vec![w::icon("chevron-right").ink(Ink::Muted)]).stop("route").on("route");
            out.push(w::panel_group(s, vec![row]).pad_top(s.panel_section));
        }
        w::column(0.0, out)
    }

    fn event(&mut self, ev: &Event, fx: &mut Effect) {
        if ev.on == "back" {
            fx.panel = Some("settings");
            return;
        }
        if ev.on == "route" {
            fx.summon = link(self.page.id).map(|(.., route)| route);
            return;
        }
        if let Some(key) = ev.on.strip_prefix("reset:").and_then(overrides::find) {
            let _ = overrides::reset(fx.store, key.key);
            return;
        }
        let Some(key) = key_of(&ev.on) else { return };
        match (&key.control, &ev.what) {
            (Control::Choice(values, _), What::Pick(i)) => {
                if let Some((value, _)) = values.get(*i) {
                    let _ = overrides::set(fx.store, key.key, (*value).into());
                }
            }
            (Control::Switch(_), _) => flip(key, fx),
            _ => {}
        }
    }

    fn activate(&mut self, stop: &str, fx: &mut Effect) {
        if stop == "back" {
            fx.panel = Some("settings");
            return;
        }
        if stop == "route" {
            fx.summon = link(self.page.id).map(|(.., route)| route);
            return;
        }
        let Some(key) = key_of(stop) else { return };
        if stop.starts_with("reset:") {
            let _ = overrides::reset(fx.store, key.key);
            return;
        }
        match key.control {
            Control::Switch(_) => flip(key, fx),
            Control::Choice(..) => step(key, 1, true, fx),
        }
    }

    fn step(&mut self, stop: &str, direction: i32, fx: &mut Effect) {
        if let Some(key) = key_of(stop).filter(|_| stop.starts_with("key:")) {
            match key.control {
                Control::Choice(..) => step(key, direction, false, fx),
                Control::Switch(_) => {
                    let on = direction > 0;
                    if overrides::current(fx.store, key).as_bool() != Some(on) {
                        let _ = overrides::set(fx.store, key.key, on.into());
                    }
                }
            }
        }
    }

    fn steps(&self) -> bool {
        true
    }

    fn back(&mut self, fx: &mut Effect) {
        fx.panel = Some("settings");
    }

    fn delete(&mut self, stop: &str, fx: &mut Effect) {
        if let Some(key) = key_of(stop) {
            let _ = overrides::reset(fx.store, key.key);
        }
    }
}

fn flip(key: &Key, fx: &Effect) {
    let on = overrides::current(fx.store, key).as_bool().unwrap_or(false);
    let _ = overrides::set(fx.store, key.key, (!on).into());
}

/// The next choice along: the arrows stop at either end, Enter wraps.
fn step(key: &Key, direction: i32, wrap: bool, fx: &Effect) {
    let values = choices(key);
    let n = values.len() as i32;
    let shown = overrides::current(fx.store, key);
    let at = values.iter().position(|(x, _)| shown.as_str() == Some(*x)).unwrap_or(0) as i32;
    let next = if wrap { (at + direction).rem_euclid(n) } else { (at + direction).clamp(0, n - 1) } as usize;
    if next as i32 != at {
        let _ = overrides::set(fx.store, key.key, values[next].0.into());
    }
}
