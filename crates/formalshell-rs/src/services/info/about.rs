//! The Information panel's static facts, read once each time the panel
//! opens: the files are small, but they are read off the UI thread like
//! every other source.

use fs_info::about;

use crate::runtime::Ctx;
use crate::store;

pub async fn run(ctx: Ctx) {
    let about = ctx.pool().run(|| about::parse(&files())).await.unwrap_or_default();
    ctx.publish(store::Diff::About(about));
    futures_lite::future::pending::<()>().await;
}

fn files() -> about::Files {
    let cat = |path: &str| std::fs::read_to_string(path).unwrap_or_default();
    about::Files {
        os: cat("/etc/os-release"),
        host: cat("/proc/sys/kernel/hostname"),
        board: cat("/sys/class/dmi/id/board_name"),
        product: cat("/sys/class/dmi/id/product_name"),
        cpu: cat("/proc/cpuinfo"),
    }
}
