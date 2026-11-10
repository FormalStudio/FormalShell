//! The capture family: the screenshot and capture IPC, the recorder and the
//! region picker. Every child runs on the service thread
//! (`services::capture`); its pid and its answer come back here as events,
//! and this is where every decision about them is made, on the UI thread
//! next to the picker surface it drives (`wayland::region`).

pub mod record;

use std::collections::HashMap;

use fs_system::capture as model;
use fs_theme::palette::ColorRole;
use fs_types::Rect;
use serde_json::{Value, json};

use crate::services::capture::{self as svc, Act, Event, Exit, Job, Run, or_code};
use crate::services::notifications::{Local, LocalAction, Op};
use crate::settings;
use crate::surfaces::capture::picker::{Cands, Picker};
use crate::wayland::App;

#[derive(Default)]
pub struct Shot {
    pub busy: bool,
    pub pending_path: String,
    pub last_path: String,
    pub last_error: String,
    pub last_cancelled: bool,
    pub processing: String,
    pub slurp: Option<u64>,
    pub grab: Option<u64>,
    pub watch: u64,
}

#[derive(Default)]
pub struct Grab {
    pub busy: bool,
    pub mode: String,
    pub last_hex: String,
    pub last_text: String,
    pub last_error: String,
    pub last_cancelled: bool,
    pub slurp: Option<u64>,
    pub ocr: Option<u64>,
    pub pick: Option<u64>,
    pub watch: u64,
}

#[derive(Default)]
pub struct Capture {
    next: u64,
    /// Every running child's pid, by the generation it was started under.
    pids: HashMap<u64, u32>,
    /// Generations whose answer nobody waits for any more.
    dropped: Vec<u64>,
    pub shot: Shot,
    pub grab: Grab,
    pub rec: record::Rec,
    pub picker: Picker,
}

impl Capture {
    pub fn generation(&mut self) -> u64 {
        self.next += 1;
        self.next
    }

    /// Ends the child started under `generation`, if it still runs, and
    /// forgets its answer.
    pub fn stop(&mut self, generation: Option<u64>) {
        let Some(g) = generation else { return };
        match self.pids.remove(&g) {
            Some(pid) => svc::terminate(pid),
            None => self.dropped.push(g),
        }
    }

    pub fn pid(&self, generation: Option<u64>) -> Option<u32> {
        generation.and_then(|g| self.pids.get(&g).copied())
    }
}

fn notify(app: &mut App, summary: &str, body: &str) {
    notify_with(app, summary, body, Vec::new(), "");
}

/// The same with `(key, label, act)` actions and a picture on disk.
fn notify_with(app: &mut App, summary: &str, body: &str, actions: Vec<(&'static str, &'static str, Act)>, image: &str) {
    let actions = actions.into_iter().map(|(key, label, act)| LocalAction { key: key.into(), label: label.into(), run: Local::Capture(act) }).collect();
    let url = if image.is_empty() { String::new() } else { format!("file://{image}") };
    let urgency = fs_info::notifications::Urgency::Normal;
    app.apply(Op::NotifyRunning { summary: summary.into(), body: body.into(), urgency, actions, image: url });
    let id = app.store.notifications.newest_local();
    if !image.is_empty()
        && let Some(rt) = &app.runtime
    {
        let path = image.to_owned();
        rt.service(move |ctx| svc::picture(ctx, id, path));
    }
}

/// Runs the actions picked on this family's notifications since the last
/// pass, in the order they were picked.
pub fn flush(app: &mut App) {
    for act in std::mem::take(&mut app.store.notifications.capture_picked) {
        match act {
            Act::Edit(path) => {
                let _ = shot_edit(app, &path);
            }
            Act::Play(path) => {
                let _ = record::play(app, &path);
            }
            Act::Gif(path) => {
                let _ = record::gif(app, &path);
            }
        }
    }
}

pub fn spawn(app: &App, run: Run) {
    if let Some(rt) = &app.runtime {
        rt.service(move |ctx| svc::start(ctx, run));
    }
}

pub fn runtime_dir() -> String {
    crate::paths::runtime().to_string_lossy().into_owned()
}

pub fn now() -> chrono::NaiveDateTime {
    chrono::Local::now().naive_local()
}

