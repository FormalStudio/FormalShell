//! The children the capture family runs (slurp, grim, tesseract, wf-recorder,
//! ffmpeg, the editor), spawned and read on the service thread. Each one
//! reports its pid as it starts and its whole answer as it exits; the UI
//! thread owns every decision about both, in `surfaces::capture`.

use std::cell::RefCell;
use std::time::Duration;

use futures_lite::{AsyncReadExt, FutureExt};

use crate::runtime::Ctx;
use crate::store::Diff;

/// Which step a child is. The UI matches an answer to the step that is
/// still waiting for it by this and the run's generation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Job {
    ShotSlurp,
    ShotGrab,
    Editor,
    Freeze,
    GrabSlurp,
    Ocr,
    Pick,
    Copy,
    RecSlurp,
    Mkdir,
    Audio,
    WebcamList,
    Recorder,
    Unload,
    ProbeVideo,
    ProbeAudio,
    Finalize,
    FinalizeCleanup,
    Preview,
    Player,
    Palette,
    Gif,
    Quiet,
}

#[derive(Clone, Debug)]
pub struct Exit {
    pub job: Job,
    pub generation: u64,
    /// False when the binary never started (QProcess's FailedToStart).
    pub started: bool,
    /// The exit code, or -1 for a child a signal ended.
    pub code: i32,
    pub stdout: String,
    pub stderr: String,
    /// Whatever the caller tagged the run with (a freeze's output name).
    pub tag: String,
}

#[derive(Clone, Debug)]
pub enum Event {
    Spawned { generation: u64, pid: u32 },
    Exited(Exit),
    /// A freeze decoded off the UI thread.
    Frame { generation: u64, output: String, frame: Option<crate::scene::Bitmap> },
    /// A shot cut out of its freeze off the UI thread: the file, or why not.
    Cropped { generation: u64, result: Result<String, String> },
    /// A notification's picture, loaded off the UI thread.
    Image { id: String, raw: Option<crate::services::icons::Raw> },
}

/// Events waiting for the UI thread to act on them, in arrival order.
#[derive(Default)]
pub struct Inbox {
    pub events: Vec<Event>,
}

impl Inbox {
    pub fn apply(&mut self, event: Event) -> bool {
        self.events.push(event);
        true
    }
}

/// One child to run: argv, extra environment, and what to call it.
pub struct Run {
    pub job: Job,
    pub generation: u64,
    pub argv: Vec<String>,
    pub env: Vec<(String, String)>,
    pub tag: String,
}

impl Run {
    pub fn new(job: Job, generation: u64, argv: Vec<String>) -> Self {
        Self { job, generation, argv, env: Vec::new(), tag: String::new() }
    }

    pub fn env(mut self, key: &str, value: impl Into<String>) -> Self {
        self.env.push((key.to_owned(), value.into()));
        self
    }

    pub fn tag(mut self, tag: impl Into<String>) -> Self {
        self.tag = tag.into();
        self
    }

    /// `sh -c <script>`: values ride the
    /// environment, never the script text.
    pub fn sh(job: Job, generation: u64, script: &str) -> Self {
        Self::new(job, generation, vec!["sh".into(), "-c".into(), script.into()])
    }
}

pub fn start(ctx: &Ctx, run: Run) {
    ctx.spawn(child(ctx.clone(), run));
}

async fn child(ctx: Ctx, run: Run) {
    let Run { job, generation, argv, env, tag } = run;
    let exit = |started: bool, code: i32, stdout: String, stderr: String| {
        Diff::Capture(Event::Exited(Exit { job, generation, started, code, stdout, stderr, tag: tag.clone() }))
    };
    let Some((program, args)) = argv.split_first() else {
        ctx.publish(exit(false, -1, String::new(), String::new()));
        return;
    };
    let spawned = crate::services::proc::command(program)
        .args(args)
        .envs(env)
        .stdin(async_process::Stdio::null())
        .stdout(async_process::Stdio::piped())
        .stderr(async_process::Stdio::piped())
        .spawn();
    let mut child = match spawned {
        Ok(child) => child,
        Err(_) => {
            ctx.publish(exit(false, -1, String::new(), String::new()));
            return;
        }
    };
    ctx.publish(Diff::Capture(Event::Spawned { generation, pid: child.id() }));
    let (mut out, mut err) = (child.stdout.take(), child.stderr.take());
    let (buf_out, buf_err) = (RefCell::new(Vec::new()), RefCell::new(Vec::new()));
    // A child that forks a daemon onto its own pipes (wl-copy does) keeps
    // them open past its exit, so the pipes are read until they close or
    // until shortly after the child itself has gone, whichever is first.
    let finished = {
        let pumps = async {
            futures_lite::future::zip(pump(out.as_mut(), &buf_out), pump(err.as_mut(), &buf_err)).await;
            None
        };
        let status = async { Some(child.status().await.ok().and_then(|s| s.code()).unwrap_or(-1)) };
        pumps.or(status).await
    };
    let code = match finished {
        Some(code) => {
            let drain = async {
                futures_lite::future::zip(pump(out.as_mut(), &buf_out), pump(err.as_mut(), &buf_err)).await;
            };
            drain
                .or(async {
                    async_io::Timer::after(Duration::from_millis(200)).await;
                })
                .await;
            code
        }
        None => child.status().await.ok().and_then(|s| s.code()).unwrap_or(-1),
    };
    let text = |b: RefCell<Vec<u8>>| String::from_utf8_lossy(&b.into_inner()).into_owned();
    ctx.publish(exit(true, code, text(buf_out), text(buf_err)));
}

async fn pump<R: futures_lite::AsyncRead + Unpin>(reader: Option<&mut R>, into: &RefCell<Vec<u8>>) {
    let Some(reader) = reader else { return };
    let mut chunk = [0u8; 4096];
    loop {
        match reader.read(&mut chunk).await {
            Ok(0) | Err(_) => return,
            Ok(n) => into.borrow_mut().extend_from_slice(&chunk[..n]),
        }
    }
}

/// SIGTERM, which wf-recorder and slurp both treat as a graceful end.
pub fn terminate(pid: u32) {
    unsafe {
        libc::kill(pid as libc::pid_t, libc::SIGTERM);
    }
}

pub fn kill(pid: u32) {
    unsafe {
        libc::kill(pid as libc::pid_t, libc::SIGKILL);
    }
}
