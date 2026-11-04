//! The boundary of its region's governed group, which the quick settings
//! card it opens shows as info tiles. Points away from the bar while that
//! card is shut and back at it while it is up.

use fs_chrome::bar::layout::{self, Entry};
use fs_chrome::types::{Edge, Region};

use crate::store::Topic;
use crate::surfaces::bar::cell::{Action, Button, Cell, Env, Look, View};

pub struct Chevron {
    region: Region,
    count: usize,
    edge: Edge,
    pub open: bool,
}

impl Chevron {
    pub fn new(region: Region, entries: &[Entry]) -> Self {
        Self { region, count: layout::collapsed_names(entries).len(), edge: Edge::Top, open: false }
    }
}

impl Cell for Chevron {
    fn reads(&self) -> &'static [Topic] {
        &[]
    }

    fn read(&mut self, env: &Env) -> bool {
        let changed = env.edge != self.edge;
        self.edge = env.edge;
        changed
    }

    fn view(&self, look: &Look) -> View {
        let (away, back) = match self.edge {
            Edge::Bottom => ("chevron-up", "chevron-down"),
            Edge::Left => ("chevron-right", "chevron-left"),
            Edge::Right => ("chevron-left", "chevron-right"),
            Edge::Top => ("chevron-down", "chevron-up"),
        };
        View::icon(if self.open { back } else { away }, look).tooltip(format!("BAR / {} ITEMS", self.count))
    }

    fn click(&mut self, _: Button, _: (f64, f64), _: &Env) -> Action {
        Action::Overflow(self.region)
    }

    fn set_open(&mut self, open: bool) -> bool {
        let changed = open != self.open;
        self.open = open;
        changed
    }
}
