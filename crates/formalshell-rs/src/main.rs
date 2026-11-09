mod chrome;
mod cmdline;
mod fontconfig;
mod gpu;
mod greeter;
mod input;
mod install;
mod instance;
mod keyboard;
mod ipc;
mod motion;
mod paths;
mod paint;
mod render;
mod runtime;
mod scene;
mod scroll;
mod services;
mod settings;
mod shards;
mod store;
mod surface;
mod surfaces;
mod text;
mod trace;
mod ui;
mod wayland;

use std::time::{Duration, Instant, SystemTime};

use calloop::EventLoop;
use calloop::channel::{self, Event};
use smithay_client_toolkit::reexports::calloop_wayland_source::WaylandSource;
use smithay_client_toolkit::reexports::client::Connection;
use smithay_client_toolkit::reexports::client::globals::registry_queue_init;

use runtime::{Msg, Publisher, Runtime};
use wayland::App;

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
    let started = trace::start();
    let epoch = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap_or_default();
    eprintln!("start epoch_us={}", epoch.as_micros());
    instance::acquire();
    own_process_group();
    trace::phase("instance");
    text::preload();
    wayland::lock::preload_login();

    let conn = Connection::connect_to_env().expect("no Wayland compositor to connect to");
    trace::phase("wayland connect");
    let (globals, queue) = registry_queue_init::<App>(&conn).expect("wl_registry");
    let qh = queue.handle();
    trace::phase("wayland");
    gpu::resolve(settings::render_backend(&services::config::read_file()), &conn, &globals);
    trace::phase("render");

    let mut event_loop: EventLoop<'static, App> = EventLoop::try_new().expect("event loop");
    let handle = event_loop.handle();
    WaylandSource::new(conn.clone(), queue).insert(handle.clone()).expect("wayland source");

    let mut app = App::new(&globals, &qh, started, motion_scale());
    app.warm_bar_faces();
    trace::phase("app");

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
    trace::phase("runtime");
    app.set_handle(handle.clone());
    gpu::watch(&handle, App::gpu_up);
    ipc::listen(&handle);

    let signal = event_loop.get_signal();
    let mut cpu_mark = thread_cpu_us();
    let ended = event_loop.run(None, &mut app, |app| {
        // A dispatch that ran the thread past half a frame is one line, read
        // off the thread's own CPU clock so the wait for events is not counted.
        let cpu = thread_cpu_us();
        let ran = Duration::from_micros((cpu - cpu_mark).max(0) as u64);
        trace::slow("dispatch", ran, format_args!("cpu_us={}", ran.as_micros()));
        // A present that holds the thread past half a frame is one line.
        let t0 = Instant::now();
        app.present();
        trace::slow("present", t0.elapsed(), format_args!("present_us={}", t0.elapsed().as_micros()));
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

/// glibc's mmap threshold climbs to 32 MiB once a large block is freed, and
/// a canvas or scene after that comes out of the heap and stays in RSS
/// after it is dropped (the screensaver's, the launcher's). Pinned, every
/// such buffer is its own mapping, gone with its owner. Fewer arenas keep
/// the threads' small allocations from each holding their own slack.
/// `FORMALSHELL_SMOKE_MOTION_SCALE`, a percentage of every motion clock, for
/// a leg that has to photograph the first frames of something the shell
/// does at start, before `debug motionScale` can reach it.
fn motion_scale() -> f64 {
    std::env::var("FORMALSHELL_SMOKE_MOTION_SCALE").ok().and_then(|v| v.parse::<f64>().ok()).map_or(1.0, |p| p / 100.0)
}

fn tune_malloc() {
    #[cfg(target_env = "gnu")]
    // SAFETY: mallopt only sets allocator parameters, before any thread.
    unsafe {
        libc::mallopt(libc::M_MMAP_THRESHOLD, 256 * 1024);
        libc::mallopt(libc::M_ARENA_MAX, 2);
    }
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
