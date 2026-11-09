//! The live process table and the three things a row can do to a process
//! One `/proc` walk per tick while something wants
//! it, parsed by fs-system; the table is unsorted and unfiltered, since
//! ordering and matching are each reader's call. A signal answers at once
//! with what was sent, and its own exit status lands in [`State::last`] a
//! moment later, which `monitor processes` reports.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;

use async_io::Timer;
#[cfg(test)]
use fs_system::monitor::collect::split_sections;
use fs_system::monitor::procs::{self, DeltaOptions, ProcRecord, ProcRow};
use futures_lite::FutureExt;
use serde_json::{Value, json};

use super::monitor::now_ms;
use super::{idle, kicked};
use crate::runtime::Ctx;
use crate::services::config;
use crate::services::compositor;
use crate::services::proc;
use crate::services::wants::Source;
use crate::store;

/// How a signal or a restart ended: ProcessService's `lastResult`.
#[derive(Clone, Debug, PartialEq)]
pub struct Action {
    pub pid: u64,
    pub action: String,
    pub ok: bool,
    pub message: String,
    pub at_ms: u64,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct State {
    pub rows: Vec<ProcRow>,
    pub available: bool,
    pub sampled_ms: Option<u64>,
    pub last: Option<Action>,
}

pub enum Diff {
    Rows { rows: Vec<ProcRow>, available: bool, sampled_ms: u64 },
    Action(Action),
}

impl State {
    pub fn apply(&mut self, diff: Diff) -> bool {
        match diff {
            Diff::Rows { rows, available, sampled_ms } => {
                let changed = self.rows != rows || self.available != available;
                self.rows = rows;
                self.available = available;
                self.sampled_ms = Some(sampled_ms);
                changed
            }
            Diff::Action(a) => {
                let changed = self.last.as_ref() != Some(&a);
                self.last = Some(a);
                changed
            }
        }
    }

    pub fn row(&self, pid: u64) -> Option<&ProcRow> {
        self.rows.iter().find(|r| r.pid == pid)
    }

    /// The IPC `processes` reply: the rows matching `query`, by CPU,
    /// capped at 40 with `matched` saying what the cap dropped.
    pub fn report(&self, query: &str, now_ms: u64) -> Value {
        let rows = procs::sort_rows(&procs::filter_rows(&self.rows, query), "cpu");
        let num = |v: Option<f64>| v.map_or(Value::Null, |n| json!(n));
        json!({
            "sampledAtMs": self.sampled_ms,
            "ageMs": self.sampled_ms.map(|t| now_ms.saturating_sub(t)),
            "available": self.available,
            "total": self.rows.len(),
            "matched": rows.len(),
            "lastAction": self.last.as_ref().map(|a| json!({
                "pid": a.pid, "action": a.action, "ok": a.ok, "message": a.message, "at": a.at_ms,
            })),
            "rows": rows.iter().take(40).map(|r| json!({
                "pid": r.pid, "ppid": r.ppid, "name": r.name, "cmd": r.cmd, "kernel": r.kernel,
                "state": r.state, "threads": r.threads, "cpuFraction": num(r.cpu_fraction),
                "memBytes": r.mem_bytes, "memFraction": num(r.mem_fraction),
            })).collect::<Vec<_>>(),
        })
    }
}

fn interval() -> Duration {
    Duration::from_millis(config::current().interval("monitor.processIntervalMs", 2000).as_millis().max(500) as u64)
}

/// What a tick compares itself to.
#[derive(Default)]
struct Prev {
    records: Option<Vec<ProcRecord>>,
    total: f64,
}

/// The process table as the parsers read it, on a pool thread. A process
/// that exits between the listing and its read is skipped.
fn collect() -> std::collections::HashMap<String, String> {
    let lines_of = |path: &str, keep: &dyn Fn(&str) -> bool| {
        std::fs::read_to_string(path).unwrap_or_default().lines().filter(|l| keep(l)).collect::<Vec<_>>().join("\n")
    };
    let mut pids: Vec<String> = std::fs::read_dir("/proc")
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|n| n.starts_with(|c: char| c.is_ascii_digit()))
        .collect();
    pids.sort();
    let (mut procs, mut cmdline) = (String::new(), String::new());
    for pid in &pids {
        if let Ok(stat) = std::fs::read_to_string(format!("/proc/{pid}/stat")) {
            procs.push_str(&stat);
        }
        if let Ok(raw) = std::fs::read(format!("/proc/{pid}/cmdline"))
            && !raw.is_empty()
        {
            let text = String::from_utf8_lossy(&raw).replace('\0', " ");
            for line in text.split('\n') {
                cmdline.push_str(&format!("/proc/{pid}/cmdline:{line}\n"));
            }
        }
    }
    // SAFETY: sysconf has no preconditions.
    let page = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
    [
        ("stat", lines_of("/proc/stat", &|l| l.starts_with("cpu "))),
        ("mem", lines_of("/proc/meminfo", &|l| l.starts_with("MemTotal:"))),
        ("meta", page.to_string()),
        ("procs", procs),
        ("cmdline", cmdline),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_owned(), v))
    .collect()
}

