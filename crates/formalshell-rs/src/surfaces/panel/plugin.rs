//! A command plugin's card (PluginPanel.qml and PluginOverlay.qml): the
//! title and width come from its manifest and the body from the rows its
//! process last printed. A plugin that has printed nothing yet is Loading,
//! one that exited is the dim PLUGIN ERROR with its reason. Activating a
//! row, by Enter, Space or a click, sends `{"event":"activate","id":...}`
//! to the process, with `checked` on a toggle row. The overlay kind draws
//! the same body on a modal card (`wayland::plugin_overlay`).

use std::collections::HashSet;
use std::sync::{LazyLock, Mutex};

use fs_chrome::plugins::{Kind, Plugin, Width};
use fs_theme::tokens::Space;

use super::{Effect, Panel, View};
use crate::services::plugins::{self, Row, RowKind, Run};
use crate::store::{Store, Topic};
use crate::ui::{El, Event, Ink, w};

/// What a plugin surface answers to on the `panel` target.
pub const PREFIX: &str = "plugin:";

/// The glyph of a plugin's header; the manifest has no key for one.
const ICON: &str = "puzzle";

/// Panel names are `&'static str` end to end, so a plugin's is leaked once
/// and reused: one short string per plugin id ever seen.
pub fn intern(name: &str) -> &'static str {
    static NAMES: LazyLock<Mutex<HashSet<&'static str>>> = LazyLock::new(Mutex::default);
    let mut names = NAMES.lock().expect("plugin names");
    if let Some(known) = names.get(name) {
        return known;
    }
    let leaked: &'static str = Box::leak(name.to_owned().into_boxed_str());
    names.insert(leaked);
    leaked
}

/// The panel or overlay plugin a `panel` name asks for, when the scan found
/// one under that id.
pub fn find<'a>(store: &'a Store, name: &str) -> Option<&'a Plugin> {
    let id = name.strip_prefix(PREFIX)?;
    store.plugins.resolved.by_id(id).filter(|p| matches!(p.kind, Kind::Panel | Kind::Overlay))
}

pub fn width(s: &Space, plugin: Option<&Plugin>) -> f64 {
    match plugin.and_then(|p| p.width) {
        Some(Width::Narrow) => s.popup_width_narrow,
        Some(Width::Wide) => s.popup_width_wide,
        Some(Width::Menu) => s.popup_width_menu,
        _ => s.popup_width_default,
    }
}

fn stop(row: &Row) -> String {
    format!("row:{}", row.id)
}

/// The rows as a body. Every row but a label is one keyboard stop and one
/// click target, both named `row:<id>`.
pub fn body(s: &Space, run: Option<&Run>) -> El {
    let label = |text: &str| w::section_label(s, text, None, true);
    let rows = match run {
        None => return w::column(s.section_gap, vec![label("Loading")]),
        Some(Run::Failed(reason)) => {
            return w::column(s.row_gap, vec![label("PLUGIN ERROR"), w::caption(reason.clone()).ink(Ink::Dim).pad_start(s.control_padding_x)]);
        }
        Some(Run::Live(display)) => &display.rows,
    };
    if rows.is_empty() {
        return w::column(s.section_gap, vec![label("No rows")]);
    }
    let els = rows
        .iter()
        .map(|r| match r.kind {
            RowKind::Label => label(&r.text),
            RowKind::Button => {
                let button = if r.icon.is_empty() { w::button(r.text.clone()) } else { w::icon_text_button(r.icon.clone(), r.text.clone()) };
                w::row(0.0, vec![button.stop(stop(r)).on(stop(r))])
            }
            RowKind::Row | RowKind::Toggle => {
                let mut trailing = Vec::new();
                if r.kind == RowKind::Toggle {
                    trailing.push(w::switch(r.checked));
                } else if !r.detail.is_empty() {
                    trailing.push(w::value(r.detail.clone()).ink(Ink::Dim));
                }
                w::list_row(s, &r.icon, &r.text, trailing).stop(stop(r)).on(stop(r))
            }
        })
        .collect();
    w::column(0.0, els)
}

/// The keyboard stops of what `run` shows, in reading order.
pub fn stops(run: Option<&Run>) -> Vec<String> {
    match run {
        Some(Run::Live(d)) => d.rows.iter().filter(|r| r.kind != RowKind::Label).map(stop).collect(),
        _ => Vec::new(),
    }
}

/// Tells plugin `id` that the row behind `stop` was activated.
pub fn activate(store: &Store, id: &str, stop: &str) {
    let Some(Run::Live(display)) = store.plugins.runs.get(id) else { return };
    let Some(row) = stop.strip_prefix("row:").and_then(|row| display.rows.iter().find(|r| r.id == row && r.kind != RowKind::Label)) else {
        return;
    };
    let checked = (row.kind == RowKind::Toggle).then_some(!row.checked);
    plugins::send(id, plugins::activate_event(&row.id, checked));
}

pub struct PluginPanel {
    key: &'static str,
    id: String,
}

impl PluginPanel {
    pub fn new(key: &'static str) -> Self {
        Self { key, id: key.strip_prefix(PREFIX).unwrap_or(key).to_owned() }
    }
}

impl Panel for PluginPanel {
    fn id(&self) -> &'static str {
        self.key
    }

    fn title(&self, v: &View) -> String {
        find(v.store, self.key).map_or_else(|| self.id.clone(), |p| p.name.clone())
    }

    fn icon(&self, _: &View) -> String {
        ICON.into()
    }

    fn reads(&self) -> &'static [Topic] {
        &[Topic::Plugins, Topic::PluginOutput]
    }

    fn width(&self, v: &View) -> f64 {
        width(&v.theme.space, find(v.store, self.key))
    }

    fn start(&mut self, _: &mut Effect) {
        plugins::shown(&self.id, true);
    }

    fn closed(&mut self) {
        plugins::shown(&self.id, false);
    }

    fn body(&self, v: &View) -> El {
        body(&v.theme.space, v.store.plugins.runs.get(&self.id))
    }

    fn event(&mut self, ev: &Event, fx: &mut Effect) {
        activate(fx.store, &self.id, &ev.on);
    }

    fn activate(&mut self, stop: &str, fx: &mut Effect) {
        activate(fx.store, &self.id, stop);
    }
}
