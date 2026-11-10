//! The lines on stderr that the smoke legs, `dev/perf-gate.py` and the rig
//! scripts parse. Each format is written by one function here, so a leg's
//! regex has one place to be checked against.

use std::fmt::{Arguments, Display};
use std::io::Write;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use crate::ipc::Request;

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

/// `commit surface=<surface> n=<n> t=<ms>ms <rest>`, when `FS_TRACE` is set.
pub fn commit(surface: impl Display, n: u64, rest: Arguments) {
    if enabled() {
        write(format_args!("commit surface={surface} n={n} t={}ms {rest}", ms()));
    }
}

/// `commit surface=<surface> <rest>`: a commit line with no count or clock.
/// Not gated; a caller that only wants it traced checks [`enabled`].
pub fn commit_bare(surface: impl Display, rest: Arguments) {
    write(format_args!("commit surface={surface} {rest}"));
}

fn ms() -> u128 {
    STARTED.get().map_or(0, |s| s.elapsed().as_millis())
}

/// `<kind> <target> <member>` then each argument JSON-quoted: an IPC
/// request as every line below names it.
pub fn request(r: &Request) -> impl Display + '_ {
    struct Line<'a>(&'a Request);
    impl Display for Line<'_> {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            match self.0 {
                Request::Call { target, function, args } => {
                    write!(f, "call {target} {function}")?;
                    args.iter().try_for_each(|a| write!(f, " {}", serde_json::Value::from(a.as_str())))
                }
                Request::Show { target, name } => write!(f, "show {target} {name}"),
                Request::Signal { target, signal } => write!(f, "signal {target} {signal}"),
                Request::Prop { target, property } => write!(f, "prop {target} {property}"),
            }
        }
    }
    Line(r)
}

/// `ipc t=<ms>ms call <target> <function> "<arg>"...`: an IPC request
/// reaching the UI loop (see [`request`]).
pub fn call(r: &Request) {
    write(format_args!("ipc t={}ms {}", ms(), request(r)));
}

/// A surface whose map state legs time against the IPC calls.
pub enum Surface<'a> {
    Bar,
    /// A panel by id, or with none whichever panel was up.
    Panel(Option<&'a str>),
    PluginOverlay,
    TrayMenu,
    Scrim,
    Menu,
    Overview,
    Switcher,
    ProfilesPill,
    Screensaver,
    Toasts,
    HeadsetCard,
    CapturePicker,
    Radio,
    Osd,
}

impl Display for Surface<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Surface::Bar => "bar",
            Surface::Panel(Some(id)) => return write!(f, "panel {id}"),
            Surface::Panel(None) => "panel",
            Surface::PluginOverlay => "panel plugin overlay",
            Surface::TrayMenu => "tray menu",
            Surface::Scrim => "scrim",
            Surface::Menu => "menu",
            Surface::Overview => "overview",
            Surface::Switcher => "switcher",
            Surface::ProfilesPill => "profiles pill",
            Surface::Screensaver => "screensaver",
            Surface::Toasts => "toasts",
            Surface::HeadsetCard => "headset card",
            Surface::CapturePicker => "capture picker",
            Surface::Radio => "radio",
            Surface::Osd => "osd",
        })
    }
}

pub enum State {
    /// Asked to open, before its first frame.
    Opening,
    Configured,
    Mapped,
    /// Its open motion under way.
    Zooming,
    Shown,
    Closing,
    Unmapped,
    UnmappedUnderFullscreen,
    /// Mapped with an empty input region, so the pointer passes through.
    InputEmpty,
}

impl Display for State {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            State::Opening => "opening",
            State::Configured => "configured",
            State::Mapped => "mapped",
            State::Zooming => "zooming",
            State::Shown => "shown",
            State::Closing => "closing",
            State::Unmapped => "unmapped",
            State::UnmappedUnderFullscreen => "unmapped under fullscreen",
            State::InputEmpty => "input empty",
        })
    }
}

/// `ipc t=<ms>ms <surface> <state>` (`ipc t=12ms menu configured`).
pub fn mapped(surface: Surface, state: State) {
    write(format_args!("ipc t={}ms {surface} {state}", ms()));
}

/// `ipc t=<ms>ms <what>`: a line for a person reading the log; no leg
/// parses these.
pub fn note(what: Arguments) {
    write(format_args!("ipc t={}ms {what}", ms()));
}

/// `event loop: slow <kind> t=<ms>ms <detail>` when `took` passes the one
/// threshold.
pub fn slow(kind: &str, took: Duration, detail: Arguments) {
    if took >= SLOW {
        eprintln!("event loop: slow {kind} t={}ms {detail}", ms());
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

/// `lock: logind asked for the lock` (or `the unlock`): logind's Lock or
/// Unlock signal for this session.
pub fn logind_lock(lock: bool) {
    write(format_args!("lock: logind asked for the {}", if lock { "lock" } else { "unlock" }));
}

/// `lock: login keyring unlocked`, or `lock: login keyring left locked:
/// denied` / `failed, <why>`. No daemon listening writes nothing.
pub fn keyring(outcome: &fs_auth::keyring::Unlock) {
    use fs_auth::keyring::Unlock;
    match outcome {
        Unlock::Unlocked => write(format_args!("lock: login keyring unlocked")),
        Unlock::NoDaemon => {}
        Unlock::Denied => write(format_args!("lock: login keyring left locked: denied")),
        Unlock::Failed(why) => write(format_args!("lock: login keyring left locked: failed, {why}")),
    }
}

/// `power: <method>: <err>`: logind refused a power call.
pub fn power_refused(method: &str, err: impl Display) {
    write(format_args!("power: {method}: {err}"));
}

/// `formalshell: render: <reason>; drawing on the CPU`: the GPU path gave up.
pub fn render_fallback(reason: &str) {
    write(format_args!("formalshell: render: {reason}; drawing on the CPU"));
}

/// `formalshell: instance lock: live instance found, requesting takeover`.
pub fn instance_takeover() {
    write(format_args!("formalshell: instance lock: live instance found, requesting takeover"));
}

/// `formalshell: instance lock: being replaced (<by>), quitting`.
pub fn instance_replaced(by: &str) {
    write(format_args!("formalshell: instance lock: being replaced ({by}), quitting"));
}

/// `RecordingService: webcam overlay did not settle in time, recording anyway`.
pub fn webcam_unsettled() {
    write(format_args!("RecordingService: webcam overlay did not settle in time, recording anyway"));
}

/// `earbuds: heart check skipped, <why>`.
pub fn heart_check_skipped(why: &str) {
    write(format_args!("earbuds: heart check skipped, {why}"));
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