fn sample(prev: &mut Prev, sections: &std::collections::HashMap<String, String>) -> (Vec<ProcRow>, bool) {
    let section = |name: &str| sections.get(name).map_or("", String::as_str);
    let records = procs::parse_procs(section("procs"));
    let total = procs::total_jiffies(section("stat"));
    let delta = match total {
        Some(t) if prev.total > 0.0 => t - prev.total,
        _ => 0.0,
    };
    let rows = procs::proc_delta(
        prev.records.as_deref(),
        &records,
        delta,
        &DeltaOptions {
            page_size: Some(procs::page_size(section("meta"))),
            mem_total_bytes: procs::mem_total_bytes(section("mem")),
            cmdlines: procs::parse_cmdlines(section("cmdline")),
        },
    );
    let available = !records.is_empty();
    prev.records = Some(records);
    prev.total = total.unwrap_or(0.0);
    (rows, available)
}

pub async fn run(ctx: Ctx) {
    let rx = config::subscribe();
    let kick = kicked(Source::Processes);
    while kick.try_recv().is_ok() {}
    let mut prev = Prev::default();
    loop {
        let wait = interval();
        let sections = ctx.pool().run(collect).await.unwrap_or_default();
        let (rows, available) = sample(&mut prev, &sections);
        ctx.publish(store::Diff::Procs(Diff::Rows { rows, available, sampled_ms: now_ms() }));
        let kicked = async {
            let _ = kick.recv().await;
        };
        kicked.or(idle(wait, &rx, &wait, interval)).await;
    }
}

fn finish(ctx: &Ctx, pid: u64, action: &str, ok: bool, message: &str) {
    ctx.publish(store::Diff::Procs(Diff::Action(Action {
        pid,
        action: action.to_owned(),
        ok,
        message: message.to_owned(),
        at_ms: now_ms(),
    })));
    // The table is one poll behind an action that worked, and a row for a
    // process already gone invites a second press on it.
    if ok {
        super::kick(Source::Processes);
    }
}

static SIGNAL_BUSY: AtomicBool = AtomicBool::new(false);
static RESTARTING: AtomicU64 = AtomicU64::new(0);

/// Checks a signal request and claims the one slot; the IPC reply is built
/// from what this answers, and [`run_signal`] then sends it.
pub fn begin_signal(pid: &str, name: &str) -> Result<(u64, &'static str), String> {
    let Some(n) = procs::parse_pid(pid) else { return Err(format!("'{pid}' is not a pid")) };
    let Some(sig) = procs::normalize_signal(Some(name)) else {
        return Err(format!("signal must be one of {}", procs::SIGNALS.join(", ")));
    };
    if SIGNAL_BUSY.swap(true, Ordering::SeqCst) {
        return Err("a signal is already in flight".into());
    }
    Ok((n, sig))
}

