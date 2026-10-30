//! The single-instance lock: one shell per Wayland
//! session, the newest winning. A socket keyed on `WAYLAND_DISPLAY` answers
//! every connection with `formalshell <pid>`; a new shell that hears that
//! line asks for `takeover <pid>`, waits for the old one to close its end,
//! and binds in its place.
//!
//! Taken before the Wayland connection: the old shell's surfaces and IPC
//! socket have to be gone before this one makes its own. A replaced shell
//! exits outright; a session lock it held stays locked by the compositor,
//! which is ext-session-lock's contract.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::time::{Duration, Instant};

/// How long a new shell waits on the old one.
const TAKEOVER_WAIT: Duration = Duration::from_secs(2);

fn log(line: &str) {
    eprintln!("formalshell: instance lock: {line}");
}

pub fn socket_path() -> PathBuf {
    let runtime = std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".into());
    let key = std::env::var("WAYLAND_DISPLAY").unwrap_or_else(|_| "default".into()).replace('/', "_");
    PathBuf::from(runtime).join("formalshell").join(format!("instance-{key}.sock"))
}

/// `$XDG_RUNTIME_DIR/formalshell/<name>-<compositor instance>`: a file that
/// outlives one shell and belongs to one compositor run. Keyed on the
/// instance signature where there is one, since socket names like
/// `wayland-1` come back with the next session.
pub fn compositor_file(name: &str) -> PathBuf {
    let id = ["HYPRLAND_INSTANCE_SIGNATURE", "MANGO_INSTANCE_SIGNATURE", "WAYLAND_DISPLAY"]
        .into_iter()
        .find_map(|k| std::env::var(k).ok().filter(|v| !v.is_empty()))
        .unwrap_or_default();
    let dir = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from).unwrap_or_else(std::env::temp_dir).join("formalshell");
    dir.join(format!("{name}-{}", id.replace('/', "_")))
}

/// Replaces a live shell if there is one, then holds the lock on a thread
/// of its own for the life of the process.
pub fn acquire() {
    let path = socket_path();
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(stream) = UnixStream::connect(&path) {
        take_over(stream);
    }
    let _ = std::fs::remove_file(&path);
    let listener = match UnixListener::bind(&path) {
        Ok(listener) => listener,
        Err(err) => {
            eprintln!("formalshell: instance lock: bind failed ({err}), continuing without single-instance guarantee");
            return;
        }
    };
    log(&format!("acquired at {} (pid {})", path.display(), std::process::id()));
    let spawned = std::thread::Builder::new().name("fs-instance".into()).spawn(move || serve(listener));
    if let Err(err) = spawned {
        eprintln!("formalshell: instance lock: {err}");
    }
}

fn take_over(stream: UnixStream) {
    let deadline = Instant::now() + TAKEOVER_WAIT;
    let _ = stream.set_read_timeout(Some(TAKEOVER_WAIT));
    let mut reader = BufReader::new(&stream);
    let mut line = String::new();
    if reader.read_line(&mut line).is_err() || !line.starts_with("formalshell ") {
        return;
    }
    log("live instance found, requesting takeover");
    let _ = (&stream).write_all(format!("takeover {}\n", std::process::id()).as_bytes());
    // The old shell's end closes when it exits.
    let mut rest = String::new();
    while Instant::now() < deadline {
        let left = deadline.saturating_duration_since(Instant::now()).max(Duration::from_millis(1));
        let _ = stream.set_read_timeout(Some(left));
        rest.clear();
        match reader.read_line(&mut rest) {
            Ok(0) => return,
            Ok(_) => {}
            Err(err) if matches!(err.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => return,
            Err(_) => return,
        }
    }
}

fn serve(listener: UnixListener) {
    for stream in listener.incoming() {
        let Ok(mut stream) = stream else { continue };
        if stream.write_all(format!("formalshell {}\n", std::process::id()).as_bytes()).is_err() {
            continue;
        }
        let _ = stream.set_read_timeout(Some(TAKEOVER_WAIT));
        let mut line = String::new();
        if BufReader::new(&stream).read_line(&mut line).is_ok() && line.starts_with("takeover") {
            log(&format!("being replaced ({}), quitting", line.trim_end()));
            std::process::exit(0);
        }
    }
}
