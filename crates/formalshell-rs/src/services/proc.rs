//! A child a service runs once and reads whole: argv in, exit code and
//! stdout out. A missing binary reads as exit 127, the way `sh -c 'command
//! -v x || exit 127'` reports it, so a poll has one failure vocabulary.

use std::io::ErrorKind;
use std::time::Duration;

use async_io::Timer;
use futures_lite::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, FutureExt};

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

/// Whether `name` would run: a path that is an executable file, or a bare
/// name found as one in one of PATH's directories.
pub fn on_path(name: &str) -> bool {
    if name.contains('/') {
        return executable(std::path::Path::new(name));
    }
    std::env::var_os("PATH").is_some_and(|p| std::env::split_paths(&p).any(|d| executable(&d.join(name))))
}

fn executable(path: &std::path::Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
}

pub struct Done {
    pub code: i32,
    pub stdout: String,
    /// Empty unless the caller asked for it ([`Run::stderr`]).
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

/// One child run to its end: argv in, exit code and output out. Dropping
/// the future kills the child.
pub struct Run<'a, F = fn(u32)> {
    argv: &'a [String],
    timeout: Duration,
    env: Vec<(String, String)>,
    stdin: Option<Vec<u8>>,
    stderr: bool,
    until_exit: bool,
    on_spawn: Option<F>,
}

impl<'a> Run<'a> {
    pub fn new(argv: &'a [String], timeout: Duration) -> Self {
        Self { argv, timeout, env: Vec::new(), stdin: None, stderr: false, until_exit: false, on_spawn: None }
    }
}

impl<'a, F: FnOnce(u32)> Run<'a, F> {

    pub fn env(mut self, env: impl IntoIterator<Item = (String, String)>) -> Self {
        self.env.extend(env);
        self
    }

    /// Written to the child's stdin, which is then closed.
    pub fn stdin(mut self, bytes: impl Into<Vec<u8>>) -> Self {
        self.stdin = Some(bytes.into());
        self
    }

    /// Reads stderr too (a failure whose words pick the state).
    pub fn stderr(mut self) -> Self {
        self.stderr = true;
        self
    }

    /// Done when the child exits rather than when its pipes close. For a
    /// child that leaves a daemon behind holding them: `wl-copy` forks one
    /// that serves the clipboard until the next copy.
    pub fn until_exit(mut self) -> Self {
        self.until_exit = true;
        self
    }

    /// Called with the pid once the child has started.
    pub fn on_spawn<G: FnOnce(u32)>(self, f: G) -> Run<'a, G> {
        let Run { argv, timeout, env, stdin, stderr, until_exit, .. } = self;
        Run { argv, timeout, env, stdin, stderr, until_exit, on_spawn: Some(f) }
    }

    pub async fn run(self) -> Done {
        let none = |code| Done { code, stdout: String::new(), stderr: String::new() };
        let Some((program, args)) = self.argv.split_first() else { return none(MISSING) };
        let child = command(program)
            .args(args)
            .envs(self.env)
            .stdin(if self.stdin.is_some() { async_process::Stdio::piped() } else { async_process::Stdio::null() })
            .stdout(async_process::Stdio::piped())
            .stderr(if self.stderr { async_process::Stdio::piped() } else { async_process::Stdio::null() })
            .kill_on_drop(true)
            .spawn();
        let mut child = match child {
            Ok(child) => child,
            Err(err) => {
                let code = if err.kind() == ErrorKind::NotFound { MISSING } else { -1 };
                return none(code);
            }
        };
        if let Some(f) = self.on_spawn {
            f(child.id());
        }
        let mut input = child.stdin.take();
        let mut stdout = child.stdout.take();
        let mut stderr = child.stderr.take();
        let until_exit = self.until_exit;
        let feed = self.stdin;
        let run = async {
            if let (Some(pipe), Some(bytes)) = (input.as_mut(), feed) {
                let _ = pipe.write_all(&bytes).await;
            }
            drop(input);
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
                if until_exit {
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
            Timer::after(self.timeout).await;
            none(TIMED_OUT)
        };
        run.or(expired).await
    }
}

pub async fn capture(argv: &[String], timeout: Duration) -> Done {
    Run::new(argv, timeout).run().await
}

pub async fn capture_err(argv: &[String], timeout: Duration) -> Done {
    Run::new(argv, timeout).stderr().run().await
}

pub async fn capture_exit(argv: &[String], timeout: Duration) -> Done {
    Run::new(argv, timeout).stderr().until_exit().run().await
}

/// What a child wrote before it exited is already in the pipe; this is
/// only the readers' turn to take it.
const DRAIN: Duration = Duration::from_millis(100);

/// How a [`Supervised`] child's backoff goes back to its floor.
#[derive(Clone, Copy, Debug)]
pub enum Reset {
    /// At every start.
    OnStart,
    /// After a run that lasted at least this long.
    After(Duration),
    /// Only when the caller calls [`Supervised::reset`] (a health signal).
    Manual,
}

/// Which of a supervised child's pipes is read line by line.
#[derive(Clone, Copy, Debug)]
pub enum Read {
    Stdout,
    Stderr,
}

