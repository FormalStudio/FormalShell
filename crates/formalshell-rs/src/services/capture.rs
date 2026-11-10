//! The children the capture family runs (slurp, grim, tesseract, wf-recorder,
//! ffmpeg, the editor), spawned and read on the service thread. Each one
//! reports its pid as it starts and its whole answer as it exits; the UI
//! thread owns every decision about both, in `wayland::capture_host`. The
//! pool jobs that read and write files (a shot's crop, a notification's
//! picture) run here too.

use std::cell::Cell;
use std::time::Duration;

use crate::runtime::Ctx;
use crate::services::proc;
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
    Image { id: fs_types::NotificationId, raw: Option<crate::services::icons::Raw> },
}

/// What one of the family's notification actions does once picked.
#[derive(Clone, Debug)]
pub enum Act {
    Edit(String),
    Play(String),
    Gif(String),
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

    /// The default sink's monitor as the recording device, or with
    /// `with_mic` a null sink fed by that monitor and the default microphone
    /// through two loopbacks. Prints the loaded pactl module ids on the first
    /// line and the device on the second ([`parse_audio_setup`]); exits 4 when
    /// pactl cannot say, 5 when the default source is a monitor.
    pub fn audio(generation: u64, with_mic: bool) -> Self {
        let desktop = concat!(
            "sink=$(pactl get-default-sink) || exit 4\n",
            "[ -n \"$sink\" ] || exit 4\n",
            "printf '\\n%s.monitor\\n' \"$sink\"\n",
        );
        let desktop_mic = concat!(
            "sink=$(pactl get-default-sink) || exit 4\n",
            "src=$(pactl get-default-source) || exit 4\n",
            "[ -n \"$sink\" ] && [ -n \"$src\" ] || exit 4\n",
            "case \"$src\" in *.monitor) exit 5 ;; esac\n",
            "m1=$(pactl load-module module-null-sink sink_name=\"$FS_MIX\" sink_properties=device.description=\"$FS_MIX\") || exit 4\n",
            "m2=$(pactl load-module module-loopback source=\"$sink\".monitor sink=\"$FS_MIX\" latency_msec=50) || { pactl unload-module \"$m1\"; exit 4; }\n",
            "m3=$(pactl load-module module-loopback source=\"$src\" sink=\"$FS_MIX\" latency_msec=50) || { pactl unload-module \"$m2\"; pactl unload-module \"$m1\"; exit 4; }\n",
            "printf '%s %s %s\\n%s.monitor\\n' \"$m1\" \"$m2\" \"$m3\" \"$FS_MIX\"\n",
        );
        Self::sh(Job::Audio, generation, if with_mic { desktop_mic } else { desktop }).env("FS_MIX", fs_system::capture::MIX_SINK)
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
    let started = Cell::new(false);
    // A recording or an editor runs for as long as the user keeps it.
    let done = proc::Run::new(&argv, Duration::MAX)
        .env(env)
        .stderr()
        .until_exit()
        .on_spawn(|pid| {
            started.set(true);
            ctx.publish(Diff::Capture(Event::Spawned { generation, pid }));
        })
        .run()
        .await;
    let started = started.get();
    let code = if started { done.code } else { -1 };
    ctx.publish(Diff::Capture(Event::Exited(Exit { job, generation, started, code, stdout: done.stdout, stderr: done.stderr, tag })));
}

/// A shot's selection, in the freeze's own pixels, and where it goes.
pub struct Crop {
    pub src: String,
    pub dest: String,
    pub x0: u32,
    pub y0: u32,
    pub x1: u32,
    pub y1: u32,
}

/// Cuts the selection out of its freeze on the pool, landing as
/// [`Event::Cropped`]: the file, or why not.
pub fn crop(ctx: &Ctx, generation: u64, crop: Crop) {
    let Crop { src, dest, x0, y0, x1, y1 } = crop;
    let job = ctx.pool().run(move || -> Result<String, String> {
        if x1 <= x0 || y1 <= y0 {
            return Err("the selection is empty".into());
        }
        let img = image::open(&src).map_err(|e| format!("{src}: {e}"))?;
        if let Some(dir) = std::path::Path::new(&dest).parent() {
            std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        }
        img.crop_imm(x0, y0, x1 - x0, y1 - y0).save(&dest).map_err(|e| format!("{dest}: {e}"))?;
        Ok(dest)
    });
    let publisher = ctx.publisher().clone();
    ctx.spawn(async move {
        let result = job.await.unwrap_or_else(|| Err("the crop did not run".into()));
        publisher.publish(Diff::Capture(Event::Cropped { generation, result }));
    });
}

/// A notification's picture loaded on the pool, landing as [`Event::Image`].
pub fn picture(ctx: &Ctx, id: fs_types::NotificationId, path: String) {
    let job = ctx.pool().run(move || crate::services::icons::load(std::path::Path::new(&path)));
    let publisher = ctx.publisher().clone();
    ctx.spawn(async move {
        let raw = job.await.flatten();
        publisher.publish(Diff::Capture(Event::Image { id, raw }));
    });
}

/// A child's stderr as the reason it failed, else `fallback`.
pub fn or_code(text: &str, fallback: String) -> String {
    match text.trim() {
        "" => fallback,
        t => t.to_owned(),
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

#[derive(Debug, Clone, PartialEq)]
pub struct AudioSetup {
    pub modules: Vec<f64>,
    pub device: String,
}

/// [`Run::audio`]'s two-line answer: the pactl module ids it loaded (blank
/// when it loaded none) on the first line, the device wf-recorder should
/// record on the second. Anything else is a failed setup, reported as an empty
/// device so the caller never starts a recording that silently captures
/// nothing.
pub fn parse_audio_setup(text: &str) -> AudioSetup {
    let mut lines = text.split('\n');
    let mut modules = Vec::new();
    for part in lines.next().unwrap_or("").split_whitespace() {
        match part.parse::<u64>() {
            Ok(id) if part.bytes().all(|b| b.is_ascii_digit()) => modules.push(id as f64),
            _ => return AudioSetup { modules: Vec::new(), device: String::new() },
        }
    }
    AudioSetup { modules, device: lines.next().unwrap_or("").trim().to_owned() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_audio_setup_reads_modules_then_device() {
        let mic = parse_audio_setup("41 42 43\nformalshell-mix.monitor\n");
        assert_eq!(mic.modules.len(), 3);
        assert_eq!(mic.modules[0], 41.0);
        assert_eq!(mic.modules[2], 43.0);
        assert_eq!(mic.device, "formalshell-mix.monitor");

        let desktop = parse_audio_setup("\nalsa_output.pci-0000_00_1f.3.analog-stereo.monitor\n");
        assert_eq!(desktop.modules.len(), 0);
        assert_eq!(desktop.device, "alsa_output.pci-0000_00_1f.3.analog-stereo.monitor");
    }

    #[test]
    fn parse_audio_setup_reports_no_device_on_a_failed_setup() {
        assert_eq!(parse_audio_setup("").device, "");
        assert_eq!(parse_audio_setup("\n").device, "");
        assert_eq!(parse_audio_setup("41 42 43\n").device, "");
        let broken = parse_audio_setup("41 oops\nformalshell-mix.monitor\n");
        assert_eq!(broken.modules.len(), 0);
        assert_eq!(broken.device, "");
    }
}
