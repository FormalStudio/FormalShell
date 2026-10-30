//! IPC. The socket is a source on the UI loop itself: a call such as `menu
//! toggle` is accepted, read and answered there, where the store and the
//! surfaces live, and never waits behind service work. Every read and write
//! is non-blocking, so a slow or silent client only holds its own source;
//! a reply the socket will not take whole is finished on the service thread.
//!
//! Each target is one module returning a [`registry::Target`], listed in
//! [`registry`]; `formalshell-ipc` (`src/bin/formalshell-ipc.rs`) is the
//! client.

mod activity;
mod airplay;
mod bluetooth;
mod bar;
mod caffeinate;
mod headset;
mod calendar;
mod capture;
mod clipboard;
mod console;
pub mod cli;
pub mod client;
mod debug;
mod display;
mod earbuds;
mod gallery;
mod iphone;
mod lights;
mod localsend;
mod lock;
#[cfg(test)]
mod golden;
mod media;
mod menu;
mod mirror;
mod notifications;
mod overnight;
mod monitor;
mod network;
mod nightlight;
mod osd;
mod panel;
mod picker;
mod plugins;
mod power;
mod radio;
mod record;
mod screensaver;
mod switcher;
pub mod registry;
mod screenshot;
mod sidebar;
mod theme;
mod tray;
mod visualizer;
pub mod wire;
mod workspaces;

use std::io::{ErrorKind, Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::sync::OnceLock;

use async_io::Async;
use calloop::generic::Generic;
use calloop::{Interest, LoopHandle, Mode, PostAction};
use futures_lite::AsyncWriteExt;

pub use wire::Request;

use crate::runtime::Ctx;
use crate::wayland::App;
use registry::Registry;

/// The most of one request the server reads.
const MAX_REQUEST: usize = 1 << 20;

fn registry() ->&'static Registry<App> {
    static REGISTRY: OnceLock<Registry<App>> = OnceLock::new();
    REGISTRY.get_or_init(|| Registry {
        targets: vec![
            debug::target(),
            theme::target(),
            theme::wallpaper(),
            bar::target(),
            panel::target(),
            media::target(),
            radio::target(),
            airplay::target(),
            bluetooth::target(),
            visualizer::target(),
            tray::target(),
            overnight::target(),
            earbuds::target(),
            display::target(),
            display::hdr(),
            workspaces::target(),
            monitor::target(),
            network::target(),
            plugins::target(),
            caffeinate::target(),
            headset::target(),
            gallery::target(),
            lock::target(),
            power::target(),
            menu::target(),
            calendar::target(),
            iphone::target(),
            console::target(),
            screensaver::target(),
            screenshot::target(),
            capture::target(),
            record::target(),
            notifications::target(),
            notifications::reminder(),
            activity::target(),
            osd::target(),
            nightlight::target(),
            lights::target(),
            switcher::target(),
            sidebar::target(),
            clipboard::target(),
            picker::target(),
            localsend::target(),
            mirror::target(),
        ],
    })
}

pub fn dispatch(app: &mut App, request: &Request) -> String {
    registry().answer(app, request)
}

pub fn start(_: &Ctx) {
    monitor::warm();
}

/// Binds the socket and serves it on the UI loop.
pub fn listen(handle: &LoopHandle<'static, App>) {
    let path = wire::socket_path();
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    // A socket that still answers belongs to another running shell; one
    // that refuses is a dead shell's leftover.
    if UnixStream::connect(&path).is_ok() {
        eprintln!("ipc: another shell already answers on {}", path.display());
        return;
    }
    let _ = std::fs::remove_file(&path);
    let listener = match UnixListener::bind(&path).and_then(|l| l.set_nonblocking(true).map(|_| l)) {
        Ok(listener) => listener,
        Err(err) => {
            eprintln!("ipc: cannot listen on {}: {err}", path.display());
            return;
        }
    };
    let clients = handle.clone();
    let watched = handle.insert_source(Generic::new(listener, Interest::READ, Mode::Level), move |_, listener, _| {
        loop {
            match listener.accept() {
                Ok((stream, _)) => client(&clients, stream),
                Err(err) if err.kind() == ErrorKind::WouldBlock => break,
                Err(err) => {
                    eprintln!("ipc: accept: {err}");
                    break;
                }
            }
        }
        Ok(PostAction::Continue)
    });
    if let Err(err) = watched {
        eprintln!("ipc: cannot watch the socket: {err}");
    }
}

/// The client writes its request and shuts its half down, so the read
/// ends at EOF; the answer goes back on the same stream.
fn client(handle: &LoopHandle<'static, App>, stream: UnixStream) {
    if stream.set_nonblocking(true).is_err() {
        return;
    }
    let mut bytes = Vec::new();
    let watched = handle.insert_source(Generic::new(stream, Interest::READ, Mode::Level), move |_, stream, app| {
        let mut chunk = [0u8; 4096];
        loop {
            match (&**stream).read(&mut chunk) {
                Ok(0) => break,
                Ok(n) if bytes.len() + n <= MAX_REQUEST => bytes.extend_from_slice(&chunk[..n]),
                Ok(_) => return Ok(PostAction::Remove),
                Err(err) if err.kind() == ErrorKind::WouldBlock => return Ok(PostAction::Continue),
                Err(err) if err.kind() == ErrorKind::Interrupted => {}
                Err(_) => return Ok(PostAction::Remove),
            }
        }
        if let Ok(request) = serde_json::from_slice::<Request>(&bytes) {
            let reply = app.call(&request);
            reply_to(app, &**stream, reply.into_bytes());
        }
        Ok(PostAction::Remove)
    });
    if let Err(err) = watched {
        eprintln!("ipc: cannot watch a client: {err}");
    }
}

/// As much of the reply as the socket takes now; the rest is written on
/// the service thread, off this loop.
fn reply_to(app: &App, stream: &UnixStream, reply: Vec<u8>) {
    let mut sent = 0;
    while sent < reply.len() {
        match (&*stream).write(&reply[sent..]) {
            Ok(0) => return,
            Ok(n) => sent += n,
            Err(err) if err.kind() == ErrorKind::Interrupted => {}
            Err(err) if err.kind() == ErrorKind::WouldBlock => break,
            Err(_) => return,
        }
    }
    if sent == reply.len() {
        return;
    }
    let (Some(runtime), Ok(stream)) = (&app.runtime, stream.try_clone()) else { return };
    runtime.service(move |ctx| {
        ctx.spawn(async move {
            if let Ok(mut stream) = Async::new(stream) {
                let _ = stream.write_all(&reply[sent..]).await;
            }
        });
    });
}

