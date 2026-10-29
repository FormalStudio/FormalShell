//! One wf-recorder child, the transient
//! PipeWire mix the desktop+mic mode needs, the webcam overlay, the
//! finalize pass and the two-pass GIF transcode, chained in order.
//! `active` is the recorder child running and
//! nothing else; it is never persisted and never polled for.

use std::time::Instant;

use fs_system::capture as model;
use serde_json::json;

use super::{Act, App, notify, notify_with, now, runtime_dir, slurp_run, spawn, SLURP_REGION};
use crate::services::capture::{Exit, Job, Run};
use crate::services::compositor::{self, Command};
use crate::services::recording;
use crate::store::Topic;

const WEBCAM_APP_ID: &str = "formalshell-webcam";

#[derive(Default)]
pub struct Rec {
    pub scope: String,
    pub audio_mode: String,
    pub last_path: String,
    pub last_gif_path: String,
    pub last_error: String,
    pub finalizing: bool,
    started: Option<Instant>,
    pending_path: String,
    pending_geometry: String,
    pending_output: String,
    pending_max_height: f64,
    pending_gif_path: String,
    pending_gif_pass2: Vec<String>,
    finalize_source: String,
    finalize_processed: String,
    finalize_reencode: bool,
    /// The encoder `CODEC_PROBE` found, once per run of the shell.
    codec: String,
    audio_modules: Vec<f64>,
    stopping: bool,
    killed: bool,
    slurp: Option<u64>,
    mkdir: Option<u64>,
    audio: Option<u64>,
    webcam_list: Option<u64>,
    recorder: Option<u64>,
    finalize_step: Option<u64>,
    preview: Option<u64>,
    palette: Option<u64>,
    gif: Option<u64>,
    webcam_window: String,
    webcam_target: Option<model::WebcamGeometry>,
    webcam_audio_device: String,
    webcam_placed: bool,
    webcam_attempts: u32,
    webcam_gave_up: bool,
    /// The two webcam polls and the region watchdog, by run.
    webcam_map: Option<u64>,
    webcam_settle: Option<u64>,
    region_watch: u64,
    tick: u64,
}

impl Rec {
    pub fn active(&self) -> bool {
        self.recorder.is_some()
    }

    pub fn transcoding(&self) -> bool {
        self.palette.is_some() || self.gif.is_some()
    }

    fn in_flight(&self) -> bool {
        self.slurp.is_some()
            || self.mkdir.is_some()
            || self.audio.is_some()
            || self.webcam_list.is_some()
            || !self.webcam_window.is_empty()
            || self.webcam_map.is_some()
            || self.webcam_settle.is_some()
    }

    fn path(&self) -> &str {
        if self.active() { &self.pending_path } else { &self.last_path }
    }

    fn elapsed_ms(&self) -> u64 {
        match (self.active(), self.started) {
            (true, Some(t)) => t.elapsed().as_millis() as u64,
            _ => 0,
        }
    }
}

fn config_str(app: &App, key: &str, fallback: &str) -> String {
    app.store.config.str(key).unwrap_or(fallback).to_owned()
}

fn dir(app: &App) -> String {
    match app.store.config.str("recording.directory") {
        Some(d) if !d.is_empty() => d.to_owned(),
        _ => format!("{}/Videos", std::env::var("HOME").unwrap_or_default()),
    }
}

fn config_max_height(app: &App) -> f64 {
    app.store.config.f64("recording.maxHeight").unwrap_or(0.0)
}

/// The bar's recording indicator reads this slice, ticked once a second
/// while the recorder runs.
fn sync_bar(app: &mut App) {
    let r = &app.capture.rec;
    let diff = recording::Diff::Active { active: r.active(), elapsed_ms: r.elapsed_ms(), stopping: r.active() && r.stopping };
    if app.store.recording.apply(diff) {
        crate::surfaces::changed(app, Topic::Recording);
    }
}

fn tick(app: &mut App) {
    app.capture.rec.tick += 1;
    let token = app.capture.rec.tick;
    app.capture_after(1000, move |app| {
        if app.capture.rec.tick == token && app.capture.rec.active() {
            sync_bar(app);
            tick(app);
        }
    });
}

fn busy_answer(r: &Rec) -> Option<String> {
    if r.active() {
        return Some("error: already recording".into());
    }
    if r.in_flight() {
        return Some("error: a recording start is already in flight".into());
    }
    None
}

