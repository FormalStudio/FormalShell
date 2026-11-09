//! The screensaver's ttfx children: a PATH probe at startup, then a run per
//! output, each one's stdout split into frames here so the UI thread only
//! ever receives parsed rows. Stopping a run drops it, child included, so no
//! stale frame or exit from it can arrive.

use std::cell::RefCell;
use std::collections::HashMap;

use fs_screensaver::ttfx::{self, Frame, FrameParser};
use futures_lite::{AsyncReadExt, FutureExt};

use crate::runtime::Ctx;
use crate::services::proc;
use crate::services::proc::Avail;
use crate::store;

/// How a run ended on its own: `frames` counts the animation frames it
/// produced, ttfx's canvas prep not among them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct End {
    pub run: u64,
    pub code: i32,
    pub frames: usize,
}

#[derive(Default)]
pub struct State {
    pub ttfx: Avail,
    /// The newest frame of each run: its index in that run, its rows.
    pub frames: HashMap<u64, (usize, Frame)>,
    pub ended: Vec<End>,
    /// The banner file read, its text: `screensaver.asciiPath`, or the
    /// bundled one when that is unset or unreadable.
    pub banner: Option<(String, String)>,
}

pub enum Diff {
    Ttfx(Avail),
    Banner(String, String),
    Frame(u64, usize, Frame),
    Ended(End),
}

impl State {
    pub fn apply(&mut self, diff: Diff) -> bool {
        match diff {
            Diff::Ttfx(v) => std::mem::replace(&mut self.ttfx, v) != v,
            Diff::Banner(path, text) => {
                let banner = Some((path, text));
                let changed = self.banner != banner;
                self.banner = banner;
                changed
            }
            Diff::Frame(run, index, frame) => {
                self.frames.insert(run, (index, frame));
                true
            }
            Diff::Ended(end) => {
                self.ended.push(end);
                true
            }
        }
    }
}

fn publish(ctx: &Ctx, diff: Diff) {
    ctx.publish(store::Diff::Screensaver(diff));
}

pub fn start(ctx: &Ctx) {
    ctx.spawn(run(ctx.clone()));
}

async fn run(ctx: Ctx) {
    let probe = proc::capture(&proc::argv(&["sh", "-c", "command -v ttfx >/dev/null 2>&1"]), std::time::Duration::from_secs(10)).await;
    publish(&ctx, Diff::Ttfx(if probe.code == 0 { Avail::Available } else { Avail::Missing }));
}

/// The bundled banner: the package's copy, else the repo's own.
pub fn bundled_banner() -> String {
    match std::env::var("FS_BRANDING_DIR") {
        Ok(dir) => format!("{dir}/screensaver.txt"),
        Err(_) => "branding/screensaver.txt".into(),
    }
}

pub fn load_banner(ctx: &Ctx, configured: String) {
    let task_ctx = ctx.clone();
    ctx.spawn(async move {
        let read = move || {
            let bundled = bundled_banner();
            if !configured.is_empty() {
                match std::fs::read_to_string(&configured) {
                    Ok(text) => return Some((configured, text)),
                    Err(err) => eprintln!(
                        "screensaver: failed to load screensaver.asciiPath ({configured}): {err}, falling back to the bundled banner"
                    ),
                }
            }
            match std::fs::read_to_string(&bundled) {
                Ok(text) => Some((bundled, text)),
                Err(err) => {
                    eprintln!("screensaver: bundled banner failed to load at {bundled}: {err}");
                    None
                }
            }
        };
        if let Some(Some((path, text))) = task_ctx.pool().run(read).await {
            publish(&task_ctx, Diff::Banner(path, text));
        }
    });
}

thread_local! {
    /// The runs going, by run; dropping a sender ends the run holding its
    /// receiver.
    static RUNS: RefCell<HashMap<u64, async_channel::Sender<()>>> = RefCell::new(HashMap::new());
}

pub fn stop(_: &Ctx) {
    RUNS.with(|c| c.borrow_mut().clear());
}

pub fn stop_run(_: &Ctx, run: u64) {
    RUNS.with(|c| c.borrow_mut().remove(&run));
}

/// `pinned`: only that frame is sent, and the run races there unpaced,
/// stopping at [`ttfx::PIN_FRAME_CAP`].
pub fn play(ctx: &Ctx, run: u64, opts: ttfx::Opts, pinned: Option<usize>) {
    let (tx, rx) = async_channel::bounded::<()>(1);
    RUNS.with(|c| c.borrow_mut().insert(run, tx));
    let task_ctx = ctx.clone();
    ctx.spawn(async move {
        let cancelled = async {
            let _ = rx.recv().await;
            None
        };
        if let Some(end) = stream(&task_ctx, run, opts, pinned).or(cancelled).await {
            RUNS.with(|c| c.borrow_mut().remove(&run));
            publish(&task_ctx, Diff::Ended(end));
        }
    });
}

async fn stream(ctx: &Ctx, run: u64, opts: ttfx::Opts, pinned: Option<usize>) -> Option<End> {
    let rows = opts.rows;
    let argv = ttfx::command(&opts);
    let child = crate::services::proc::command(&argv[0])
        .args(&argv[1..])
        .stdin(async_process::Stdio::null())
        .stdout(async_process::Stdio::piped())
        .stderr(async_process::Stdio::null())
        .kill_on_drop(true)
        .spawn();
    let mut child = match child {
        Ok(child) => child,
        Err(err) => {
            eprintln!("screensaver: ttfx: {err}");
            return Some(End { run, code: proc::MISSING, frames: 0 });
        }
    };
    let mut stdout = child.stdout.take()?;
    let mut parser = FrameParser::new(rows);
    let mut chunks = 0usize;
    let mut buf = vec![0u8; 1 << 16];
    // The first segment is ttfx's canvas prep, not a frame.
    let take = |frame: Frame, chunks: &mut usize| -> Option<End> {
        let index = *chunks;
        *chunks += 1;
        if index == 0 {
            return None;
        }
        match pinned {
            Some(p) => {
                if index - 1 == p {
                    publish(ctx, Diff::Frame(run, p, frame));
                }
                (index >= ttfx::PIN_FRAME_CAP).then_some(End { run, code: 0, frames: index })
            }
            None => {
                publish(ctx, Diff::Frame(run, index - 1, frame));
                None
            }
        }
    };
    loop {
        let n = stdout.read(&mut buf).await.unwrap_or(0);
        if n == 0 {
            break;
        }
        for frame in parser.feed(&buf[..n]) {
            if let Some(end) = take(frame, &mut chunks) {
                return Some(end);
            }
        }
    }
    if let Some(frame) = parser.finish() {
        if let Some(end) = take(frame, &mut chunks) {
            return Some(end);
        }
    }
    let code = child.status().await.ok().and_then(|s| s.code()).unwrap_or(-1);
    Some(End { run, code, frames: chunks.saturating_sub(1) })
}
