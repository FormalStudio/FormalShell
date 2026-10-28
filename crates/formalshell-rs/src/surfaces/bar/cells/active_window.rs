//! The held-focus window's app icon and desktop-entry
//! name, then its title as the strip's second free label. The name crossfades
//! when it changes. With no desktop entry resolved the name is the app id in
//! the meta ink and there is no icon.

use crate::services::appicon;
use crate::store::Topic;
use crate::surfaces::bar::cell::{Action, Button, Cell, Env, Limits, Look, Part, View};
use crate::ui::el::Pic;

#[derive(Default)]
pub struct ActiveWindow {
    window: Option<Held>,
    probed: Option<(u32, appicon::Query)>,
}

#[derive(Clone, PartialEq)]
struct Held {
    app_id: String,
    title: String,
    /// The entry's display name and the icon its theme resolved, once the
    /// probe answered.
    name: Option<String>,
    icon: Pic,
    size: f64,
}

impl Cell for ActiveWindow {
    fn reads(&self) -> &'static [Topic] {
        &[Topic::Compositor, Topic::AppIcon]
    }

    fn read(&mut self, env: &Env) -> bool {
        let h = &env.store.compositor;
        let held = h.held_focused_window_id();
        let Some(w) = h.window(&held) else {
            let changed = self.window.is_some();
            self.window = None;
            return changed;
        };
        let size = super::now_playing::slot(env);
        let query = appicon::Query {
            id: format!("bar-active:{}", w.id),
            app_id: w.app_id.clone(),
            initial_class: w.initial_class.clone(),
            initial_title: w.initial_title.clone(),
            pid: w.pid,
        };
        let (edge, scale) = (size.round() as u32, crate::scene::raster_scale());
        let px = crate::scene::pixels_at(f64::from(edge), scale);
        if self.probed.as_ref().is_none_or(|(s, q)| *s != px || *q != query) {
            appicon::probe(edge, scale, vec![query.clone()]);
            self.probed = Some((px, query.clone()));
        }
        let resolved = env.store.appicon.get(&query.id, edge, scale);
        let resolved = resolved.as_ref();
        let next = Held {
            app_id: w.app_id.clone(),
            title: w.title.clone(),
            name: resolved.and_then(|i| i.entry.as_ref()).map(|e| if e.name.is_empty() { w.app_id.clone() } else { e.name.clone() }),
            icon: Pic(resolved.and_then(|i| i.image.clone())),
            size,
        };
        let changed = self.window.as_ref() != Some(&next);
        self.window = Some(next);
        changed
    }

    fn view(&self, look: &Look) -> View {
        let Some(w) = &self.window else { return View::hidden() };
        let mut parts = Vec::new();
        if w.icon.0.is_some() {
            parts.push(Part::AppIcon { image: w.icon.clone(), size: w.size });
        }
        parts.push(Part::Name { text: w.name.clone().unwrap_or_else(|| w.app_id.clone()), max: look.narrow / 2.0, dim: w.name.is_none() });
        parts.push(Part::Free { text: w.title.clone(), dim: w.name.is_some(), lead: look.md, ceiling: f64::INFINITY, cross: false });
        View::new(parts, look.xs)
    }

    fn limits(&self, look: &Look, _along: f64, rest: f64) -> Option<Limits> {
        Some(Limits { cap: (look.narrow - rest).max(0.0), min: look.cell_width * 2.0 })
    }

    fn click(&mut self, _: Button, _: (f64, f64), _: &Env) -> Action {
        Action::Panel("appmenu")
    }
}
