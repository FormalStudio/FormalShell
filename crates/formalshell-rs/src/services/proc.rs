//! A child a service runs once and reads whole: argv in, exit code and
//! stdout out. A missing binary reads as exit 127, the way `sh -c 'command
//! -v x || exit 127'` reports it, so a poll has one failure vocabulary.

use std::io::ErrorKind;
use std::time::Duration;

use async_io::Timer;
use futures_lite::{AsyncReadExt, FutureExt};

pub const MISSING: i32 = 127;
pub const TIMED_OUT: i32 = 124;

/// Whether a program the shell shells out to is on PATH, as a one-shot
/// probe reports it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Avail {
    /// Until the one-shot PATH probe answers.
    #[default]
    Unknown,
    Available,
    Missing,
}

impl Avail {
    pub fn as_str(self) -> &'static str {
        match self {
            Avail::Unknown => "unknown",
            Avail::Available => "available",
            Avail::Missing => "missing",
        }
    }
}

pub struct Done {
    pub code: i32,
    pub stdout: String,
    /// Empty unless the caller asked for it ([`capture_err`]).
    pub stderr: String,
}

/// Every child the shell starts: SIGTERM'd when the thread that spawned it
/// goes (`PR_SET_PDEATHSIG`; services spawn from the service thread, which
/// lives as long as the process), and in the shell's process group, which
/// the shell signals on its way out (`main`'s `reap_children`).
pub fn std_command(program: impl AsRef<std::ffi::OsStr>) -> std::process::Command {
    use std::os::unix::process::CommandExt;
    let mut cmd = std::process::Command::new(program);
    // SAFETY: prctl is async-signal-safe and touches nothing of the parent.
    unsafe {
        cmd.pre_exec(|| {
            libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM);
            Ok(())
        });
    }
    cmd
}

/// [`std_command`] for the service thread's executor.
pub fn command(program: impl AsRef<std::ffi::OsStr>) -> async_process::Command {
    std_command(program).into()
}

pub fn argv(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|p| (*p).to_owned()).collect()
}

/// Dropping the future kills the child.
pub async fn capture(argv: &[String], timeout: Duration) -> Done {
    run(argv, timeout, false, false).await
}

/// [`capture`], reading stderr too (a failure whose words pick the state).
pub async fn capture_err(argv: &[String], timeout: Duration) -> Done {
    run(argv, timeout, true, false).await
}

/// [`capture_err`], done when the child exits rather than when its pipes
/// close. For a child that leaves a daemon behind holding them: `wl-copy`
/// forks one that serves the clipboard until the next copy, with stdout on
/// /dev/null but stderr still the pipe.
pub async fn capture_exit(argv: &[String], timeout: Duration) -> Done {
    run(argv, timeout, true, true).await
}

/// What a child wrote before it exited is already in the pipe; this is
/// only the readers' turn to take it.
const DRAIN: Duration = Duration::from_millis(100);

async fn run(argv: &[String], timeout: Duration, err: bool, on_exit: bool) -> Done {
    let none = |code| Done { code, stdout: String::new(), stderr: String::new() };
    let Some((program, args)) = argv.split_first() else { return none(MISSING) };
    let child = command(program)
        .args(args)
        .stdin(async_process::Stdio::null())
        .stdout(async_process::Stdio::piped())
        .stderr(if err { async_process::Stdio::piped() } else { async_process::Stdio::null() })
        .kill_on_drop(true)
        .spawn();
    let mut child = match child {
        Ok(child) => child,
        Err(err) => {
            let code = if err.kind() == ErrorKind::NotFound { MISSING } else { -1 };
            return none(code);
        }
    };
    let mut stdout = child.stdout.take();
    let mut stderr = child.stderr.take();
    let run = async {
        let (mut raw, mut errs) = (Vec::new(), Vec::new());
        let mut exited = None;
        {
            let out = async {
                if let Some(out) = stdout.as_mut() {
                    let _ = out.read_to_end(&mut raw).await;
                }
            };
            let error = async {
                if let Some(e) = stderr.as_mut() {
                    let _ = e.read_to_end(&mut errs).await;
                }
            };
            let read = async {
                futures_lite::future::zip(out, error).await;
            };
            if on_exit {
                let exit = async {
                    exited = Some(child.status().await);
                    Timer::after(DRAIN).await;
                };
                read.or(exit).await;
            } else {
                read.await;
            }
        }
        let status = match exited {
            Some(status) => status,
            None => child.status().await,
        };
        let code = status.ok().and_then(|s| s.code()).unwrap_or(-1);
        Done { code, stdout: String::from_utf8_lossy(&raw).into_owned(), stderr: String::from_utf8_lossy(&errs).into_owned() }
    };
    let expired = async {
        Timer::after(timeout).await;
        none(TIMED_OUT)
    };
    run.or(expired).await
}
