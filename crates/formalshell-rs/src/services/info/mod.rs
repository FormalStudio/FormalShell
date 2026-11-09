//! The readings the info cells show, one module per source. Each source
//! runs only while a cell wants it ([`wants`](super::wants)), publishes its
//! whole state as one snapshot into its own slice of the store, and
//! answers a failure with a state the cell words honestly (no gh, no auth,
//! no location), never a stale count.
//!
//! Services run on their own thread and never see the store, so the UI
//! thread hands them settings.json through [`configure`], and each poll
//! reads the keys it owns again whenever that changes.

pub mod about;
pub mod calendar;
pub mod geoclue;
pub mod github;
pub mod iphone;
pub mod monitor;
pub mod powerflow;
pub mod procs;
pub mod tailscale;
pub mod update;
pub mod usage;
pub mod weather;

use std::sync::{Arc, LazyLock, Mutex};
use std::time::Duration;

use async_io::Timer;
use futures_lite::FutureExt;
use serde_json::Value;

use super::wants::{COUNT, Source};

static SETTINGS: Mutex<Option<Arc<Value>>> = Mutex::new(None);
static LISTENERS: Mutex<Vec<async_channel::Sender<()>>> = Mutex::new(Vec::new());

/// The settings the polls read, replaced on every config change.
pub fn configure(settings: &Value) {
    *SETTINGS.lock().unwrap() = Some(Arc::new(settings.clone()));
    LISTENERS.lock().unwrap().retain(|tx| {
        let _ = tx.try_send(());
        !tx.is_closed()
    });
}

/// Whether settings.json has been handed over yet.
pub fn configured() -> bool {
    SETTINGS.lock().unwrap().is_some()
}

/// Fires after each [`configure`].
pub fn changed() -> async_channel::Receiver<()> {
    let (tx, rx) = async_channel::bounded(1);
    LISTENERS.lock().unwrap().push(tx);
    rx
}

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

pub struct Settings(Arc<Value>);

pub fn settings() -> Settings {
    Settings(SETTINGS.lock().unwrap().clone().unwrap_or_else(|| Arc::new(Value::Null)))
}

impl Settings {
    pub fn get(&self, path: &str) -> Option<&Value> {
        let mut node = &*self.0;
        for key in path.split('.') {
            node = node.get(key)?;
        }
        Some(node)
    }

    pub fn str(&self, path: &str) -> Option<&str> {
        self.get(path)?.as_str()
    }

    pub fn f64(&self, path: &str) -> Option<f64> {
        self.get(path)?.as_f64()
    }

    /// `default` unless the key is exactly `false` or `true`.
    pub fn flag(&self, path: &str, default: bool) -> bool {
        self.get(path).and_then(Value::as_bool).unwrap_or(default)
    }

    /// A poll interval: the key when it is a positive number, else `default_ms`.
    pub fn interval(&self, path: &str, default_ms: u64) -> Duration {
        let ms = self.f64(path).filter(|n| *n > 0.0).map_or(default_ms, |n| n as u64);
        Duration::from_millis(ms)
    }
}

/// Waits out `interval`, cutting it short when the keys a poll reads
/// changed (`read` over the new settings no longer equals `current`).
pub async fn idle<C: PartialEq>(interval: Duration, changed: &async_channel::Receiver<()>, current: &C, read: impl Fn() -> C) {
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

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn settings_read_dotted_keys_with_defaults() {
        let s = Settings(Arc::new(json!({"github": {"intervalMs": 1000}, "weather": {"intervalMs": 0}, "usage": {"claude": false}})));
        assert_eq!(s.interval("github.intervalMs", 300_000), Duration::from_millis(1000));
        assert_eq!(s.interval("weather.intervalMs", 900_000), Duration::from_millis(900_000));
        assert_eq!(s.interval("tailscale.intervalMs", 60_000), Duration::from_millis(60_000));
        assert!(!s.flag("usage.claude", true));
        assert!(s.flag("usage.codex", true));
    }
}