fn audio_ok(audio: &str) -> bool {
    ["none", "desktop", "desktopmic"].contains(&audio)
}

pub fn start(app: &mut App, scope: &str, audio: &str, max_height: Option<&str>) -> String {
    if let Some(e) = busy_answer(&app.capture.rec) {
        return e;
    }
    let scope = if scope.is_empty() { "screen" } else { scope };
    if scope != "screen" && scope != "region" {
        return format!("error: unknown scope \"{scope}\" (screen|region)");
    }
    let audio = if audio.is_empty() { "none" } else { audio };
    if !audio_ok(audio) {
        return format!("error: unknown audio mode \"{audio}\" (none|desktop|desktopmic)");
    }
    let max = model::resolve_max_height(max_height, config_max_height(app));
    if max.is_nan() {
        return format!("error: not a number: \"{}\" (maxHeight)", max_height.unwrap_or(""));
    }
    let path = model::output_path(&dir(app), "screenrecording", "mp4", &now());
    let r = &mut app.capture.rec;
    r.scope = scope.into();
    r.audio_mode = audio.into();
    r.pending_max_height = max;
    r.last_error.clear();
    r.pending_geometry.clear();
    r.pending_output.clear();
    r.pending_path = path.clone();
    if scope == "region" {
        let g = app.capture.generation();
        app.capture.rec.slurp = Some(g);
        spawn(app, slurp_run(app, Job::RecSlurp, g, "destructive", SLURP_REGION));
        let secs = app.store.config.f64("recording.timeoutSeconds").unwrap_or(90.0);
        app.capture.rec.region_watch += 1;
        let token = app.capture.rec.region_watch;
        app.capture_after((secs * 1000.0) as u64, move |app| {
            if app.capture.rec.region_watch != token || app.capture.rec.slurp.is_none() {
                return;
            }
            let slurp = app.capture.rec.slurp.take();
            app.capture.stop(slurp);
            app.capture.rec.pending_path.clear();
            notify("RECORDING CANCELLED", &format!("no region after {}s", secs.round()));
        });
        return path;
    }
    prepare_directory(app);
    path
}

pub fn start_at(app: &mut App, geometry: &str, output: &str, scope: &str, audio: &str, max_height: Option<&str>) -> String {
    if let Some(e) = busy_answer(&app.capture.rec) {
        return e;
    }
    let parsed = model::parse_geometry(geometry);
    if parsed.is_empty() {
        return format!("error: not a geometry: \"{geometry}\" (expected \"X,Y WxH\")");
    }
    let scope = if scope.is_empty() { "region" } else { scope };
    if !["screen", "region", "window"].contains(&scope) {
        return format!("error: unknown scope \"{scope}\" (screen|region|window)");
    }
    let audio = if audio.is_empty() { "none" } else { audio };
    if !audio_ok(audio) {
        return format!("error: unknown audio mode \"{audio}\" (none|desktop|desktopmic)");
    }
    let max = model::resolve_max_height(max_height, config_max_height(app));
    if max.is_nan() {
        return format!("error: not a number: \"{}\" (maxHeight)", max_height.unwrap_or(""));
    }
    let path = model::output_path(&dir(app), "screenrecording", "mp4", &now());
    let r = &mut app.capture.rec;
    r.scope = scope.into();
    r.audio_mode = audio.into();
    r.pending_max_height = max;
    r.last_error.clear();
    r.pending_geometry = parsed;
    r.pending_output = output.into();
    r.pending_path = path.clone();
    prepare_directory(app);
    path
}

pub fn stop(app: &mut App) -> String {
    if app.capture.rec.slurp.is_some() {
        app.capture.rec.region_watch += 1;
        let slurp = app.capture.rec.slurp.take();
        app.capture.stop(slurp);
        app.capture.rec.pending_path.clear();
        return "ok".into();
    }
    if !app.capture.rec.active() {
        return "error: not recording".into();
    }
    app.capture.rec.stopping = true;
    // wf-recorder only reads its exit flag when a new frame arrives, and a
    // screencopy frame arrives only on damage; a shell at rest damages
    // nothing, so the indicator dims here and that redraw is the frame.
    sync_bar(app);
    let Some(pid) = app.capture.pid(app.capture.rec.recorder) else {
        // Not spawned yet: the Spawned event lands on a stopping run.
        return "ok".into();
    };
    crate::services::capture::terminate(pid);
    let g = app.capture.rec.recorder;
    app.capture_after(5000, move |app| {
        if app.capture.rec.recorder != g {
            return;
        }
        if let Some(pid) = app.capture.pid(g) {
            app.capture.rec.killed = true;
            crate::services::capture::kill(pid);
        }
    });
    "ok".into()
}

