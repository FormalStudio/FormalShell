//! The Information panel's static facts, read once each time the panel
//! opens: the files are small, but cpuinfo and DMI still go through a
//! child off the UI thread like every other source.

use std::time::Duration;

use fs_info::about;

use crate::runtime::Ctx;
use crate::services::proc;
use crate::store;

pub async fn run(ctx: Ctx) {
    let out = proc::capture(&about::collect_command(), Duration::from_secs(5)).await;
    ctx.publish(store::Diff::Info(super::Diff::About(about::parse(&out.stdout))));
    futures_lite::future::pending::<()>().await;
}
