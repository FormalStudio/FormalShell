//! The things running in the background, a rail of cells
//! that each exist only while their thing runs, each with its own hover,
//! tooltip and click. The loud ones lead, Activity's ring first. A service
//! joins by adding one [`Indicator`] to [`rail`].

use std::time::{Duration, Instant};

use fs_info::reminders;
use fs_system::capture::elapsed_label;

use crate::services::notifications::now_ms;
use crate::store::Topic;
use crate::surfaces::bar::cell::{Action, Button, Cell, Env, Look, Part, Tone, View};

#[derive(Clone, Debug, PartialEq)]
struct Item {
    icon: &'static str,
    /// A dim countdown beside the icon.
    label: Option<String>,
    tooltip: String,
    destructive: bool,
    /// Drawn dim: the thing is winding down.
    dim: bool,
}

impl Item {
    fn new(icon: &'static str, tooltip: impl Into<String>) -> Self {
        Self { icon, label: None, tooltip: tooltip.into(), destructive: false, dim: false }
    }
}

/// One cell of the rail: the item while its thing runs, none otherwise.
pub struct Indicator {
    reads: &'static [Topic],
    item: fn(&Env) -> Option<Item>,
    /// Hover and click: a cell with nothing to
    /// do on a click still washes under the pointer.
    interactive: bool,
    click: Action,
    shown: Option<Item>,
    /// The countdown's next second, while it shows one.
    tick: Option<Instant>,
}

impl Indicator {
    fn new(reads: &'static [Topic], item: fn(&Env) -> Option<Item>, interactive: bool, click: Action) -> Self {
        Self { reads, item, interactive, click, shown: None, tick: None }
    }
}

pub fn rail() -> Vec<Box<dyn Cell>> {
    let cells = [
        Indicator::new(&[Topic::Recording], recording, true, Action::RecordStop),
        Indicator::new(&[Topic::Clipssh], clipssh, false, Action::None),
        Indicator::new(&[Topic::Media], airplay, true, Action::None),
        Indicator::new(&[Topic::Notifications], reminder, true, Action::ReminderSummary),
        Indicator::new(&[Topic::Caffeinate], |env| env.store.caffeinate.active.then(|| Item::new("coffee", "CAFFEINATE ON")), true, Action::Caffeinate(false)),
        Indicator::new(&[Topic::NightLight], |env| env.store.nightlight.active.then(|| Item::new("lightbulb", "NIGHT LIGHT ON")), true, Action::None),
        Indicator::new(&[Topic::State], |env| (!env.store.state.data.overnight.is_null()).then(|| Item::new("moon-star", "OVERNIGHT ON")), true, Action::OvernightOff),
    ];
    let mut rail: Vec<Box<dyn Cell>> = vec![Box::new(super::activity::Activity::default())];
    rail.extend(cells.into_iter().map(|c| Box::new(c) as Box<dyn Cell>));
    rail
}

fn recording(env: &Env) -> Option<Item> {
    let rec = &env.store.recording;
    rec.active.then(|| Item {
        destructive: true,
        dim: rec.stopping,
        ..Item::new("circle-dot", format!("RECORDING {}", elapsed_label(rec.elapsed_ms as f64)))
    })
}

fn clipssh(env: &Env) -> Option<Item> {
    let target = &env.store.clipssh.target;
    (!target.is_empty()).then(|| Item::new("terminal", format!("SENDING CLIPBOARD IMAGE TO {target}")))
}

fn airplay(env: &Env) -> Option<Item> {
    let airplay = &env.store.media.airplay;
    let from = if airplay.client.is_empty() { String::new() } else { format!(" FROM {}", airplay.client) };
    airplay.active.then(|| Item::new("airplay", format!("AIRPLAY{from}")))
}

fn reminder(env: &Env) -> Option<Item> {
    let pending = &env.store.notifications.reminders;
    let first = pending.first()?;
    let label = reminders::bar_label(pending, now_ms() as f64);
    Some(Item { label: Some(label.clone()), ..Item::new("alarm-clock", format!("{} / {label}", first.message)) })
}

impl Cell for Indicator {
    fn reads(&self) -> &'static [Topic] {
        self.reads
    }

    fn read(&mut self, env: &Env) -> bool {
        let item = (self.item)(env);
        // An unrelated change must not slide the countdown's next second away.
        let now = Instant::now();
        self.tick = match (item.as_ref().is_some_and(|i| i.label.is_some()), self.tick) {
            (false, _) => None,
            (true, Some(due)) if due > now => Some(due),
            (true, _) => Some(now + Duration::from_secs(1)),
        };
        let changed = item != self.shown;
        self.shown = item;
        changed
    }

    fn view(&self, look: &Look) -> View {
        let Some(item) = &self.shown else { return View::hidden() };
        let mut parts = vec![Part::Icon { name: item.icon.into(), dim: item.dim, dot: false }];
        if let Some(label) = &item.label {
            parts.push(Part::DimLabel { text: label.clone() });
        }
        let mut view = View::new(parts, look.xxs).tooltip(item.tooltip.clone());
        if item.destructive {
            view = view.tone(Tone::Destructive);
        }
        view.interactive = self.interactive;
        view
    }

    fn wake(&self) -> Option<Instant> {
        self.tick
    }

    fn click(&mut self, _: Button, _: (f64, f64), _: &Env) -> Action {
        self.click.clone()
    }
}
