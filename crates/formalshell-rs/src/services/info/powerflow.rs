//! What the power panel samples while it is open: the CPU package draw
//! the nix module's poller leaves in
//! `/run/formalshell/rapl`, and one sysfs pass over the supplies and USB-C
//! ports that Power/flow.js decodes. A missing poller leaves `cpu_w` empty
//! rather than 0.

use std::time::Duration;

use fs_system::power::flow::{self, Snapshot};
use fs_system::power::model::parse_rapl_mw;

use async_io::Timer;

use crate::runtime::Ctx;
use crate::services::proc;
use crate::store;

const RAPL: &str = "/run/formalshell/rapl";
const SAMPLE: Duration = Duration::from_millis(3000);

#[derive(Clone, Debug, Default, PartialEq)]
pub struct State {
    pub cpu_w: Option<f64>,
    pub snapshot: Snapshot,
}

pub async fn run(ctx: Ctx) {
    loop {
        let rapl = proc::capture(&proc::argv(&["cat", RAPL]), Duration::from_secs(5)).await;
        let cpu_w = (rapl.code == 0).then(|| parse_rapl_mw(&rapl.stdout)).flatten();
        let sysfs = proc::capture(&flow::collect_command(), Duration::from_secs(10)).await;
        let snapshot = flow::parse_snapshot(&sysfs.stdout);
        ctx.publish(store::Diff::PowerFlow(State { cpu_w, snapshot }));
        Timer::after(SAMPLE).await;
    }
}
