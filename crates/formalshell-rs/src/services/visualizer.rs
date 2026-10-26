//! The one shared `cava` child behind the bar's
//! spectrum cell and the media panel's band.
//!
//! The process runs only while a track is playing, motion is on and
//! something wants the levels (a bar showing the cell, or the open panel):
//! paused music is a dead process. The iPhone's audio plays on the phone, so
//! cava would hear nothing of it; its frame is drawn off the track's tempo
//! instead (the one exception CLAUDE.md names), and only the bpm lookup
//! lives here.
//!
//! Frames arrive at cava's own 120 Hz, far above what the store should be
//! asked to apply, so the latest one sits in a lock of its own that the
//! drawing cell reads each screen frame. The store only carries what changes
//! rarely: whether cava exists, whether anything runs, the bpm.

use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;
use std::cell::RefCell;
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

use futures_lite::io::{AsyncBufReadExt, BufReader};
use futures_lite::{FutureExt, StreamExt};
use fs_media::visualizer::model::{self, BAR_COUNT, MAX_LEVEL};

use crate::runtime::Ctx;
use crate::store;

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

#[derive(Clone, Debug, Default, PartialEq)]
pub struct State {
    pub avail: Avail,
    /// cava is up, or the tempo clock is.
    pub running: bool,
    pub bpm: f64,
    /// `visualizer style`'s in-memory pick, "" following settings.json.
    pub style_override: String,
}

pub enum Diff {
    Avail(Avail),
    Running(bool),
    Bpm(f64),
    Style(String),
}

impl State {
    pub fn apply(&mut self, diff: Diff) -> bool {
        match diff {
            Diff::Avail(v) => std::mem::replace(&mut self.avail, v) != v,
            Diff::Running(v) => std::mem::replace(&mut self.running, v) != v,
            Diff::Bpm(v) => std::mem::replace(&mut self.bpm, v) != v,
            Diff::Style(v) => {
                let changed = self.style_override != v;
                self.style_override = v;
                changed
            }
        }
    }

    /// The style drawn: the override, else the configured one when known,
    /// else `bars`.
    pub fn style(&self, configured: &str) -> String {
        if !self.style_override.is_empty() {
            return self.style_override.clone();
        }
        if fs_media::visualizer::styles::is_known(configured) { configured.to_owned() } else { "bars".to_owned() }
    }
}

/// One cava frame after the gain stage: 0..1 per band, the mix and each
/// channel.
#[derive(Clone, Debug, PartialEq)]
pub struct Frame {
    pub mono: Vec<f64>,
    pub left: Vec<f64>,
    pub right: Vec<f64>,
}

impl Frame {
    pub fn baseline() -> Self {
        Self { mono: model::baseline_levels(), left: model::baseline_levels(), right: model::baseline_levels() }
    }
}

static FRAME: Mutex<Option<Frame>> = Mutex::new(None);

/// The latest frame cava sent, the baseline while nothing runs.
pub fn frame() -> Frame {
    FRAME.lock().ok().and_then(|f| f.clone()).unwrap_or_else(Frame::baseline)
}

fn set_frame(frame: Frame) {
    if let Ok(mut slot) = FRAME.lock() {
        *slot = Some(frame);
    }
}

/// Everything the run gate reads, kept by whoever knows it: the UI thread
/// sets what the media pick, the config and the open surfaces say.
#[derive(Clone, Debug, Default, PartialEq)]
struct Gate {
    playing: bool,
    tempo: bool,
    motion: bool,
    bars: usize,
    panel: bool,
    /// The artist and title the bpm is looked up for, while tempo driven.
    track: Option<(String, String)>,
}

impl Gate {
    fn wanted(&self) -> bool {
        self.playing && self.motion && (self.bars > 0 || self.panel)
    }
}

static GATE: Mutex<Gate> = Mutex::new(Gate { playing: false, tempo: false, motion: true, bars: 0, panel: false, track: None });
static GATE_TX: OnceLock<async_channel::Sender<Gate>> = OnceLock::new();

fn update(f: impl FnOnce(&mut Gate)) {
    let Ok(mut gate) = GATE.lock() else { return };
    let before = gate.clone();
    f(&mut gate);
    if *gate != before
        && let Some(tx) = GATE_TX.get()
    {
        let _ = tx.try_send(gate.clone());
    }
}

