//! The lines on stderr that the smoke legs, `dev/perf-gate.py` and the rig
//! scripts parse. Each format is written by one function here, so a leg's
//! regex has one place to be checked against.

use std::fmt::{Arguments, Display};
use std::io::Write;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

static STARTED: OnceLock<Instant> = OnceLock::new();

/// A dispatch, present or draw that holds the UI thread longer than this is
/// one `event loop: slow ...` line: half a frame at 60 Hz.
const SLOW: Duration = Duration::from_millis(8);

/// Zero of every `t=` in the log, taken once at start.
pub fn start() -> Instant {
    *STARTED.get_or_init(Instant::now)
}

/// `FS_TRACE` set: every commit is a line on stderr, which the smoke rig's
/// budget legs read. Unset, a shell left running writes nothing per frame.
pub fn enabled() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("FS_TRACE").is_some())
}

/// One line in one write: unbuffered stderr would take a syscall per
/// formatted piece.
fn write(args: Arguments) {
    let mut line = args.to_string();
    line.push('\n');
    let _ = std::io::stderr().write_all(line.as_bytes());
}

/// Milliseconds since `started`, the `t=` of the lines below.
pub fn since(started: Instant) -> u128 {
    started.elapsed().as_millis()
}

/// `commit surface=<surface> n=<n> t=<t_ms>ms <rest>`, when `FS_TRACE` is set.
pub fn commit(surface: impl Display, n: u64, t_ms: u128, rest: Arguments) {
    if enabled() {
        write(format_args!("commit surface={surface} n={n} t={t_ms}ms {rest}"));
    }
}

/// `commit surface=<surface> <rest>`: a commit line with no count or clock.
/// Not gated; a caller that only wants it traced checks [`enabled`].
pub fn commit_bare(surface: impl Display, rest: Arguments) {
    write(format_args!("commit surface={surface} {rest}"));
}

/// `ipc t=<ms>ms <what>`: a surface mapped or unmapped, or an IPC call
/// reaching the UI loop.
pub fn ipc(started: Instant, what: &str) {
    eprintln!("ipc t={}ms {what}", since(started));
}

/// `event loop: slow <kind> t=<ms>ms <detail>` when `took` passes the one
/// threshold.
pub fn slow(kind: &str, took: Duration, detail: Arguments) {
    if took >= SLOW {
        eprintln!("event loop: slow {kind} t={}ms {detail}", STARTED.get().map_or(0, |s| s.elapsed().as_millis()));
    }
}

/// `dissolve frame <ms>`: one pixel-dissolve frame's drawing time.
pub fn dissolve_frame(ms: f64) {
    eprintln!("dissolve frame {ms:.2}");
}

/// A free-form line a leg reads, written whole (`atlas globe ...`).
pub fn line(args: Arguments) {
    write(args);
}

/// One step of the cold start, on the same zero as every `t=` in the log.
pub fn phase(name: &str) {
    let t = STARTED.get().map_or(0, |s| s.elapsed().as_micros());
    eprintln!("phase t={}.{:03}ms {name}", t / 1000, t % 1000);
}

/// Every thread's CPU time so far, in clock ticks, for reading what ran
/// during the cold start.
pub fn thread_ticks() {
    let Ok(tasks) = std::fs::read_dir("/proc/self/task") else { return };
    let mut out = Vec::new();
    for task in tasks.flatten() {
        let comm = std::fs::read_to_string(task.path().join("comm")).unwrap_or_default();
        let stat = std::fs::read_to_string(task.path().join("stat")).unwrap_or_default();
        // utime and stime, the 14th and 15th fields: 12th and 13th past the
        // comm's closing paren.
        let fields: Vec<&str> = stat.rsplit_once(") ").map(|(_, rest)| rest.split(' ').collect()).unwrap_or_default();
        let tick = |i: usize| fields.get(i).and_then(|f| f.parse::<u64>().ok()).unwrap_or(0);
        out.push(format!("{}={}", comm.trim(), tick(11) + tick(12)));
    }
    eprintln!("phase threads {}", out.join(" "));
}

/// Hands freed heap pages back to the kernel, after a large surface goes.
pub fn trim_heap() {
    #[cfg(target_env = "gnu")]
    // SAFETY: malloc_trim is thread-safe.
    unsafe {
        libc::malloc_trim(0);
    }
}
