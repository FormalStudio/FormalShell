//! `monitor`'s `status`, `gpu`, `processes`, `kill`,
//! `restart` and `launch`. A reply is whatever the last tick left, never a wait for a
//! fresh one; each read also asks for the poll for a grace period (the
//! subscribe/unsubscribe pulse), so a caller with no monitor cell on the bar
//! finds data on its next call. A signal answers that it was sent, and its
//! own exit status lands in the next `processes` reply's `lastAction`.
//! `mode` joins with the launcher's monitor view.

use super::registry::{Function, Reply, Target, Type, Value, text};
use crate::runtime::now_ms;
use crate::services::info::procs;
use crate::services::launch;
use crate::services::wants::{self, Source};
use crate::wayland::App;

fn status(app: &mut App, _: &[Value]) -> Reply {
    wants::pulse(Source::Monitor);
    Ok(app.store.monitor.status(now_ms() as u64))
}

fn gpu(app: &mut App, _: &[Value]) -> Reply {
    wants::pulse(Source::Monitor);
    Ok(app.store.monitor.gpu(now_ms() as u64))
}

fn processes(app: &mut App, args: &[Value]) -> Reply {
    wants::pulse(Source::Processes);
    Ok(app.store.procs.report(args[0].str(), now_ms() as u64))
}

fn signal(app: &mut App, args: &[Value]) -> Reply {
    let (n, sig) = procs::begin_signal(args[0].str(), args[1].str())?;
    if let Some(rt) = &app.runtime {
        rt.service(move |ctx| procs::run_signal(ctx, n, sig));
    }
    text(format!("ok: {sig} sent to {n}"))
}

fn restart(app: &mut App, args: &[Value]) -> Reply {
    let pid = args[0].str();
    let kernel = fs_system::monitor::procs::parse_pid(pid).and_then(|n| app.store.procs.row(n)).is_some_and(|r| r.kernel);
    let n = procs::begin_restart(pid, kernel)?;
    if let Some(rt) = &app.runtime {
        rt.service(move |ctx| procs::run_restart(ctx, n));
    }
    text(format!("ok: restarting {n}"))
}

/// `card` is a card id, or "" for the first discrete one.
fn launch(app: &mut App, args: &[Value]) -> Reply {
    let (id, card) = (args[0].str(), args[1].str());
    let l = launch::on_gpu(&app.store, id, card)?;
    let mut reply = format!("ok: launched '{id}' on {} ({}): {}", l.card, l.name, l.argv.join(" "));
    if l.bare_terminal {
        reply.push_str(" [runInTerminal, but console.command is not set: spawned bare]");
    }
    text(reply)
}

/// Registering pulses the poll, so one sample lands at startup before any
/// caller can connect and the first reply is not an empty one.
pub fn target() -> Target<App> {
    wants::pulse(Source::Monitor);
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