/// MediaService's pick, as the run gate reads it.
pub fn set_media(playing: bool, tempo: bool, artist: &str, title: &str) {
    let track = (tempo && !title.is_empty()).then(|| (artist.to_owned(), title.to_owned()));
    update(|g| {
        g.playing = playing;
        g.tempo = tempo;
        g.track = track;
    });
}

pub fn set_motion(on: bool) {
    update(|g| g.motion = on);
}

/// A bar window showing the cell came on screen (+1) or left it (-1).
pub fn bar_visible(delta: isize) {
    update(|g| g.bars = g.bars.saturating_add_signed(delta));
}

/// The media panel is open with its spectrum band on.
#[allow(dead_code)]
pub fn set_panel(open: bool) {
    update(|g| g.panel = open);
}

fn runtime_dir() -> PathBuf {
    let base = std::env::var("XDG_RUNTIME_DIR").ok().filter(|d| !d.is_empty()).unwrap_or_else(|| "/tmp".into());
    PathBuf::from(base).join("formalshell")
}

/// Every key is in cava 0.10.7's own example config. Fixed gain rather than
/// autosens (model's gain stage supplies the height), 12 kHz upper cutoff to
/// pull hats into the top band, light smoothing so six bars track transients,
/// stereo so the two channels keep their own frame. FS_CAVA_SOURCE names a
/// node to read instead of the default sink's monitor (the smoke rig's null
/// sink).
fn conf_text() -> String {
    let source = std::env::var("FS_CAVA_SOURCE").ok().filter(|s| !s.is_empty()).unwrap_or_else(|| "auto".into());
    format!(
        "[general]\nframerate = 120\nautosens = 0\nsensitivity = 200\nbars = {}\nlower_cutoff_freq = 50\n\
         higher_cutoff_freq = 12000\nsleep_timer = 3\n\n[input]\nmethod = pipewire\nsource = {source}\n\n\
         [output]\nmethod = raw\nchannels = stereo\nraw_target = /dev/stdout\ndata_format = ascii\n\
         ascii_max_range = {}\n\n[smoothing]\nnoise_reduction = 35\nmonstercat = 1.5\n",
        BAR_COUNT * 2,
        MAX_LEVEL as i64
    )
}

/// `setpriv` hands the child a parent-death signal, the shell's own
/// guarantee that a crash never leaves cava behind.
fn cava_command(conf: &std::path::Path) -> async_process::Command {
    let mut cmd = crate::services::proc::command("setpriv");
    cmd.args(["--pdeathsig", "TERM", "--", "sh", "-c", "command -v cava >/dev/null 2>&1 || exit 127; exec cava -p \"$1\"", "sh"])
        .arg(conf)
        .stdin(async_process::Stdio::null())
        .stdout(async_process::Stdio::piped())
        .stderr(async_process::Stdio::null())
        .kill_on_drop(true);
    cmd
}

struct Cava {
    child: async_process::Child,
    lines: futures_lite::io::Lines<BufReader<async_process::ChildStdout>>,
    agc: f64,
    at: Option<Instant>,
}

impl Cava {
    fn spawn(conf: &std::path::Path) -> Option<Self> {
        let mut child = cava_command(conf).spawn().ok()?;
        let lines = BufReader::new(child.stdout.take()?).lines();
        Some(Self { child, lines, agc: 0.0, at: None })
    }

    fn frame(&mut self, line: &str) {
        let raw = model::stereo_frame_to_levels(Some(line), BAR_COUNT, MAX_LEVEL);
        let now = Instant::now();
        let dt = self.at.map_or(0.0, |at| now.duration_since(at).as_secs_f64());
        // One running peak off the mix, so the channels keep their balance.
        self.agc = model::agc_step(self.agc, model::frame_peak(&raw.mono), dt);
        self.at = Some(now);
        set_frame(Frame {
            mono: model::level_frame(&raw.mono, self.agc),
            left: model::level_frame(&raw.left, self.agc),
            right: model::level_frame(&raw.right, self.agc),
        });
    }

    fn stop(mut self) {
        let _ = self.child.kill();
    }
}

enum Event {
    Gate(Option<Gate>),
    Line(Option<String>),
}

async fn curl(url: &str) -> String {
    let out = crate::services::proc::command("curl")
        .args(["-sS", "--fail", "--max-time", "5", url])
        .stdin(async_process::Stdio::null())
        .stderr(async_process::Stdio::null())
        .kill_on_drop(true)
        .output()
        .await;
    match out {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).into_owned(),
        _ => String::new(),
    }
}

