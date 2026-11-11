//! The lid: a host's `lidOutput` is off while it is shut, at the start, on
//! a reload and when the lid moves.

use std::path::PathBuf;

use shojiwm_rs::prelude::*;

use crate::engine::{Engine, enter};

const STATE: &str = "/proc/acpi/button/lid/LID0/state";
/// The core reports no lid switch, so the state file is read this often.
const POLL_MS: f64 = 1000.0;

/// `FORMALWM_LID` names the state file on a machine with no lid.
pub fn shut() -> bool {
    let path = std::env::var_os("FORMALWM_LID").map_or_else(|| PathBuf::from(STATE), PathBuf::from);
    std::fs::read_to_string(path).is_ok_and(|state| state.contains("closed"))
}

impl Engine {
    /// Follows the lid for as long as the settings name a lid output.
    pub fn watch_lid(&mut self) {
        if let Some(timer) = self.lid_timer.take() {
            timer.cancel();
        }
        if self.settings.lid_output.is_none() {
            return;
        }
        self.lid_shut = shut();
        self.lid_timer = Some(set_interval(POLL_MS, || {
            enter(|engine| {
                let shut = shut();
                if shut != engine.lid_shut {
                    engine.lid_shut = shut;
                    COMPOSITOR.output.reconfigure();
                }
            })
        }));
    }
}