pub fn run_signal(ctx: &Ctx, n: u64, sig: &'static str) {
    let task = ctx.clone();
    ctx.spawn(async move {
        let done = proc::capture(&procs::kill_command(n, sig), Duration::from_secs(10)).await;
        if done.code == 0 {
            finish(&task, n, sig, true, "sent");
        } else {
            finish(&task, n, sig, false, &format!("kill exited {}", done.code));
        }
        SIGNAL_BUSY.store(false, Ordering::SeqCst);
    });
}

const RESTART_TIMEOUT: Duration = Duration::from_millis(5000);
const RESTART_POLL: Duration = Duration::from_millis(200);

/// `kernel` is whether the table says the pid is a kernel thread.
pub fn begin_restart(pid: &str, kernel: bool) -> Result<u64, String> {
    let Some(n) = procs::parse_pid(pid) else { return Err(format!("'{pid}' is not a pid")) };
    let busy = RESTARTING.load(Ordering::SeqCst);
    if busy != 0 {
        return Err(format!("a restart is already in flight for {busy}"));
    }
    if kernel {
        return Err(format!("{n} is a kernel thread, it has no command line to re-run"));
    }
    RESTARTING.store(n, Ordering::SeqCst);
    Ok(n)
}

pub fn run_restart(ctx: &Ctx, n: u64) {
    let task = ctx.clone();
    ctx.spawn(async move {
        restart_task(&task, n).await;
        RESTARTING.store(0, Ordering::SeqCst);
    });
}

async fn restart_task(ctx: &Ctx, pid: u64) {
    let fail = |message: &str| finish(ctx, pid, "RESTART", false, message);
    let argv = proc::capture(&procs::argv_command(pid), Duration::from_secs(10)).await;
    let args = procs::parse_argv(&argv.stdout);
    if argv.code != 0 || args.is_empty() {
        return fail("no command line to re-run (process gone, or a kernel thread)");
    }
    // Unreadable is normal (another user's process): the respawn then runs
    // from the shell's own cwd.
    let cwd = proc::capture(&procs::cwd_command(pid), Duration::from_secs(10)).await.stdout.trim().to_owned();
    let term = proc::capture(&procs::kill_command(pid, "TERM"), Duration::from_secs(10)).await;
    if term.code != 0 {
        return fail(&format!("kill exited {}", term.code));
    }
    // Re-running while the old one is still up would give two copies, the
    // one outcome worse than a restart that did not happen. A process that
    // ignores TERM is reported rather than escalated to KILL unasked.
    let mut waited = Duration::ZERO;
    loop {
        Timer::after(RESTART_POLL).await;
        waited += RESTART_POLL;
        if proc::capture(&procs::alive_command(pid), Duration::from_secs(10)).await.code != 0 {
            break;
        }
        if waited >= RESTART_TIMEOUT {
            return fail(&format!("still running {}s after TERM, nothing re-run", RESTART_TIMEOUT.as_secs()));
        }
    }
    compositor::spawn(&procs::respawn_command(&args, &cwd));
    finish(ctx, pid, "RESTART", true, &format!("re-ran {}", args[0]));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sections(stat: &str, procs: &str) -> std::collections::HashMap<String, String> {
        split_sections(&format!("@stat\n{stat}\n@mem\nMemTotal: 1000 kB\n@meta\n4096\n@procs\n{procs}\n@cmdline\n@end"))
    }

    const P: &str = "7 (smokevictim) R 1 7 7 0 -1 0 0 0 0 0 100 50 0 0 20 0 1 0 500 0 10 0 0 0 0 0 0 0 0 0 0 0 0 0 17 0 0 0 0 0 0";

    #[test]
    fn the_first_tick_has_no_cpu_figure() {
        let mut prev = Prev::default();
        let (rows, available) = sample(&mut prev, &sections("cpu 100 0 100 800 0 0 0 0", P));
        assert!(available);
        assert_eq!(rows[0].cpu_fraction, None);
    }

    #[test]
    fn a_bad_pid_and_signal_are_refused_before_anything_runs() {
        assert!(procs::parse_pid("0").is_none());
        assert_eq!(procs::normalize_signal(Some("usr1")), None);
        assert_eq!(procs::normalize_signal(Some("")), Some("TERM"));
    }
}
