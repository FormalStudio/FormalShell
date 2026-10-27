mod boot;
mod cmdline;
mod effects;
mod fontconfig;
mod greeter;
mod install;
mod instance;
mod keyboard;
mod ipc;
mod motion;
mod render;
mod runtime;
mod scene;
mod scroll;
mod services;
mod sfx;
mod shards;
mod store;
mod surface;
mod surfaces;
mod text;
mod ui;
mod wayland;

use std::sync::OnceLock;
use std::time::{Instant, SystemTime};

use calloop::EventLoop;
use calloop::channel::{self, Event};
use smithay_client_toolkit::reexports::calloop_wayland_source::WaylandSource;
use smithay_client_toolkit::reexports::client::Connection;
use smithay_client_toolkit::reexports::client::globals::registry_queue_init;

use runtime::{Msg, Publisher, Runtime};
use wayland::App;

static STARTED: OnceLock<Instant> = OnceLock::new();

/// One step of the cold start, on the same zero as every `t=` in the log.
pub fn phase(name: &str) {
    let t = STARTED.get().map_or(0, |s| s.elapsed().as_micros());
    eprintln!("phase t={}.{:03}ms {name}", t / 1000, t % 1000);
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    match cmdline::parse(&args) {
        cmdline::Command::Shell => {}
        cmdline::Command::Greeter(rest) => {
            greeter::main(rest);
            return;
        }
        cmdline::Command::Install(cmd, rest) => std::process::exit(install::main(cmd, rest)),
        cmdline::Command::Ipc(rest) => std::process::exit(ipc::client::run(rest)),
        cmdline::Command::Theme(rest) => std::process::exit(ipc::client::run(&cmdline::theme_call(rest))),
        cmdline::Command::Usage => {
            eprint!("{}", cmdline::USAGE);
            std::process::exit(2);
        }
    }
    tune_malloc();
    // Every `t=` in the log counts from here; this line puts that zero on the
    // wall clock, so a cold start reads against the launcher's own stamp.
    let started = *STARTED.get_or_init(Instant::now);
    let epoch = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap_or_default();
    eprintln!("start epoch_us={}", epoch.as_micros());
    instance::acquire();
    own_process_group();
    phase("instance");

    let conn = Connection::connect_to_env().expect("no Wayland compositor to connect to");
    let (globals, queue) = registry_queue_init::<App>(&conn).expect("wl_registry");
    let qh = queue.handle();
    phase("wayland");

    let mut event_loop: EventLoop<'static, App> = EventLoop::try_new().expect("event loop");
    let handle = event_loop.handle();
    WaylandSource::new(conn.clone(), queue).insert(handle.clone()).expect("wayland source");

    let mut app = App::new(&globals, &qh, started);
    app.warm_bar_faces();
    phase("app");

    let (sender, inbox) = channel::channel::<Msg>();
    handle
        .insert_source(inbox, |event, _, app| {
            if let Event::Msg(msg) = event {
                app.receive(msg);
            }
        })
        .expect("runtime channel");
    let runtime = Runtime::start(Publisher::new(sender), |ctx| {
        services::start(ctx);
        ipc::start(ctx);
    });
    app.runtime = Some(runtime);
    phase("runtime");
    app.set_handle(handle.clone());
    ipc::listen(&handle);

    let signal = event_loop.get_signal();
    let mut cpu_mark = thread_cpu_us();
    let ended = event_loop.run(None, &mut app, |app| {
        // A dispatch that ran the thread past half a frame is one line, read
        // off the thread's own CPU clock so the wait for events is not counted.
        let cpu = thread_cpu_us();
        if cpu - cpu_mark >= 8000 {
            eprintln!("event loop: slow dispatch t={}ms cpu_us={}", started.elapsed().as_millis(), cpu - cpu_mark);
        }
        // A present that holds the thread past half a frame is one line.
        let t0 = Instant::now();
        app.present();
        if t0.elapsed().as_millis() >= 8 {
            eprintln!("event loop: slow present t={}ms present_us={}", started.elapsed().as_millis(), t0.elapsed().as_micros());
        }
        if app.exit {
            signal.stop();
        }
        cpu_mark = thread_cpu_us();
    });
    app.report_exit();
    reap_children();
    if let Err(err) = ended {
        eprintln!("event loop: {err}");
    }
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

/// glibc's mmap threshold climbs to 32 MiB once a large block is freed, and
/// a canvas or scene after that comes out of the heap and stays in RSS
/// after it is dropped (the screensaver's, the launcher's). Pinned, every
/// such buffer is its own mapping, gone with its owner. Fewer arenas keep
/// the threads' small allocations from each holding their own slack.
fn tune_malloc() {
    #[cfg(target_env = "gnu")]
    // SAFETY: mallopt only sets allocator parameters, before any thread.
    unsafe {
        libc::mallopt(libc::M_MMAP_THRESHOLD, 256 * 1024);
        libc::mallopt(libc::M_ARENA_MAX, 2);
    }
}

/// Hands freed heap pages back to the kernel, after a large surface goes.
pub fn trim_heap() {
    #[cfg(target_env = "gnu")]
    // SAFETY: malloc_trim is thread-safe.
    unsafe {
        libc::malloc_trim(0);
    }
}

/// `FS_TRACE` set: every commit is a line on stderr, which the smoke rig's
/// budget legs read. Unset, a shell left running writes nothing per frame.
pub fn tracing() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("FS_TRACE").is_some())
}

/// One trace line in one write: unbuffered stderr would take a syscall
/// per formatted piece.
pub fn trace(mut line: String) {
    use std::io::Write;
    line.push('\n');
    let _ = std::io::stderr().write_all(line.as_bytes());
}

/// CPU time the calling thread has run, in microseconds.
fn thread_cpu_us() -> i64 {
    let mut ts = libc::timespec { tv_sec: 0, tv_nsec: 0 };
    // SAFETY: a valid out pointer, for the calling thread's own clock.
    unsafe { libc::clock_gettime(libc::CLOCK_THREAD_CPUTIME_ID, &mut ts) };
    ts.tv_sec * 1_000_000 + ts.tv_nsec / 1000
}

/// The shell leads a process group of its own, which every child it starts
/// joins, so its way out (an exit, a SIGTERM, a SIGINT, a SIGHUP) takes
/// them along: `PR_SET_PDEATHSIG` alone misses a child's own children.
fn own_process_group() {
    // SAFETY: plain syscalls; the handler only makes async-signal-safe ones.
    unsafe {
        libc::setpgid(0, 0);
        for sig in [libc::SIGTERM, libc::SIGINT, libc::SIGHUP] {
            libc::signal(sig, on_exit_signal as *const () as libc::sighandler_t);
        }
    }
}

extern "C" fn on_exit_signal(sig: libc::c_int) {
    // SAFETY: async-signal-safe calls only. With its handler gone, the
    // signal the group gets ends the shell as well.
    unsafe {
        libc::signal(sig, libc::SIG_DFL);
        if libc::getpgrp() == libc::getpid() {
            libc::kill(0, sig);
        } else {
            libc::raise(sig);
        }
    }
}

/// SIGTERM to every child still running, from the group the shell leads.
fn reap_children() {
    // SAFETY: plain syscalls on the shell's own group.
    unsafe {
        if libc::getpgrp() == libc::getpid() {
            libc::signal(libc::SIGTERM, libc::SIG_IGN);
            libc::kill(0, libc::SIGTERM);
        }
    }
}
