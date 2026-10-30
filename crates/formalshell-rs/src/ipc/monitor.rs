//! `monitor`'s `status`, `gpu`, `processes`, `kill`,
//! `restart` and `launch`. A reply is whatever the last tick left, never a wait for a
//! fresh one; each read also asks for the poll for a grace period (the
//! subscribe/unsubscribe pulse), so a caller with no monitor cell on the bar
//! finds data on its next call. A signal answers that it was sent, and its
//! own exit status lands in the next `processes` reply's `lastAction`.
//! `mode` joins with the launcher's monitor view.

use fs_system::monitor::gpu::{self, OffloadTools};
use serde_json::Value as Json;

use super::registry::{Function, Target, Type, Value};
use crate::services::icons::data_dirs;
use crate::services::info::monitor::now_ms;
use crate::services::info::procs;
use crate::services::wants::{self, Source};
use crate::wayland::App;

/// One sample lands at startup, before
/// any caller can connect, so the first reply is not an empty one.
pub fn warm() {
    wants::pulse(Source::Monitor);
}

fn status(app: &mut App, _: &[Value]) -> Value {
    wants::pulse(Source::Monitor);
    Value::Str(app.store.info.monitor.status(now_ms()).to_string())
}

fn gpu(app: &mut App, _: &[Value]) -> Value {
    wants::pulse(Source::Monitor);
    Value::Str(app.store.info.monitor.gpu(now_ms()).to_string())
}

fn processes(app: &mut App, args: &[Value]) -> Value {
    wants::pulse(Source::Processes);
    Value::Str(app.store.info.procs.report(args[0].str(), now_ms()).to_string())
}

fn signal(app: &mut App, args: &[Value]) -> Value {
    Value::Str(match procs::begin_signal(args[0].str(), args[1].str()) {
        Ok((n, sig)) => {
            if let Some(rt) = &app.runtime {
                rt.service(move |ctx| procs::run_signal(ctx, n, sig));
            }
            format!("ok: {sig} sent to {n}")
        }
        Err(e) => e,
    })
}

fn restart(app: &mut App, args: &[Value]) -> Value {
    let pid = args[0].str();
    let kernel = fs_system::monitor::procs::parse_pid(pid).and_then(|n| app.store.info.procs.row(n)).is_some_and(|r| r.kernel);
    Value::Str(match procs::begin_restart(pid, kernel) {
        Ok(n) => {
            if let Some(rt) = &app.runtime {
                rt.service(move |ctx| procs::run_restart(ctx, n));
            }
            format!("ok: restarting {n}")
        }
        Err(e) => e,
    })
}

struct Entry {
    exec: String,
    path: String,
    terminal: bool,
}

/// `DesktopEntries.byId`: the first `<id>.desktop` on the data dirs. The
/// raw `Exec` is kept, since the offload wrapper runs it through `sh -c`.
fn entry(id: &str) -> Option<Entry> {
    let text = data_dirs().iter().find_map(|d| std::fs::read_to_string(d.join("applications").join(format!("{id}.desktop"))).ok())?;
    let mut in_entry = false;
    let (mut exec, mut path, mut terminal) = (None, None, false);
    for line in text.lines().map(str::trim) {
        if line.starts_with('[') {
            in_entry = line == "[Desktop Entry]";
        } else if in_entry && let Some((k, v)) = line.split_once('=') {
            match k.trim() {
                "Exec" if exec.is_none() => exec = Some(v.trim().to_owned()),
                "Path" if path.is_none() => path = Some(v.trim().to_owned()),
                "Terminal" => terminal = v.trim() == "true",
                _ => {}
            }
        }
    }
    Some(Entry { exec: exec.unwrap_or_default(), path: path.unwrap_or_default(), terminal })
}

fn shq(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

/// `card` is a card id, or "" for the first discrete one. Spawned with the
/// offload argv built by hand, since a desktop entry's own launch carries
/// no environment.
fn launch(app: &mut App, args: &[Value]) -> Value {
    let (id, card) = (args[0].str(), args[1].str());
    let Some(entry) = entry(id) else { return Value::Str(format!("error: no desktop entry '{id}'")) };
    let m = &app.store.info.monitor;
    let target = if card.is_empty() {
        match m.cards.iter().find(|c| c.discrete) {
            Some(c) => c,
            None => return Value::Str("error: no discrete GPU on this machine".into()),
        }
    } else {
        match m.cards.iter().find(|c| c.record.card.card == card) {
            Some(c) => c,
            None => return Value::Str(format!("error: no GPU card '{card}'")),
        }
    };
    let mut exec = entry.exec;
    if !entry.path.is_empty() {
        exec = format!("cd {} && {exec}", shq(&entry.path));
    }
    let tools = OffloadTools { nvidia_offload: m.tools.0, prime_run: m.tools.1 };
    let mut argv = gpu::offload_argv(&exec, Some(&target.record.card), tools);
    let mut bare = true;
    if entry.terminal {
        let term: Vec<String> = match app.store.config.get("console.command") {
            Some(Json::Array(items)) => items.iter().map(|v| v.as_str().map_or_else(|| v.to_string(), str::to_owned)).collect(),
            Some(Json::String(s)) if !s.is_empty() => vec![s.clone()],
            _ => Vec::new(),
        };
        if !term.is_empty() {
            argv = term.into_iter().chain(argv).collect();
            bare = false;
        }
    }
    crate::services::launch::spawn(&app.store, id, &argv);
    let mut reply = format!("ok: launched '{id}' on {} ({}): {}", target.record.card.card, target.name, argv.join(" "));
    if entry.terminal && bare {
        reply.push_str(" [runInTerminal, but console.command is not set: spawned bare]");
    }
    Value::Str(reply)
}

pub fn target() -> Target<App> {
    Target {
        name: "monitor",
        functions: vec![
            Function { name: "status", params: &[], ret: Type::String, call: status },
            Function { name: "processes", params: &[("query", Type::String)], ret: Type::String, call: processes },
            Function { name: "kill", params: &[("pid", Type::String), ("signal", Type::String)], ret: Type::String, call: signal },
            Function { name: "restart", params: &[("pid", Type::String)], ret: Type::String, call: restart },
            Function { name: "gpu", params: &[], ret: Type::String, call: gpu },
            Function { name: "launch", params: &[("desktopId", Type::String), ("card", Type::String)], ret: Type::String, call: launch },
        ],
    }
}