/// Deezer's bpm for a track, looked up once per track per session.
struct Bpm {
    cache: Rc<RefCell<HashMap<(String, String), f64>>>,
    current: Rc<RefCell<Option<(String, String)>>>,
}

impl Bpm {
    fn track(&self, ctx: &Ctx, track: Option<(String, String)>) {
        *self.current.borrow_mut() = track.clone();
        let Some(key) = track else {
            ctx.publish(store::Diff::Visualizer(Diff::Bpm(0.0)));
            return;
        };
        if let Some(bpm) = self.cache.borrow().get(&key) {
            ctx.publish(store::Diff::Visualizer(Diff::Bpm(*bpm)));
            return;
        }
        ctx.publish(store::Diff::Visualizer(Diff::Bpm(0.0)));
        let (cache, current, ctx2) = (self.cache.clone(), self.current.clone(), ctx.clone());
        ctx.spawn(async move {
            let id = model::parse_deezer_search(&curl(&model::deezer_search_url(&key.0, &key.1)).await);
            let bpm = if id.is_empty() { 0.0 } else { model::parse_deezer_bpm(&curl(&model::deezer_track_url(&id)).await) };
            cache.borrow_mut().insert(key.clone(), bpm);
            if current.borrow().as_ref() == Some(&key) {
                ctx2.publish(store::Diff::Visualizer(Diff::Bpm(bpm)));
            }
        });
    }
}

pub async fn run(ctx: Ctx) {
    let (tx, rx) = async_channel::unbounded();
    if GATE_TX.set(tx).is_err() {
        return;
    }
    let dir = runtime_dir();
    let conf = dir.join("cava.conf");
    let probe = async {
        std::fs::create_dir_all(&dir).ok()?;
        std::fs::write(&conf, conf_text()).ok()?;
        let status = crate::services::proc::command("sh")
            .args(["-c", "command -v cava >/dev/null 2>&1"])
            .stdin(async_process::Stdio::null())
            .status()
            .await
            .ok()?;
        status.success().then_some(())
    };
    let avail = if probe.await.is_some() { Avail::Available } else { Avail::Missing };
    ctx.publish(store::Diff::Visualizer(Diff::Avail(avail)));

    let bpm = Bpm { cache: Rc::default(), current: Rc::default() };
    let mut gate = GATE.lock().map(|g| g.clone()).unwrap_or_default();
    let mut cava: Option<Cava> = None;
    let mut exited = false;
    let mut running = false;
    let mut track: Option<(String, String)> = None;
    loop {
        let wanted = gate.wanted();
        let want_cava = avail == Avail::Available && wanted && !gate.tempo && !exited;
        match (want_cava, cava.is_some()) {
            (true, false) => match Cava::spawn(&conf) {
                Some(c) => cava = Some(c),
                None => exited = true,
            },
            (false, true) => {
                if let Some(c) = cava.take() {
                    c.stop();
                }
            }
            _ => {}
        }
        let tempo_running = gate.tempo && wanted;
        let now_running = cava.is_some() || tempo_running;
        if now_running != running {
            running = now_running;
            if !running {
                set_frame(Frame::baseline());
            }
            ctx.publish(store::Diff::Visualizer(Diff::Running(running)));
        }
        let wanted_track = if tempo_running { gate.track.clone() } else { None };
        if wanted_track != track {
            track = wanted_track.clone();
            bpm.track(&ctx, wanted_track);
        }

        let event = match cava.as_mut() {
            Some(c) => {
                let next_gate = async { Event::Gate(rx.recv().await.ok()) };
                let next_line = async { Event::Line(c.lines.next().await.and_then(Result::ok)) };
                next_gate.or(next_line).await
            }
            None => Event::Gate(rx.recv().await.ok()),
        };
        match event {
            Event::Gate(Some(next)) => {
                gate = next;
                exited = false;
            }
            Event::Gate(None) => return,
            Event::Line(Some(line)) => {
                if let Some(c) = cava.as_mut() {
                    c.frame(&line);
                }
            }
            Event::Line(None) => {
                // cava went away on its own: not restarted until the gate moves.
                cava = None;
                exited = true;
            }
        }
    }
}
