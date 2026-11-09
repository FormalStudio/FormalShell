//! The readings the info cells show, one module per source. Each source
//! runs only while a cell wants it ([`wants`](super::wants)), publishes its
//! whole state as one snapshot into its own slice of the store, and
//! answers a failure with a state the cell words honestly (no gh, no auth,
//! no location), never a stale count.
//!
//! Services run on their own thread and never see the store; each poll
//! reads the keys it owns off [`config::current`](super::config::current) again whenever config
//! publishes.

pub mod about;
pub mod calendar;
pub mod geoclue;
pub mod github;
pub mod monitor;
pub mod powerflow;
pub mod procs;
pub mod tailscale;
pub mod update;
pub mod usage;
pub mod weather;

use std::sync::LazyLock;
use std::time::Duration;

use async_io::Timer;
use futures_lite::FutureExt;

use super::config::Settings;
use super::wants::{COUNT, Source};

static KICKS: LazyLock<Vec<(async_channel::Sender<()>, async_channel::Receiver<()>)>> =
    LazyLock::new(|| (0..COUNT).map(|_| async_channel::bounded(1)).collect());

/// Asks a running poll to go again now (a cell's right click).
pub fn kick(source: Source) {
    let _ = KICKS[source as usize].0.try_send(());
}

/// The poll's end of [`kick`].
pub fn kicked(source: Source) -> async_channel::Receiver<()> {
    KICKS[source as usize].1.clone()
}

/// Waits out `interval`, cutting it short when the keys a poll reads
/// changed (`read` over the new settings no longer equals `current`).
pub async fn idle<C: PartialEq>(interval: Duration, changed: &async_channel::Receiver<Settings>, current: &C, read: impl Fn() -> C) {
    let elapsed = async {
        Timer::after(interval).await;
    };
    let moved = async {
        while changed.recv().await.is_ok() {
            if read() != *current {
                return;
            }
        }
        std::future::pending::<()>().await
    };
    elapsed.or(moved).await
}