fn hex(app: &App, role: ColorRole, alpha: &str) -> String {
    format!("{}{alpha}", app.store.theme.theme.colors.get(role).to_hex())
}

fn border_weight(app: &App) -> String {
    fs_js::num_str(app.store.theme.theme.border_width)
}

/// slurp's chrome, resolved at call time so it follows matugen.
fn slurp_run(app: &App, job: Job, generation: u64, border_role: ColorRole, script: &str) -> Run {
    Run::sh(job, generation, script)
        .env("FS_SLURP_BG", hex(app, ColorRole::Background, "99"))
        .env("FS_SLURP_BORDER", hex(app, border_role, "FF"))
        .env("FS_SLURP_SEL", "#00000000")
        .env("FS_SLURP_WEIGHT", border_weight(app))
}

const SLURP_REGION: &str =
    r#"exec slurp -d -w "$FS_SLURP_WEIGHT" -b "$FS_SLURP_BG" -c "$FS_SLURP_BORDER" -s "$FS_SLURP_SEL" 0</dev/null"#;

pub fn cands(app: &App) -> Cands {
    Cands::new(&app.capture_screens(), &app.store.compositor.snapshot)
}

// screenshot

fn shot_path(app: &App) -> String {
    format!("{}/screenshot-{}.png", settings::screenshot_dir(&app.store.config), model::timestamp(&now()))
}

fn shot_watchdog(app: &mut App) {
    let secs = settings::SCREENSHOT_TIMEOUT_SECONDS.get(&app.store.config);
    app.capture.shot.watch += 1;
    let token = app.capture.shot.watch;
    app.capture_after((secs * 1000.0) as u64, move |app| {
        if app.capture.shot.watch == token {
            let _ = shot_cancel(app, &format!("no selection after {}s", secs.round()));
        }
    });
}

pub fn shot_start(app: &mut App, region: bool, processing: &str) -> Result<String, String> {
    if app.capture.shot.busy {
        return Err("capture already in flight".into());
    }
    let path = shot_path(app);
    let s = &mut app.capture.shot;
    s.busy = true;
    s.processing = if processing.is_empty() { "default".into() } else { processing.into() };
    s.pending_path = path.clone();
    s.last_cancelled = false;
    if region {
        let g = app.capture.generation();
        app.capture.shot.slurp = Some(g);
        spawn(app, slurp_run(app, Job::ShotSlurp, g, ColorRole::Primary, SLURP_REGION));
        shot_watchdog(app);
    } else {
        shot_grab(app, "");
    }
    Ok(path)
}

pub fn shot_pick(app: &mut App, mode: &str, processing: &str) -> Result<String, String> {
    if app.capture.shot.busy {
        return Err("capture already in flight".into());
    }
    let path = shot_path(app);
    let s = &mut app.capture.shot;
    s.busy = true;
    s.processing = if processing.is_empty() { "default".into() } else { processing.into() };
    s.pending_path = path;
    s.last_cancelled = false;
    if let Err(e) = picker_open(app, if mode.is_empty() { "smart" } else { mode }) {
        app.capture.shot.busy = false;
        return Err(e);
    }
    // A fullscreen pick has already committed inside open(); its grab owns
    // the capture from here and needs no watchdog.
    if app.capture.shot.busy && app.capture.shot.grab.is_none() {
        shot_watchdog(app);
    }
    Ok(app.capture.shot.pending_path.clone())
}

