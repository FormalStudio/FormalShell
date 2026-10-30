//! The focused app's menu, opened from the active-window
//! cell. The window's desktop entry supplies the Actions rows (its own
//! `[Desktop Action]` groups) and the compositor's window list filtered by
//! the same app id supplies the Windows rows, the focused one carrying a
//! check. Nothing focused is NO WINDOW, an app id with no entry NO DESKTOP
//! ENTRY and an entry with no actions NO ACTIONS, never an invented row.
//!
//! The cursor walks the action rows, then the window rows, then the close
//! row, and Enter does what a click on that row does.

use super::{Effect, Panel, View};
use crate::services::appicon::{self, Action, Entry};
use crate::services::compositor::{self, model::Window};
use crate::store::{Store, Topic};
use crate::ui::{El, Event, Ink, Weight, w};

/// The probe's key in the appicon slice: the workspaces cell asks for the
/// same windows at its own size under their ids.
const PROBE: &str = "appmenu";

#[derive(Default)]
pub struct AppMenu;

struct Subject {
    window: Window,
    entry: Option<Entry>,
    image: Option<crate::scene::Bitmap>,
    windows: Vec<Window>,
}

/// Held focus, not raw: opening this panel takes keyboard focus off the
/// very window it describes.
fn subject(store: &Store) -> Option<Subject> {
    let h = &store.compositor;
    let window = h.window(&h.held_focused_window_id())?.clone();
    let icon = store.appicon.get(PROBE, probe_size(store), crate::scene::raster_scale());
    let icon = icon.as_ref();
    let windows = h.snapshot.windows.iter().filter(|x| !window.app_id.is_empty() && x.app_id == window.app_id).cloned().collect();
    Some(Subject { entry: icon.and_then(|i| i.entry.clone()), image: icon.and_then(|i| i.image.clone()), window, windows })
}

/// The hero picture's edge.
fn probe_size(store: &Store) -> u32 {
    (store.theme.theme.space.lg * 2.0).round() as u32
}

/// Proc.appLaunch: its own scope under uwsm, the command itself outside a
/// uwsm session.
fn launch(store: &Store, entry: &Entry, action: &Action) {
    let argv = if std::env::var_os("UWSM_FINALIZE_VARNAMES").is_some_and(|v| !v.is_empty()) && !action.command.is_empty() {
        ["uwsm", "app", "--"].iter().map(|s| (*s).to_owned()).chain(action.command.iter().cloned()).collect()
    } else {
        action.command.clone()
    };
    if !argv.is_empty() {
        crate::services::launch::spawn(store, &entry.id, &argv);
    }
}

impl Panel for AppMenu {
    fn id(&self) -> &'static str {
        "appmenu"
    }

    fn title(&self, _: &View) -> String {
        "App menu".into()
    }

    fn icon(&self, _: &View) -> String {
        "menu".into()
    }

    fn reads(&self) -> &'static [Topic] {
        &[Topic::Compositor, Topic::AppIcon]
    }

    fn start(&mut self, fx: &mut Effect) {
        let h = &fx.store.compositor;
        let Some(win) = h.window(&h.held_focused_window_id()) else { return };
        appicon::probe(
            probe_size(fx.store),
            crate::scene::raster_scale(),
            vec![appicon::Query {
                id: PROBE.into(),
                app_id: win.app_id.clone(),
                initial_class: win.initial_class.clone(),
                initial_title: win.initial_title.clone(),
                pid: win.pid,
            }],
        );
    }

    fn body(&self, v: &View) -> El {
        let s = &v.theme.space;
        let Some(sub) = subject(v.store) else {
            return w::column(s.section_gap, vec![w::section_label(s, "No window", None, true)]);
        };
        let actions: &[Action] = sub.entry.as_ref().map_or(&[], |e| &e.actions);
        let name = sub.entry.as_ref().map_or_else(|| sub.window.app_id.clone(), |e| e.name.clone());
        let hero = w::hero_with(
            s,
            w::Hero {
                glyph: String::new(),
                title: name,
                meta: if sub.entry.is_some() { sub.window.app_id.clone() } else { String::new() },
                readout: String::new(),
                trailing: None,
                rail: None,
                rail_on: None,
            },
            true,
            crate::ui::Type::Display,
            Some(w::picture(sub.image.clone(), s.lg * 2.0)),
        );

        let mut action_rows: Vec<El> = Vec::new();
        let mut action_col = vec![w::heading(s, "Actions", Some(actions.len()), true)];
        if sub.entry.is_none() {
            action_col.push(w::section_label(s, "No desktop entry", None, true));
        } else if actions.is_empty() {
            action_col.push(w::section_label(s, "No actions", None, true));
        }
        for (i, a) in actions.iter().enumerate() {
            action_rows.push(w::cell(w::label(&a.name).elide()).ghost().interactive().stop(format!("action:{i}")).on(format!("action:{i}")));
        }
        action_col.push(w::column(0.0, action_rows));

        let window_rows: Vec<El> = sub
            .windows
            .iter()
            .enumerate()
            .map(|(i, win)| {
                let current = win.id == sub.window.id;
                let title = if win.title.is_empty() { &win.app_id } else { &win.title };
                let mut parts = vec![w::label(title).elide()];
                if current {
                    parts.push(w::icon("check").ink(Ink::Primary));
                }
                w::cell(w::row(s.icon_gap, parts).fill())
                    .ghost()
                    .selected(current)
                    .interactive()
                    .stop(format!("window:{i}"))
                    .on(format!("window:{i}"))
            })
            .collect();
        let windows = w::column(
            s.row_gap,
            vec![w::heading(s, "Windows", Some(sub.windows.len()), true), w::column(0.0, window_rows)],
        );
        let close = w::cell(w::row(s.icon_gap, vec![w::icon("x"), w::label("Close window").weight(Weight::Medium).elide()]).fill())
            .ghost()
            .interactive()
            .stop("close-window")
            .on("close-window");

        w::column(
            s.section_gap,
            vec![hero, w::column(s.row_gap, action_col), w::column(s.row_gap, vec![windows, w::separator(), close])],
        )
    }

    fn event(&mut self, ev: &Event, fx: &mut Effect) {
        self.activate(&ev.on, fx);
    }

    fn activate(&mut self, stop: &str, fx: &mut Effect) {
        let Some(sub) = subject(fx.store) else { return };
        if let Some(i) = stop.strip_prefix("action:").and_then(|i| i.parse::<usize>().ok()) {
            if let Some(entry) = &sub.entry
                && let Some(a) = entry.actions.get(i)
            {
                launch(fx.store, entry, a);
                fx.close = true;
            }
        } else if let Some(i) = stop.strip_prefix("window:").and_then(|i| i.parse::<usize>().ok()) {
            if let Some(win) = sub.windows.get(i) {
                compositor::focus_window(&win.id);
                fx.close = true;
            }
        } else if stop == "close-window" {
            compositor::close_window(&sub.window.id);
            fx.close = true;
        }
    }
}
