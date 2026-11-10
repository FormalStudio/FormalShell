//! Whether a screen recording is running, for the bar's recording indicator.
//! `wayland::capture_host::record` owns the recorder and writes this slice.

#[derive(Default)]
pub struct State {
    pub active: bool,
    /// Milliseconds since the recording started.
    pub elapsed_ms: u64,
    /// A stop was asked for and the recorder has not exited yet.
    pub stopping: bool,
}

pub enum Diff {
    Active { active: bool, elapsed_ms: u64, stopping: bool },
}

impl State {
    pub fn apply(&mut self, diff: Diff) -> bool {
        let Diff::Active { active, elapsed_ms, stopping } = diff;
        let changed = (self.active, self.elapsed_ms, self.stopping) != (active, elapsed_ms, stopping);
        (self.active, self.elapsed_ms, self.stopping) = (active, elapsed_ms, stopping);
        changed
    }
}
