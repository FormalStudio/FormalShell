//! `bar.modules` entries of type `command` that the bar's layout places:
//! each runs its argv every `interval` ms, and what it printed resolves
//! through [`Output::resolve`]. A run past `timeout` is killed
//! and reads as an error.

use std::collections::BTreeMap;
use std::time::Duration;

use async_io::Timer;
use fs_chrome::bar::layout::EntryKind;
use fs_types::Region;
use futures_lite::{AsyncReadExt, FutureExt};
use serde_json::Value;

use super::config::{self, Settings};
use crate::runtime::Ctx;
use crate::store;

#[derive(Clone, Debug, PartialEq)]
pub struct Module {
    pub id: String,
    pub argv: Vec<String>,
    pub interval: Duration,
    pub timeout: Duration,
}

impl Module {
    /// A `bar.modules` entry.
    pub fn from_json(id: &str, module: &Value) -> Self {
        let ms = |key: &str| {
            let n = module.get(key).and_then(Value::as_f64).unwrap_or(0.0);
            Duration::from_millis(if n > 0.0 { n as u64 } else { 5000 })
        };
        let argv = module
            .get("command")
            .and_then(Value::as_array)
            .map(|a| a.iter().map(|v| v.as_str().map_or_else(|| v.to_string(), str::to_owned)).collect())
            .unwrap_or_default();
        Self { id: id.to_owned(), argv, interval: ms("interval"), timeout: ms("timeout") }
    }
}

/// What one run left the cell showing.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Output {
    pub text: String,
    pub tooltip: String,
    pub class: String,
}

impl Output {
    pub fn error() -> Self {
        Output { text: "MODULE ERROR".into(), ..Output::default() }
    }

    pub fn resolve(exit_ok: bool, raw: &str) -> Self {
        if !exit_ok {
            return Self::error();
        }
        let Ok(parsed) = serde_json::from_str::<Value>(raw) else { return Self::error() };
        let Some(text) = parsed.get("text").and_then(Value::as_str) else { return Self::error() };
        let s = |k: &str| parsed.get(k).and_then(Value::as_str).unwrap_or("").to_owned();
        Output { text: text.to_owned(), tooltip: s("tooltip"), class: s("class") }
    }
}

#[derive(Default)]
pub struct State {
    pub outputs: BTreeMap<String, Output>,
}

pub struct Diff(pub String, pub Output);

impl State {
    pub fn apply(&mut self, Diff(id, out): Diff) -> bool {
        if self.outputs.get(&id) == Some(&out) {
            return false;
        }
        self.outputs.insert(id, out);
        true
    }
}

/// The command modules the bar places, region by region. Plugins never
/// place a module, so the layout is resolved without them.
fn placed(settings: &Settings) -> Vec<Module> {
    let resolved = crate::settings::bar_layout(settings, &[]);
    Region::ALL
        .iter()
        .flat_map(|r| resolved.regions.get(*r).to_vec())
        .filter_map(|e| match &e.kind {
            EntryKind::Module { id, module } if module.get("type").and_then(Value::as_str) == Some("command") => Some(Module::from_json(id, module)),
            _ => None,
        })
        .collect()
}

async fn once(module: &Module) -> Output {
    if module.argv.is_empty() {
        return Output::error();
    }
    let child = crate::services::proc::command(&module.argv[0])
        .args(&module.argv[1..])
        .stdin(async_process::Stdio::null())
        .stdout(async_process::Stdio::piped())
        .stderr(async_process::Stdio::null())
        .kill_on_drop(true)
        .spawn();
    let Ok(mut child) = child else { return Output::error() };
    let mut stdout = child.stdout.take();
    let run = async {
        let mut raw = String::new();
        if let Some(out) = stdout.as_mut() {
            let _ = out.read_to_string(&mut raw).await;
        }
        let status = child.status().await;
        Some(Output::resolve(status.map(|s| s.success()).unwrap_or(false), &raw))
    };
    let timeout = async {
        Timer::after(module.timeout).await;
        None
    };
    run.or(timeout).await.unwrap_or_else(Output::error)
}

async fn poll(ctx: Ctx, module: Module, generation: async_channel::Receiver<()>) {
    loop {
        let out = once(&module).await;
        if generation.is_closed() {
            return;
        }
        ctx.publish(store::Diff::Commands(Diff(module.id.clone(), out)));
        let stop = async {
            let _ = generation.recv().await;
            true
        };
        if Timer::after(module.interval).map(|_| false).or(stop).await {
            return;
        }
    }
}

pub fn start(ctx: &Ctx) {
    ctx.spawn(run(ctx.clone()));
}

async fn run(ctx: Ctx) {
    let settings = config::subscribe();
    let mut current: Vec<Module> = Vec::new();
    let mut stop: Option<async_channel::Sender<()>> = None;
    while let Some(next) = config::moved(&settings, &current, placed).await {
        let modules = placed(&next);
        // Dropping the sender closes the receiver every poll holds.
        stop.take();
        let (tx, gen_rx) = async_channel::bounded(1);
        for module in &modules {
            ctx.spawn(poll(ctx.clone(), module.clone(), gen_rx.clone()));
        }
        stop = Some(tx);
        current = modules;
    }
}

trait MapFuture: std::future::Future + Sized {
    fn map<T>(self, f: impl FnOnce(Self::Output) -> T) -> impl std::future::Future<Output = T> {
        async move { f(self.await) }
    }
}

impl<F: std::future::Future> MapFuture for F {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_rules() {
        let ok = Output::resolve(true, r#"{"text": "CMD 42", "tooltip": "t", "class": "warning"}"#);
        assert_eq!(ok, Output { text: "CMD 42".into(), tooltip: "t".into(), class: "warning".into() });
        assert_eq!(Output::resolve(false, r#"{"text": "x"}"#), Output::error());
        assert_eq!(Output::resolve(true, "not json"), Output::error());
        assert_eq!(Output::resolve(true, r#"{"text": 3}"#), Output::error());
        assert_eq!(Output::resolve(true, r#"{"text": ""}"#).text, "");
    }
}