fn shot_grab(app: &mut App, geometry: &str) {
    let grab = if geometry.is_empty() { r#"grim "$FS_SHOT_PATH""# } else { r#"grim -g "$FS_SHOT_GEOM" "$FS_SHOT_PATH""# };
    let pipeline = match app.capture.shot.processing.as_str() {
        "copy" => grab.replace(r#""$FS_SHOT_PATH""#, "-") + " | wl-copy --type image/png",
        "save" => grab.to_owned(),
        _ => format!(r#"{grab} && wl-copy --type image/png < "$FS_SHOT_PATH""#),
    };
    let g = app.capture.generation();
    app.capture.shot.grab = Some(g);
    let run = Run::sh(Job::ShotGrab, g, &format!(r#"mkdir -p "$FS_SHOT_DIR" && {pipeline}"#))
        .env("FS_SHOT_DIR", settings::screenshot_dir(&app.store.config))
        .env("FS_SHOT_PATH", app.capture.shot.pending_path.clone())
        .env("FS_SHOT_GEOM", geometry);
    spawn(app, run);
}

pub fn shot_edit(app: &mut App, path: &str) -> Result<(), String> {
    if path.is_empty() {
        return Err("no path".into());
    }
    let editor = settings::SCREENSHOT_EDITOR.get(&app.store.config).to_owned();
    let g = app.capture.generation();
    spawn(app, Run::sh(Job::Editor, g, r#"exec "$FS_EDITOR" "$FS_EDIT_PATH""#).env("FS_EDITOR", editor).env("FS_EDIT_PATH", path));
    Ok(())
}

pub fn shot_cancel(app: &mut App, reason: &str) -> Result<(), String> {
    if !app.capture.shot.busy {
        return Err("no capture in flight".into());
    }
    app.capture.shot.watch += 1;
    let (slurp, grab) = (app.capture.shot.slurp.take(), app.capture.shot.grab.take());
    app.capture.stop(slurp);
    app.capture.stop(grab);
    if app.capture.picker.open || app.capture.picker.pending_freezes > 0 {
        picker_done(app);
    }
    let s = &mut app.capture.shot;
    s.busy = false;
    s.pending_path.clear();
    s.last_error.clear();
    s.last_cancelled = true;
    notify(app, "SCREENSHOT CANCELLED", reason);
    Ok(())
}

pub fn shot_status(app: &App) -> Value {
    let s = &app.capture.shot;
    json!({"capturing": s.busy, "lastPath": s.last_path, "lastError": s.last_error, "lastCancelled": s.last_cancelled})
}

fn shot_exit(app: &mut App, e: Exit) {
    match e.job {
        Job::ShotSlurp => {
            if app.capture.shot.slurp != Some(e.generation) {
                return;
            }
            app.capture.shot.slurp = None;
            app.capture.shot.watch += 1;
            let geometry = e.stdout.trim().to_owned();
            if e.started && e.code == 0 && !geometry.is_empty() {
                shot_grab(app, &geometry);
                return;
            }
            let s = &mut app.capture.shot;
            s.busy = false;
            s.pending_path.clear();
            if e.started && e.code == 1 {
                s.last_error.clear();
                s.last_cancelled = true;
                return;
            }
            s.last_error = match e.stderr.trim() {
                "" if !e.started => "slurp failed to start".into(),
                "" if e.code == 0 => "slurp reported no geometry".into(),
                "" => format!("slurp exited {}", e.code),
                t => t.to_owned(),
            };
            eprintln!("ScreenshotIpc: {}", s.last_error);
            let why = s.last_error.clone();
            notify(app, "SCREENSHOT FAILED", &why);
        }
        Job::ShotGrab => {
            if app.capture.picker.open {
                picker_done(app);
            }
            if app.capture.shot.grab != Some(e.generation) {
                return;
            }
            app.capture.shot.grab = None;
            app.capture.shot.watch += 1;
            let s = &mut app.capture.shot;
            s.busy = false;
            if e.started && e.code == 0 {
                s.last_error.clear();
                if s.processing == "copy" {
                    s.last_path.clear();
                    notify(app, "SCREENSHOT COPIED", "on the clipboard");
                    return;
                }
                s.last_path = s.pending_path.clone();
                let saved = s.last_path.clone();
                if s.processing == "save" {
                    notify_with(app, "SCREENSHOT SAVED", &saved, Vec::new(), &saved);
                } else {
                    notify_with(app, "SCREENSHOT SAVED", &saved, vec![("default", "EDIT", Act::Edit(saved.clone()))], &saved);
                }
                return;
            }
            s.last_error = match e.stderr.trim() {
                "" => format!("capture exited {}", e.code),
                t => t.to_owned(),
            };
            eprintln!("ScreenshotIpc: {}", s.last_error);
            let why = s.last_error.clone();
            notify(app, "SCREENSHOT FAILED", &why);
        }
        Job::Editor => {
            if e.started && e.code == 0 {
                return;
            }
            let why = match e.stderr.trim() {
                "" => format!("editor exited {}", e.code),
                t => t.to_owned(),
            };
            eprintln!("ScreenshotIpc: editor launch failed: {why}");
            notify(app, "EDITOR FAILED", &why);
        }
        _ => {}
    }
}

/// A whole-output pick with nothing frozen: grim takes the live screen.
fn on_picked(app: &mut App, r: Rect) {
    app.capture.shot.watch += 1;
    let geometry = format!("{},{} {}x{}", r.x.round(), r.y.round(), r.width.round(), r.height.round());
    shot_grab(app, &geometry);
}

/// The selection cut out of its output's freeze at the freeze's native
/// pixels, so the file holds exactly what the picker showed. A selection
/// over two outputs keeps the part on the one it mostly covers.
fn crop_shot(app: &mut App, r: Rect, c: &Cands) {
    app.capture.shot.watch += 1;
    let picked = c
        .outputs
        .iter()
        .filter_map(|o| Some((o.label.clone(), o.rect?, o.rect?.intersect(&r)?)))
        .max_by(|a, b| (a.2.width * a.2.height).total_cmp(&(b.2.width * b.2.height)));
    let frame = picked.as_ref().and_then(|(name, ..)| app.capture.picker.frames.get(name).cloned().flatten());
    let (Some((name, out, sel)), Some(frame)) = (picked, frame) else {
        // No freeze to cut from: take the picker down and photograph the
        // live screen once the compositor has dropped it.
        picker_done(app);
        let g = app.capture.picker.generation;
        app.capture_after(120, move |app| {
            if app.capture.shot.busy && app.capture.shot.grab.is_none() && app.capture.picker.generation == g {
                on_picked(app, r);
            }
        });
        return;
    };
    picker_done(app);
    let (fw, fh) = (frame.pixmap.width() as f64, frame.pixmap.height() as f64);
    let (sx, sy) = (fw / out.width, fh / out.height);
    let x0 = ((sel.x - out.x) * sx).round().clamp(0.0, fw) as u32;
    let y0 = ((sel.y - out.y) * sy).round().clamp(0.0, fh) as u32;
    let x1 = ((sel.x + sel.width - out.x) * sx).round().clamp(0.0, fw) as u32;
    let y1 = ((sel.y + sel.height - out.y) * sy).round().clamp(0.0, fh) as u32;
    let src = format!("{}/formalshell-capture-{name}.png", runtime_dir());
    let dest = if app.capture.shot.processing == "copy" {
        format!("{}/formalshell/shot.png", runtime_dir())
    } else {
        app.capture.shot.pending_path.clone()
    };
    let g = app.capture.generation();
    app.capture.shot.grab = Some(g);
    let Some(rt) = &app.runtime else { return };
    let crop = svc::Crop { src, dest, x0, y0, x1, y1 };
    rt.service(move |ctx| svc::crop(ctx, g, crop));
}

/// The crop landed: the clipboard step runs as the grab, so its exit posts
/// the notification every other shot posts.
fn cropped(app: &mut App, generation: u64, result: Result<String, String>) {
    if app.capture.shot.grab != Some(generation) {
        return;
    }
    let path = match result {
        Ok(path) => path,
        Err(why) => {
            app.capture.shot.grab = None;
            let s = &mut app.capture.shot;
            s.busy = false;
            s.pending_path.clear();
            s.last_error = why.clone();
            eprintln!("ScreenshotIpc: {why}");
            notify(app, "SCREENSHOT FAILED", &why);
            return;
        }
    };
    let script = match app.capture.shot.processing.as_str() {
        "save" => "true",
        "copy" => r#"wl-copy --type image/png < "$FS_SHOT_PATH"; status=$?; rm -f "$FS_SHOT_PATH"; exit $status"#,
        _ => r#"wl-copy --type image/png < "$FS_SHOT_PATH""#,
    };
    let g = app.capture.generation();
    app.capture.shot.grab = Some(g);
    spawn(app, Run::sh(Job::ShotGrab, g, script).env("FS_SHOT_PATH", path));
}

fn on_picked_record(app: &mut App, r: Rect, output: String) {
    app.capture.shot.watch += 1;
    let s = &mut app.capture.shot;
    s.busy = false;
    s.pending_path.clear();
    s.last_error.clear();
    picker_done(app);
    let audio = settings::RECORDING_AUDIO.get(&app.store.config).to_owned();
    let geometry = format!("{},{} {}x{}", r.x.round(), r.y.round(), r.width.round(), r.height.round());
    let Err(why) = record::start_at(app, &geometry, &output, "region", &audio, None) else { return };
    let answer = format!("error: {why}");
    eprintln!("ScreenshotIpc: {answer}");
    app.capture.shot.last_error = answer.clone();
    notify(app, "RECORDING FAILED", &answer);
}

fn on_cancelled(app: &mut App) {
    if !app.capture.shot.busy {
        return;
    }
    app.capture.shot.watch += 1;
    let s = &mut app.capture.shot;
    s.busy = false;
    s.pending_path.clear();
    s.last_cancelled = true;
    s.last_error.clear();
}

// picker lifecycle

pub fn picker_open(app: &mut App, mode: &str) -> Result<(), String> {
    if app.capture.picker.open {
        return Err("picker already open".into());
    }
    let p = &mut app.capture.picker;
    p.mode = mode.into();
    p.action = "shot".into();
    p.drag = None;
    p.hover = None;
    p.cursor = -1;
    p.frames.clear();
    crate::services::compositor::send(crate::services::compositor::Command::RefreshWindows);
    if mode == "fullscreen" {
        let c = cands(app);
        let Some(r) = c.focused_output_rect().and_then(|o| o.rect) else { return Err("no focused output".into()) };
        on_picked(app, r);
        return Ok(());
    }
    freeze(app);
    Ok(())
}

fn freeze(app: &mut App) {
    let screens = app.capture_screens();
    let g = app.capture.generation();
    app.capture.picker.generation = g;
    app.capture.picker.pending_freezes = screens.len();
    if screens.is_empty() {
        picker_cancelled(app, "no outputs to capture");
        return;
    }
    let dir = runtime_dir();
    for s in screens {
        let path = format!("{dir}/formalshell-capture-{}.png", s.name);
        let run = Run::new(
            Job::Freeze,
            g,
            ["sh", "-c", r#"mkdir -p "$(dirname "$2")" && exec grim -o "$1" "$2""#, "sh", &s.name, &path].map(String::from).to_vec(),
        )
        .tag(s.name.clone());
        spawn(app, run);
    }
}

fn freeze_exit(app: &mut App, e: Exit) {
    if e.generation != app.capture.picker.generation || app.capture.picker.pending_freezes == 0 {
        return;
    }
    let path = format!("{}/formalshell-capture-{}.png", runtime_dir(), e.tag);
    if e.started && e.code == 0 {
        app.capture_decode(e.generation, e.tag, path);
        return;
    }
    let why = if !e.started { "grim failed to start".to_owned() } else { e.stderr.trim().to_owned() };
    freeze_done(app, &e.tag, None, &why);
}

/// One output's freeze settled, decoded or not.
pub fn freeze_done(app: &mut App, output: &str, frame: Option<crate::scene::Bitmap>, why: &str) {
    let p = &mut app.capture.picker;
    if frame.is_none() {
        p.last_freeze_error = format!("{output}: {}", if why.is_empty() { "grim produced nothing" } else { why });
    } else {
        p.frames.insert(output.to_owned(), frame);
    }
    p.pending_freezes = p.pending_freezes.saturating_sub(1);
    if p.pending_freezes > 0 {
        return;
    }
    if p.frames.is_empty() {
        eprintln!("RegionPicker: no output could be frozen: {why}");
        let reason = format!("freeze failed: {}", if why.is_empty() { "grim produced nothing" } else { why });
        picker_cancelled(app, &reason);
        return;
    }
    app.capture.picker.open = true;
    app.picker_sync();
}

/// The `cancelled` signal: a close, not a completed pick.
fn picker_cancelled(app: &mut App, _reason: &str) {
    on_cancelled(app);
}

pub fn picker_close(app: &mut App, reason: &str) -> Result<(), String> {
    let p = &mut app.capture.picker;
    if !p.open && p.pending_freezes == 0 {
        return Err("picker not open".into());
    }
    p.open = false;
    p.pending_freezes = 0;
    p.frames.clear();
    app.capture.picker.generation = app.capture.generation();
    app.picker_sync();
    picker_cancelled(app, reason);
    Ok(())
}

/// Tears the surface down after a pick completed: no `cancelled`.
pub fn picker_done(app: &mut App) {
    let p = &mut app.capture.picker;
    p.open = false;
    p.pending_freezes = 0;
    p.frames.clear();
    app.capture.picker.generation = app.capture.generation();
    app.picker_sync();
}

pub fn picker_commit(app: &mut App, whole_output: bool) -> Result<(), String> {
    if !app.capture.picker.open {
        return Err("picker not open".into());
    }
    let c = cands(app);
    let Some(sel) = app.capture.picker.current(&c) else { return Err("nothing selected".into()) };
    if whole_output {
        let anchor = match sel.rect {
            Some(r) => c.outputs.iter().find(|o| o.rect.is_some_and(|o| o.contains(r.x, r.y))),
            None => c.focused_output_rect(),
        };
        let Some(out) = anchor.or(c.focused_output_rect()).and_then(|o| o.rect) else {
            return Err("no output under the selection".into());
        };
        finish(app, out, &c);
        return Ok(());
    }
    let Some(r) = sel.rect else { return Err(format!("no geometry for \"{}\"", sel.label)) };
    finish(app, r, &c);
    Ok(())
}

/// A freeform drag released: taken as it stands.
pub fn finish_rect(app: &mut App, r: Rect) {
    let c = cands(app);
    finish(app, r, &c);
}

/// A shot is cut from the freeze; a recording takes the surface down first,
/// since wf-recorder records live content.
fn finish(app: &mut App, r: Rect, c: &Cands) {
    let g = app.capture.picker.generation;
    if app.capture.picker.recording() {
        app.capture.picker.open = false;
        app.picker_sync();
        let output = c.output_name_for(r);
        app.capture_after(120, move |app| {
            if app.capture.picker.generation == g {
                on_picked_record(app, r, output);
            }
        });
        return;
    }
    crop_shot(app, r, c);
}

pub fn picker_key(app: &mut App, name: &str) -> Result<(), String> {
    if !app.capture.picker.open {
        return Err("picker not open".into());
    }
    let c = cands(app);
    let answer = match name {
        "return" => return picker_commit(app, false),
        "ctrl-return" => return picker_commit(app, true),
        "tab" => {
            app.capture.picker.move_cursor(&c, 1);
            Ok(())
        }
        "shift-tab" => {
            app.capture.picker.move_cursor(&c, -1);
            Ok(())
        }
        "left" | "right" | "up" | "down" => {
            app.capture.picker.move_spatial(&c, name);
            Ok(())
        }
        "escape" => return picker_close(app, "cancelled from the picker"),
        "1" | "2" | "3" | "4" | "5" | "6" => app.capture.picker.set_tool(&c, name.parse::<i32>().unwrap_or(0) - 1),
        _ => Err(format!("unknown key {name}")),
    };
    app.picker_sync();
    answer
}

pub fn picker_status(app: &App) -> Value {
    let c = cands(app);
    let screens = app.capture_screens().len();
    app.capture.picker.status(&c, &runtime_dir(), screens)
}

// capture (text and colour)

fn grab_watchdog(app: &mut App) {
    let secs = settings::CAPTURE_TIMEOUT_SECONDS.get(&app.store.config);
    app.capture.grab.watch += 1;
    let token = app.capture.grab.watch;
    app.capture_after((secs * 1000.0) as u64, move |app| {
        if app.capture.grab.watch == token {
            let _ = grab_cancel(app, &format!("no selection after {}s", secs.round()));
        }
    });
}

pub fn grab_start(app: &mut App, mode: &str, geometry: &str) -> Result<(), String> {
    if app.capture.grab.busy {
        return Err("capture already in flight".into());
    }
    let g = &mut app.capture.grab;
    g.busy = true;
    g.mode = mode.into();
    g.last_cancelled = false;
    g.last_error.clear();
    if !geometry.is_empty() {
        if mode == "color" { run_pick(app, geometry) } else { run_ocr(app, geometry) }
        return Ok(());
    }
    let script = if mode == "color" {
        r#"exec slurp -p -w "$FS_SLURP_WEIGHT" -b "$FS_SLURP_BG" -c "$FS_SLURP_BORDER""#
    } else {
        SLURP_REGION
    };
    let g_ = app.capture.generation();
    app.capture.grab.slurp = Some(g_);
    spawn(app, slurp_run(app, Job::GrabSlurp, g_, ColorRole::Primary, script));
    grab_watchdog(app);
    Ok(())
}

fn run_ocr(app: &mut App, geometry: &str) {
    let dir = format!("{}/formalshell", runtime_dir());
    let base = format!("{dir}/ocr");
    let lang = settings::OCR_LANGUAGE.get(&app.store.config).to_owned();
    let script = concat!(
        "mkdir -p \"$FS_TMP_DIR\" || exit 2\n",
        "grim -g \"$FS_OCR_GEOM\" \"$FS_OCR_PNG\" || exit 2\n",
        "tesseract \"$FS_OCR_PNG\" \"$FS_OCR_BASE\" --oem 1 --psm 6 -l \"$FS_OCR_LANG\" --dpi 300 -c preserve_interword_spaces=1 || exit 2\n",
        "tr -d \"\\f\" < \"$FS_OCR_BASE.txt\" > \"$FS_OCR_TXT\" || exit 2\n",
        "rm -f \"$FS_OCR_PNG\" \"$FS_OCR_BASE.txt\"\n",
        "[ -n \"$(tr -d \"[:space:]\" < \"$FS_OCR_TXT\")\" ] || exit 3\n",
        "wl-copy --type text/plain < \"$FS_OCR_TXT\" || exit 2\n",
        "cat \"$FS_OCR_TXT\"\n",
    );
    let g_ = app.capture.generation();
    app.capture.grab.ocr = Some(g_);
    let run = Run::sh(Job::Ocr, g_, script)
        .env("FS_TMP_DIR", dir)
        .env("FS_OCR_GEOM", geometry)
        .env("FS_OCR_PNG", format!("{base}.png"))
        .env("FS_OCR_BASE", base.clone())
        .env("FS_OCR_TXT", format!("{base}.out"))
        .env("FS_OCR_LANG", lang);
    spawn(app, run);
}

fn run_pick(app: &mut App, geometry: &str) {
    let dir = format!("{}/formalshell", runtime_dir());
    let script = concat!(
        "mkdir -p \"$FS_TMP_DIR\" || exit 2\n",
        "grim -g \"$FS_PICK_GEOM\" -t ppm \"$FS_PICK_PPM\" || exit 2\n",
        "out=$(tail -c 3 \"$FS_PICK_PPM\" | od -An -tu1) || exit 2\n",
        "rm -f \"$FS_PICK_PPM\"\n",
        "printf '%s' \"$out\"\n",
    );
    let g_ = app.capture.generation();
    app.capture.grab.pick = Some(g_);
    let run = Run::sh(Job::Pick, g_, script)
        .env("FS_TMP_DIR", dir.clone())
        .env("FS_PICK_GEOM", geometry)
        .env("FS_PICK_PPM", format!("{dir}/pick.ppm"));
    spawn(app, run);
}

pub fn grab_cancel(app: &mut App, reason: &str) -> Result<(), String> {
    if !app.capture.grab.busy {
        return Err("no capture in flight".into());
    }
    app.capture.grab.watch += 1;
    let g = &mut app.capture.grab;
    let (a, b, c) = (g.slurp.take(), g.ocr.take(), g.pick.take());
    for g_ in [a, b, c] {
        app.capture.stop(g_);
    }
    let g = &mut app.capture.grab;
    g.busy = false;
    g.mode.clear();
    g.last_error.clear();
    g.last_cancelled = true;
    notify(app, "CAPTURE CANCELLED", reason);
    Ok(())
}

fn grab_fail(app: &mut App, why: String) {
    let g = &mut app.capture.grab;
    g.busy = false;
    g.mode.clear();
    eprintln!("CaptureIpc: {why}");
    g.last_error = why.clone();
    notify(app, "CAPTURE FAILED", &why);
}

pub fn grab_status(app: &App) -> Value {
    let g = &app.capture.grab;
    json!({
        "capturing": g.busy,
        "mode": g.mode,
        "lastHex": g.last_hex,
        "lastText": g.last_text,
        "lastError": g.last_error,
        "lastCancelled": g.last_cancelled,
    })
}

fn grab_exit(app: &mut App, e: Exit) {
    match e.job {
        Job::GrabSlurp => {
            if app.capture.grab.slurp != Some(e.generation) {
                return;
            }
            app.capture.grab.slurp = None;
            app.capture.grab.watch += 1;
            let geometry = model::parse_geometry(&e.stdout);
            if e.started && e.code == 0 && !geometry.is_empty() {
                if app.capture.grab.mode == "color" { run_pick(app, &geometry) } else { run_ocr(app, &geometry) }
                return;
            }
            if e.started && e.code == 1 {
                let g = &mut app.capture.grab;
                g.busy = false;
                g.mode.clear();
                g.last_cancelled = true;
                return;
            }
            let fallback = if e.code == 0 { "slurp reported no geometry".into() } else { format!("slurp exited {}", e.code) };
            grab_fail(app, or_code(&e.stderr, fallback));
        }
        Job::Ocr => {
            if app.capture.grab.ocr != Some(e.generation) {
                return;
            }
            app.capture.grab.ocr = None;
            let code = if e.started { e.code } else { -1 };
            match model::ocr_outcome(code) {
                model::OcrOutcome::Ok => {
                    let g = &mut app.capture.grab;
                    g.busy = false;
                    g.mode.clear();
                    g.last_text = e.stdout.trim().to_owned();
                    g.last_error.clear();
                    let text = g.last_text.clone();
                    notify(app, "TEXT COPIED", &text);
                }
                model::OcrOutcome::Empty => {
                    let g = &mut app.capture.grab;
                    g.busy = false;
                    g.mode.clear();
                    g.last_text.clear();
                    g.last_error.clear();
                    notify(app, "NO TEXT FOUND", "the selected region held no readable text");
                }
                model::OcrOutcome::Failed => grab_fail(app, or_code(&e.stderr, format!("ocr exited {}", e.code))),
            }
        }
        Job::Pick => {
            if app.capture.grab.pick != Some(e.generation) {
                return;
            }
            app.capture.grab.pick = None;
            let hex = if e.started && e.code == 0 { model::hex_from_ppm_bytes(&e.stdout) } else { String::new() };
            if hex.is_empty() {
                grab_fail(app, or_code(&e.stderr, format!("could not read the pixel (grim exited {})", e.code)));
                return;
            }
            let g = &mut app.capture.grab;
            g.busy = false;
            g.mode.clear();
            g.last_hex = hex.clone();
            g.last_error.clear();
            let g_ = app.capture.generation();
            spawn(app, Run::sh(Job::Copy, g_, r#"printf '%s' "$FS_PICK_HEX" | wl-copy --type text/plain"#).env("FS_PICK_HEX", hex));
        }
        Job::Copy => {
            if !e.started || e.code != 0 {
                grab_fail(app, or_code(&e.stderr, format!("wl-copy exited {}", e.code)));
                return;
            }
            let hex = app.capture.grab.last_hex.clone();
            notify(app, "COLOR COPIED", &hex);
        }
        _ => {}
    }
}

// events

/// Everything the service thread reported since the last call.
pub fn events(app: &mut App) {
    let events = std::mem::take(&mut app.store.capture.events);
    for event in events {
        match event {
            Event::Spawned { generation, pid } => {
                if let Some(at) = app.capture.dropped.iter().position(|g| *g == generation) {
                    app.capture.dropped.swap_remove(at);
                    svc::terminate(pid);
                } else {
                    app.capture.pids.insert(generation, pid);
                    record::spawned(app, generation, pid);
                }
            }
            Event::Image { id, raw } => {
                if let Some(raw) = raw {
                    app.apply(Op::Picture(id, raw));
                }
            }
            Event::Cropped { generation, result } => cropped(app, generation, result),
            Event::Frame { generation, output, frame } => {
                if generation == app.capture.picker.generation && app.capture.picker.pending_freezes > 0 {
                    let why = if frame.is_none() { "the frame could not be decoded" } else { "" };
                    freeze_done(app, &output, frame, why);
                }
            }
            Event::Exited(e) => {
                app.capture.pids.remove(&e.generation);
                if let Some(at) = app.capture.dropped.iter().position(|g| *g == e.generation) {
                    app.capture.dropped.swap_remove(at);
                }
                match e.job {
                    Job::ShotSlurp | Job::ShotGrab | Job::Editor => shot_exit(app, e),
                    Job::Freeze => freeze_exit(app, e),
                    Job::GrabSlurp | Job::Ocr | Job::Pick | Job::Copy => grab_exit(app, e),
                    Job::Quiet => {}
                    _ => record::exit(app, e),
                }
            }
        }
    }
}