pub fn toggle(app: &mut App, scope: &str, audio: &str) -> String {
    if app.capture.rec.active() || app.capture.rec.slurp.is_some() {
        return stop(app);
    }
    start(app, scope, audio, None)
}

pub fn gif(app: &mut App, path: &str) -> String {
    if app.capture.rec.transcoding() {
        return "error: a transcode is already running".into();
    }
    let source = if path.is_empty() { app.capture.rec.last_path.clone() } else { path.to_owned() };
    if source.is_empty() {
        return "error: no recording to convert".into();
    }
    let out = model::gif_output_path(&source);
    let palette = format!("{}/formalshell/gif-palette.png", runtime_dir());
    let opts = model::GifOpts {
        fps: app.store.config.f64("recording.gifFps").unwrap_or(12.0),
        width: app.store.config.f64("recording.gifWidth").unwrap_or(640.0),
    };
    let argv = model::gif_argv(&source, &palette, &out, opts);
    let g = app.capture.generation();
    let r = &mut app.capture.rec;
    r.last_error.clear();
    r.pending_gif_path = out.clone();
    r.pending_gif_pass2 = argv.pass2;
    r.palette = Some(g);
    let mut pass1 = vec!["sh".into(), "-c".into(), r#"mkdir -p "$(dirname "$1")" && shift && exec "$@""#.into(), "sh".into(), palette];
    pass1.extend(argv.pass1);
    spawn(app, Run::new(Job::Palette, g, pass1));
    out
}

/// RECORDING SAVED's PLAY action: recording.player, the same env and sh
/// handoff the screenshot editor takes.
pub fn play(app: &mut App, path: &str) -> String {
    if path.is_empty() {
        return "error: no path".into();
    }
    let player = config_str(app, "recording.player", "xdg-open");
    let g = app.capture.generation();
    spawn(app, Run::sh(Job::Player, g, r#"exec "$FS_PLAYER" "$FS_PLAY_PATH""#).env("FS_PLAYER", player).env("FS_PLAY_PATH", path));
    "ok".into()
}

pub fn status(app: &App) -> String {
    let r = &app.capture.rec;
    json!({
        "active": r.active(),
        "scope": if r.scope.is_empty() { "screen" } else { &r.scope },
        "audio": if r.audio_mode.is_empty() { "none" } else { &r.audio_mode },
        "path": r.path(),
        "elapsedMs": r.elapsed_ms(),
        "transcoding": r.transcoding(),
        "finalizing": r.finalizing,
        "lastGifPath": r.last_gif_path,
        "lastError": r.last_error,
    })
    .to_string()
}

fn prepare_directory(app: &mut App) {
    let g = app.capture.generation();
    app.capture.rec.mkdir = Some(g);
    let script = format!(r#"mkdir -p "$FS_DIR" || exit 1; [ -n "$FS_PROBE" ] || exit 0; {}; exit 0"#, model::CODEC_PROBE);
    let probe = if app.capture.rec.codec.is_empty() { "1" } else { "" };
    spawn(app, Run::sh(Job::Mkdir, g, &script).env("FS_DIR", dir(app)).env("FS_PROBE", probe));
}

fn setup_audio(app: &mut App) {
    if app.capture.rec.audio_mode == "none" {
        setup_webcam(app, String::new());
        return;
    }
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
    let script = if app.capture.rec.audio_mode == "desktopmic" { desktop_mic } else { desktop };
    let g = app.capture.generation();
    app.capture.rec.audio = Some(g);
    spawn(app, Run::sh(Job::Audio, g, script).env("FS_MIX", model::MIX_SINK));
}

fn launch(app: &mut App, device: String) {
    let output = if app.capture.rec.pending_output.is_empty() {
        app.store.compositor.snapshot.focused_output_name.clone()
    } else {
        app.capture.rec.pending_output.clone()
    };
    let r = &app.capture.rec;
    let configured = config_str(app, "recording.codec", "");
    let picked = configured.is_empty() && !r.codec.is_empty();
    let opts = model::RecorderOpts {
        path: r.pending_path.clone(),
        framerate: app.store.config.f64("recording.framerate").unwrap_or(30.0),
        output,
        geometry: r.pending_geometry.clone(),
        codec: if configured.is_empty() { r.codec.clone() } else { configured },
        pixel_format: if picked { "yuv420p".into() } else { String::new() },
        max_height: Some(r.pending_max_height),
        no_dmabuf: app.store.config.bool("recording.noDmabuf") == Some(true),
        audio_device: device,
        audio_backend: config_str(app, "recording.audioBackend", ""),
    };
    let argv = fs_system::proc::die_with_parent(&model::recorder_argv(&opts));
    let g = app.capture.generation();
    let r = &mut app.capture.rec;
    r.started = Some(Instant::now());
    r.stopping = false;
    r.recorder = Some(g);
    spawn(app, Run::new(Job::Recorder, g, argv));
    sync_bar(app);
    tick(app);
}

fn setup_webcam(app: &mut App, device: String) {
    app.capture.rec.webcam_audio_device = device.clone();
    if app.store.config.bool("recording.webcam") != Some(true) {
        launch(app, device);
        return;
    }
    let Some(region) = webcam_region(app) else {
        eprintln!("RecordingService: webcam overlay unavailable: could not resolve the captured region");
        notify("WEBCAM UNAVAILABLE", "could not resolve the captured region");
        launch(app, device);
        return;
    };
    let size = config_str(app, "recording.webcamSize", "medium");
    let margin = app.store.theme.theme.space.huge;
    app.capture.rec.webcam_target = Some(model::webcam_geometry(&size, region, margin));
    let configured = config_str(app, "recording.webcamDevice", "");
    if !configured.is_empty() {
        start_webcam(app, &configured);
        return;
    }
    let g = app.capture.generation();
    app.capture.rec.webcam_list = Some(g);
    spawn(app, Run::sh(Job::WebcamList, g, r#"for f in /dev/video*; do [ -e "$f" ] && echo "$f"; done"#));
}

fn webcam_region(app: &App) -> Option<model::Region> {
    let r = &app.capture.rec;
    if !r.pending_geometry.is_empty() {
        return model::region_from_geometry(&r.pending_geometry);
    }
    let name = if r.pending_output.is_empty() { app.store.compositor.snapshot.focused_output_name.clone() } else { r.pending_output.clone() };
    app.capture_screens()
        .into_iter()
        .find(|s| s.name == name)
        .map(|s| model::Region { x: s.rect.x, y: s.rect.y, width: s.rect.w, height: s.rect.h })
}

fn start_webcam(app: &mut App, device: &str) {
    let r = &mut app.capture.rec;
    r.webcam_attempts = 0;
    r.webcam_gave_up = false;
    r.webcam_placed = false;
    r.webcam_window.clear();
    compositor::spawn(&model::webcam_argv(device, WEBCAM_APP_ID));
    let g = app.capture.generation();
    app.capture.rec.webcam_map = Some(g);
    webcam_map_poll(app, g);
}

fn webcam_map_poll(app: &mut App, g: u64) {
    app.capture_after(50, move |app| {
        if app.capture.rec.webcam_map != Some(g) {
            return;
        }
        let r = &mut app.capture.rec;
        r.webcam_attempts += 1;
        let win = app.store.compositor.snapshot.windows.iter().find(|w| w.app_id == WEBCAM_APP_ID).map(|w| w.id.clone());
        let action = model::webcam_map_poll_action(
            win.is_some(),
            r.webcam_attempts,
            r.webcam_gave_up,
            model::WEBCAM_MAP_GIVEUP_ATTEMPTS,
            model::WEBCAM_MAP_REAP_ATTEMPTS,
        );
        match action {
            model::MapPollAction::Place => {
                let id = win.unwrap_or_default();
                r.webcam_map = None;
                r.webcam_window = id.clone();
                r.webcam_attempts = 0;
                compositor::send(Command::FloatWindow(id));
                let s = app.capture.generation();
                app.capture.rec.webcam_settle = Some(s);
                webcam_settle_poll(app, s);
                return;
            }
            model::MapPollAction::Reap => {
                r.webcam_map = None;
                r.webcam_gave_up = false;
                eprintln!("RecordingService: webcam mapped after the timeout; closing the straggler");
                compositor::close_window(&win.unwrap_or_default());
                return;
            }
            model::MapPollAction::GiveUp => {
                r.webcam_gave_up = true;
                eprintln!("RecordingService: webcam overlay unavailable: camera did not open in time");
                notify("WEBCAM UNAVAILABLE", "camera did not open in time");
                let device = r.webcam_audio_device.clone();
                launch(app, device);
            }
            model::MapPollAction::Stop => {
                r.webcam_map = None;
                r.webcam_gave_up = false;
                return;
            }
            model::MapPollAction::Wait => {}
        }
        webcam_map_poll(app, g);
    });
}

fn webcam_settle_poll(app: &mut App, g: u64) {
    app.capture_after(50, move |app| {
        if app.capture.rec.webcam_settle != Some(g) {
            return;
        }
        app.capture.rec.webcam_attempts += 1;
        let id = app.capture.rec.webcam_window.clone();
        let rect = app.store.compositor.snapshot.windows.iter().find(|w| w.id == id).and_then(|w| w.rect);
        let r = &mut app.capture.rec;
        let t = r.webcam_target.unwrap_or(model::WebcamGeometry { width: 0.0, height: 0.0, x: 0.0, y: 0.0 });
        if rect.is_some() && !r.webcam_placed {
            r.webcam_placed = true;
            r.webcam_attempts = 0;
            compositor::send(Command::PlaceFloatingWindow { id, x: t.x, y: t.y, width: t.width, height: t.height });
            webcam_settle_poll(app, g);
            return;
        }
        if let (Some(rect), true) = (rect, r.webcam_placed)
            && rect.width as f64 == t.width
            && rect.height as f64 == t.height
        {
            r.webcam_settle = None;
            let device = r.webcam_audio_device.clone();
            launch(app, device);
            return;
        }
        if r.webcam_attempts >= 40 {
            r.webcam_settle = None;
            eprintln!("RecordingService: webcam overlay did not settle in time, recording anyway");
            notify("WEBCAM UNPLACED", "the camera did not settle in time");
            let device = r.webcam_audio_device.clone();
            launch(app, device);
            return;
        }
        webcam_settle_poll(app, g);
    });
}

fn cleanup_webcam(app: &mut App) {
    let r = &mut app.capture.rec;
    if r.webcam_window.is_empty() {
        return;
    }
    compositor::close_window(&r.webcam_window);
    r.webcam_window.clear();
}

fn release_audio(app: &mut App) {
    if app.capture.rec.audio_modules.is_empty() {
        return;
    }
    let mut modules = std::mem::take(&mut app.capture.rec.audio_modules);
    modules.reverse();
    let list = modules.iter().map(|m| (*m as i64).to_string()).collect::<Vec<_>>().join(" ");
    let g = app.capture.generation();
    spawn(app, Run::sh(Job::Unload, g, r#"for m in $FS_MODULES; do pactl unload-module "$m"; done"#).env("FS_MODULES", list));
}

fn fail(app: &mut App, why: String) {
    let r = &mut app.capture.rec;
    r.region_watch += 1;
    r.pending_path.clear();
    r.pending_geometry.clear();
    r.pending_output.clear();
    eprintln!("RecordingService: {why}");
    notify("RECORDING FAILED", &why);
    r.last_error = why;
    release_audio(app);
}

fn finalize(app: &mut App, path: String) {
    if app.store.config.bool("recording.finalize") == Some(false) {
        announce_saved(app, path);
        return;
    }
    let g = app.capture.generation();
    let r = &mut app.capture.rec;
    r.finalizing = true;
    r.finalize_source = path.clone();
    r.finalize_step = Some(g);
    let argv = ["ffprobe", "-v", "error", "-select_streams", "v:0", "-read_intervals", "%+0.2", "-show_entries", "packet=flags", "-of", "csv=p=0", &path];
    spawn(app, Run::new(Job::ProbeVideo, g, argv.map(String::from).to_vec()));
}

fn announce_saved(app: &mut App, path: String) {
    app.capture.rec.finalizing = false;
    let g = app.capture.generation();
    app.capture.rec.preview = Some(g);
    let argv = model::preview_frame_argv(&path, &model::preview_frame_path(&path));
    spawn(app, Run::new(Job::Preview, g, argv).tag(path));
}

fn or_code(text: &str, fallback: String) -> String {
    match text.trim() {
        "" => fallback,
        t => t.to_owned(),
    }
}

pub fn exit(app: &mut App, e: Exit) {
    let r = &app.capture.rec;
    let mine = |slot: Option<u64>| slot == Some(e.generation);
    match e.job {
        Job::RecSlurp => {
            if !mine(r.slurp) {
                return;
            }
            app.capture.rec.slurp = None;
            app.capture.rec.region_watch += 1;
            let geometry = model::parse_geometry(&e.stdout);
            if e.started && e.code == 0 && !geometry.is_empty() {
                app.capture.rec.pending_geometry = geometry;
                prepare_directory(app);
                return;
            }
            app.capture.rec.pending_path.clear();
            if e.started && e.code == 1 {
                return;
            }
            let fallback = if !e.started { "slurp failed to start".into() } else if e.code == 0 { "slurp reported no geometry".into() } else { format!("slurp exited {}", e.code) };
            fail(app, or_code(&e.stderr, fallback));
        }
        Job::Mkdir => {
            if !mine(r.mkdir) {
                return;
            }
            app.capture.rec.mkdir = None;
            if e.started && e.code == 0 {
                if app.capture.rec.codec.is_empty() {
                    app.capture.rec.codec = e.stdout.trim().to_owned();
                }
                setup_audio(app);
                return;
            }
            let d = dir(app);
            fail(app, or_code(&e.stderr, format!("could not create {d}")));
        }
        Job::Audio => {
            if !mine(r.audio) {
                return;
            }
            app.capture.rec.audio = None;
            if e.code == 5 {
                fail(app, "no microphone: the default source is a monitor".into());
                return;
            }
            let setup = model::parse_audio_setup(&e.stdout);
            if !e.started || e.code != 0 || setup.device.is_empty() {
                fail(app, or_code(&e.stderr, format!("audio setup exited {}", e.code)));
                return;
            }
            app.capture.rec.audio_modules = setup.modules;
            setup_webcam(app, setup.device);
        }
        Job::WebcamList => {
            if !mine(r.webcam_list) {
                return;
            }
            app.capture.rec.webcam_list = None;
            let devices = model::parse_webcam_devices(&e.stdout);
            let Some(first) = devices.first() else {
                eprintln!("RecordingService: webcam overlay unavailable: no /dev/video* device");
                notify("WEBCAM UNAVAILABLE", "no video capture device found");
                let device = app.capture.rec.webcam_audio_device.clone();
                launch(app, device);
                return;
            };
            let first = first.clone();
            start_webcam(app, &first);
        }
        Job::Recorder => {
            if !mine(r.recorder) {
                return;
            }
            app.capture.rec.recorder = None;
            cleanup_webcam(app);
            if !e.started && !app.capture.rec.stopping {
                fail(app, "wf-recorder not found (failed to start)".into());
                sync_bar(app);
                return;
            }
            release_audio(app);
            let r = &mut app.capture.rec;
            let saved = std::mem::take(&mut r.pending_path);
            r.pending_geometry.clear();
            r.pending_output.clear();
            let was_stopping = std::mem::take(&mut r.stopping);
            if std::mem::take(&mut r.killed) {
                r.last_path = saved.clone();
                r.last_error = format!("wf-recorder ignored SIGTERM and was killed; {saved} may be truncated");
                eprintln!("RecordingService: {}", r.last_error);
                notify("RECORDING TRUNCATED", &r.last_error.clone());
                sync_bar(app);
                return;
            }
            if e.code != 0 && !was_stopping {
                r.last_error = or_code(&e.stderr, format!("wf-recorder exited {}", e.code));
                eprintln!("RecordingService: {}", r.last_error);
                notify("RECORDING FAILED", &r.last_error.clone());
                sync_bar(app);
                return;
            }
            r.last_path = saved.clone();
            r.last_error.clear();
            sync_bar(app);
            finalize(app, saved);
        }
        Job::ProbeVideo => {
            if !mine(r.finalize_step) {
                return;
            }
            if !e.started {
                eprintln!("RecordingService: ffprobe not found (failed to start)");
                let source = app.capture.rec.finalize_source.clone();
                announce_saved(app, source);
                return;
            }
            app.capture.rec.finalize_reencode = model::finalize_needs_reencode(&e.stdout);
            let g = app.capture.generation();
            app.capture.rec.finalize_step = Some(g);
            let source = app.capture.rec.finalize_source.clone();
            let argv = ["ffprobe", "-v", "error", "-select_streams", "a", "-show_entries", "stream=codec_type", "-of", "csv=p=0", &source];
            spawn(app, Run::new(Job::ProbeAudio, g, argv.map(String::from).to_vec()));
        }
        Job::ProbeAudio => {
            if !mine(r.finalize_step) {
                return;
            }
            let source = app.capture.rec.finalize_source.clone();
            if !e.started {
                eprintln!("RecordingService: ffprobe not found (failed to start)");
                announce_saved(app, source);
                return;
            }
            let has_audio = model::finalize_has_audio(&e.stdout);
            let processed = model::finalize_output_path(&source);
            let argv = model::finalize_argv(&source, &processed, app.capture.rec.finalize_reencode.then_some(app.capture.rec.codec.as_str()), has_audio);
            app.capture.rec.finalize_processed = processed;
            let g = app.capture.generation();
            app.capture.rec.finalize_step = Some(g);
            spawn(app, Run::new(Job::Finalize, g, argv));
        }
        Job::Finalize => {
            if !mine(r.finalize_step) {
                return;
            }
            let (source, processed) = (r.finalize_source.clone(), r.finalize_processed.clone());
            if !e.started {
                eprintln!("RecordingService: ffmpeg not found (failed to start)");
                announce_saved(app, source);
                return;
            }
            let argv = if e.code == 0 {
                vec!["mv".to_string(), processed, source]
            } else {
                eprintln!("RecordingService: finalize failed, keeping the raw recording: {}", or_code(&e.stderr, format!("ffmpeg exited {}", e.code)));
                vec!["rm".to_string(), "-f".into(), processed]
            };
            let g = app.capture.generation();
            app.capture.rec.finalize_step = Some(g);
            spawn(app, Run::new(Job::FinalizeCleanup, g, argv));
        }
        Job::FinalizeCleanup => {
            if !mine(r.finalize_step) {
                return;
            }
            app.capture.rec.finalize_step = None;
            let source = app.capture.rec.finalize_source.clone();
            announce_saved(app, source);
        }
        Job::Preview => {
            if !mine(r.preview) {
                return;
            }
            app.capture.rec.preview = None;
            let path = e.tag.clone();
            let preview = model::preview_frame_path(&path);
            let image = if e.started && e.code == 0 { preview.as_str() } else { "" };
            let actions = vec![("default", "PLAY", Act::Play(path.clone())), ("gif", "GIF", Act::Gif(path.clone()))];
            notify_with("RECORDING SAVED", &path, actions, image);
            if e.started && e.code == 0 {
                app.capture_after(2000, move |app| {
                    let g = app.capture.generation();
                    spawn(app, Run::new(Job::Quiet, g, vec!["rm".into(), "-f".into(), preview]));
                });
            }
        }
        Job::Player => {
            if e.started && e.code == 0 {
                return;
            }
            let why = or_code(&e.stderr, format!("player exited {}", e.code));
            eprintln!("RecordingService: player launch failed: {why}");
            notify("PLAYER FAILED", &why);
        }
        Job::Palette => {
            if !mine(r.palette) {
                return;
            }
            app.capture.rec.palette = None;
            if e.started && e.code == 0 {
                let g = app.capture.generation();
                app.capture.rec.gif = Some(g);
                let pass2 = std::mem::take(&mut app.capture.rec.pending_gif_pass2);
                spawn(app, Run::new(Job::Gif, g, pass2));
                return;
            }
            let why = or_code(&e.stderr, format!("palettegen exited {}", e.code));
            eprintln!("RecordingService: {why}");
            notify("GIF FAILED", &why);
            app.capture.rec.last_error = why;
        }
        Job::Gif => {
            if !mine(r.gif) {
                return;
            }
            app.capture.rec.gif = None;
            let r = &mut app.capture.rec;
            if e.started && e.code == 0 {
                r.last_gif_path = r.pending_gif_path.clone();
                r.last_error.clear();
                let gif = r.last_gif_path.clone();
                notify_with("GIF SAVED", &gif, Vec::new(), &gif);
                return;
            }
            r.last_error = or_code(&e.stderr, format!("paletteuse exited {}", e.code));
            eprintln!("RecordingService: {}", r.last_error);
            notify("GIF FAILED", &r.last_error.clone());
        }
        _ => {}
    }
}

/// A recorder pid arriving after `stop` was asked for: the stop goes out
/// now.
pub fn spawned(app: &mut App, generation: u64, pid: u32) {
    if app.capture.rec.recorder == Some(generation) && app.capture.rec.stopping {
        crate::services::capture::terminate(pid);
    }
}
