//! The Information panel's static facts, read once each time the panel
//! opens: the files are small, but they are read off the UI thread like
//! every other source.

use fs_info::about;

use crate::runtime::Ctx;
use crate::store;

pub async fn run(ctx: Ctx) {
    let about = ctx.pool().run(|| about::parse(&about::Files::read())).await.unwrap_or_default();
    ctx.publish(store::Diff::About(about));
    futures_lite::future::pending::<()>().await;
}
