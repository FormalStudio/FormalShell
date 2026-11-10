//! An activity icon and "C42% M63% G10%" in the dim ink
//! (CPU, memory and, where a card reports a busy figure, GPU). The panel
//! and launcher view open from the click; the figures only run while this
//! cell is on a bar.

use fs_chrome::bar::panels::PanelId;
use fs_system::monitor::format::pct;
use fs_types::Fraction;

use crate::services::wants::{Source, Want};
use crate::store::Topic;
use crate::surfaces::bar::cell::{Action, Button, Cell, Env, Part, Summary, View};
use crate::paint::Look;

pub struct Monitor {
    want: Want,
    cpu: Option<Fraction>,
    mem: Option<Fraction>,
    gpu: Option<Fraction>,
    label: bool,
}

impl Default for Monitor {
    fn default() -> Self {
        Self { want: Want::new(Source::Monitor), cpu: None, mem: None, gpu: None, label: false }
    }
}

/// The icon the strip warms ahead of the first frame.
pub const ICONS: &[&str] = &["activity"];

impl Cell for Monitor {
    fn visible(&mut self, on: bool) {
        self.want.hold(on);
    }

    fn reads(&self) -> &'static [Topic] {
        &[Topic::Monitor]
    }

    fn read(&mut self, env: &Env) -> bool {
        let m = &env.store.monitor;
        let next = (m.cpu.aggregate, m.mem.as_ref().map(|x| x.used_fraction), m.gpu_busy());
        let label = env.store.config.bool("bar.widgets.monitor.showLabel").unwrap_or(false);
        let changed = next != (self.cpu, self.mem, self.gpu) || label != self.label;
        (self.cpu, self.mem, self.gpu) = next;
        self.label = label;
        changed
    }

    fn view(&self, look: &Look) -> View {
        let mut text = format!("C{} M{}", pct(self.cpu), pct(self.mem));
        let mut tooltip = format!("MONITOR / CPU {} / MEM {}", pct(self.cpu), pct(self.mem));
        if self.gpu.is_some() {
            text.push_str(&format!(" G{}", pct(self.gpu)));
            tooltip.push_str(&format!(" / GPU {}", pct(self.gpu)));
        }
        let mut parts = vec![
            Part::SlotIcon { name: "activity".into(), width: look.huge },
            Part::DimLabel { text },
        ];
        if self.label {
            parts.push(Part::Name { text: "Monitor".into(), max: f64::INFINITY, dim: false });
        }
        View::new(parts, look.xxs).panel(PanelId::Monitor).tooltip(tooltip)
    }

    fn summary(&self) -> Option<Summary> {
        let mut value = format!("CPU {}  Mem {}", pct(self.cpu), pct(self.mem));
        if self.gpu.is_some() {
            value.push_str(&format!("  GPU {}", pct(self.gpu)));
        }
        Some(Summary::new("activity", "Monitor", value))
    }

    fn click(&mut self, _: Button, _: (f64, f64), _: &Env) -> Action {
        Action::Panel(PanelId::Monitor)
    }
}
