//! The low-battery watcher, which runs for the shell's whole
//! lifetime. The notification
//! is a local one, so critical earns the DND bypass.

use fs_info::notifications::Urgency;
use fs_system::power::model::{Fired, Low, warn_event};
use fs_types::Percent;
use fs_upower::DeviceState;

use crate::services::devices::power;
use crate::store::Store;

#[derive(Default)]
pub struct Watch {
    last: Option<Percent>,
    fired: Fired,
}

impl Watch {
    /// Reads the battery once; true when it raised a notification.
    pub fn check(&mut self, store: &mut Store) -> bool {
        let Some(b) = store.power.battery.clone() else { return false };
        // UPower has not populated yet: the percent can still read 0, and
        // recording it would arm the rising guard against a bogus value.
        if b.state == DeviceState::Unknown {
            return false;
        }
        let draining = matches!(b.state, DeviceState::Discharging | DeviceState::PendingDischarge | DeviceState::Empty);
        let result = warn_event(self.last, b.level, !draining, self.fired, power::thresholds(&store.config));
        self.last = Some(b.level);
        self.fired = result.fired;
        let body = format!("{}% REMAINING", b.level.rounded());
        match result.event {
            Some(Low::Warn) => store.notifications.notify("LOW BATTERY", &body, Urgency::Normal),
            Some(Low::Critical) => store.notifications.notify("CRITICAL BATTERY", &body, Urgency::Critical),
            None => return false,
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::devices::power::Battery;

    fn store(percent: f64, state: DeviceState) -> Store {
        let mut s = Store::default();
        let level = Percent::from_percent(percent);
        s.power.battery = Some(Battery { level, state, rate: 0.0, time_to_full: None, time_to_empty: None, health: None, size_wh: 0.0 });
        s
    }

    #[test]
    fn a_threshold_fires_once_and_charging_rearms_it() {
        let mut watch = Watch::default();
        assert!(!watch.check(&mut store(40.0, DeviceState::Discharging)));
        assert!(watch.check(&mut store(9.0, DeviceState::Discharging)));
        assert!(!watch.check(&mut store(8.0, DeviceState::Discharging)));
        assert!(watch.check(&mut store(4.0, DeviceState::Discharging)));
        assert!(!watch.check(&mut store(30.0, DeviceState::Charging)));
        assert!(watch.check(&mut store(9.0, DeviceState::Discharging)));
    }

    #[test]
    fn a_battery_that_has_not_reported_is_skipped() {
        let mut watch = Watch::default();
        assert!(!watch.check(&mut store(0.0, DeviceState::Unknown)));
    }
}