pub enum Event {
    Line(String),
    /// The child is gone and a retry is due after the backoff.
    Exit(std::io::Result<std::process::ExitStatus>),
    /// The backoff after an exit or a failed start is over.
    Retry,
}

type Lines = futures_lite::io::Lines<futures_lite::io::BufReader<Box<dyn futures_lite::AsyncRead + Unpin>>>;

/// A long-running child kept up while it is wanted: its lines read, and
/// started again on a doubling backoff once it exits.
pub struct Supervised {
    backoff: fs_types::Backoff,
    reset: Reset,
    child: Option<(async_process::Child, Lines, std::time::Instant)>,
    retry_at: Option<std::time::Instant>,
}

impl Supervised {
    pub fn new(floor: Duration, ceiling: Duration, reset: Reset) -> Self {
        Self { backoff: fs_types::Backoff::new(floor, ceiling), reset, child: None, retry_at: None }
    }

    pub fn running(&self) -> bool {
        self.child.is_some()
    }

    /// Neither running nor waiting out a backoff.
    pub fn idle(&self) -> bool {
        self.child.is_none() && self.retry_at.is_none()
    }

    pub fn reset(&mut self) {
        self.backoff.reset();
    }

    /// Spawns `cmd` with `read` piped and killed on drop, handing back its
    /// stdin when `cmd` piped one. A start that fails waits out the backoff.
    pub fn start(&mut self, mut cmd: async_process::Command, read: Read) -> std::io::Result<Option<async_process::ChildStdin>> {
        if let Reset::OnStart = self.reset {
            self.backoff.reset();
        }
        match read {
            Read::Stdout => cmd.stdout(async_process::Stdio::piped()),
            Read::Stderr => cmd.stderr(async_process::Stdio::piped()),
        };
        let spawned = cmd.kill_on_drop(true).spawn().and_then(|mut child| {
            let pipe: Option<Box<dyn futures_lite::AsyncRead + Unpin>> = match read {
                Read::Stdout => child.stdout.take().map(|p| Box::new(p) as _),
                Read::Stderr => child.stderr.take().map(|p| Box::new(p) as _),
            };
            let pipe = pipe.ok_or_else(|| std::io::Error::other("no pipe"))?;
            Ok((child, pipe))
        });
        match spawned {
            Ok((mut child, pipe)) => {
                let stdin = child.stdin.take();
                self.child = Some((child, futures_lite::io::BufReader::new(pipe).lines(), std::time::Instant::now()));
                Ok(stdin)
            }
            Err(err) => {
                self.retry_at = Some(std::time::Instant::now() + self.backoff.delay());
                Err(err)
            }
        }
    }

    /// Kills the child and drops any pending retry.
    pub fn stop(&mut self) {
        self.child = None;
        self.retry_at = None;
    }

    /// Waits up to `grace` for the child to exit on its own (its stdin
    /// closed), then kills it.
    pub async fn stop_within(&mut self, grace: Duration) {
        if let Some((child, ..)) = self.child.as_mut() {
            let _ = child
                .status()
                .or(async {
                    Timer::after(grace).await;
                    Err(ErrorKind::TimedOut.into())
                })
                .await;
        }
        self.stop();
    }

    /// The next line, exit or retry; pending while idle.
    pub async fn next(&mut self) -> Event {
        let Some((child, lines, started)) = self.child.as_mut() else {
            return match self.retry_at {
                Some(at) => {
                    Timer::at(at).await;
                    self.retry_at = None;
                    Event::Retry
                }
                None => std::future::pending().await,
            };
        };
        if let Some(Ok(line)) = futures_lite::StreamExt::next(lines).await {
            return Event::Line(line);
        }
        // A closed pipe comes before the exit; the exit is awaited here so
        // the next start never races the old child.
        let status = child.status().await;
        if let Reset::After(stable) = self.reset
            && started.elapsed() >= stable
        {
            self.backoff.reset();
        }
        self.child = None;
        self.retry_at = Some(std::time::Instant::now() + self.backoff.delay());
        Event::Exit(status)
    }

    /// The wait the next retry will take, for a log line.
    pub fn retry_in(&self) -> Duration {
        self.retry_at.map_or(Duration::ZERO, |at| at.saturating_duration_since(std::time::Instant::now()))
    }
}

/// An exit's code, -1 for one a signal ended or that could not be read.
pub fn exit_code(status: &std::io::Result<std::process::ExitStatus>) -> i32 {
    status.as_ref().ok().and_then(|s| s.code()).unwrap_or(-1)
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn a_file_without_an_execute_bit_does_not_run() {
        let dir = tempfile::tempdir().unwrap();
        let tool = dir.path().join("tool");
        std::fs::write(&tool, "#!/bin/sh\n").unwrap();
        std::fs::set_permissions(&tool, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(!super::on_path(tool.to_str().unwrap()));
        std::fs::set_permissions(&tool, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(super::on_path(tool.to_str().unwrap()));
        assert!(!super::on_path(dir.path().to_str().unwrap()));
    }
}
