//! The palette and the chrome the compositor reads: the theme.json half
//! and the theme engine. The pure halves live in
//! fs-theme; this runs the files, the matugen children and the schedule.
//!
//! [`watch`] publishes theme.json as it stands. [`run`] is the engine: it
//! reacts to [`Inputs`] (settings, wallpaper, mode) and asks for mode writes
//! through [`Request`], since state.json is not its to write.
//!
//! A retheme queues rather than overlapping: one requested while a run is
//! in flight reruns once after it, and the run in flight is never killed
//! mid-write. Its wallpaper and mode are captured when it starts, so a flip
//! mid-run cannot send its outputs down the other path.
//!
//! The source colour is pinned to matugen's own rank 0, not left to
//! `--prefer`. Some decision has to be forced: matugen prompts when an image
//! yields more than one candidate, a child has no TTY to prompt on, and an
//! unforced run fails outright (50 of the owner's 57 wallpapers on g815
//! produce several). Every `--prefer` value is a scalar tiebreak that never
//! asks what the wallpaper looks like: `lightness` sat a mean 50 degrees of
//! hue off the image's own saturation-weighted dominant hue over those 57
//! (2026-08-14), seeding a tan scheme under a blue tower. matugen ranks the
//! candidates by material's Score and prints the ranking under `-d`, and
//! rank 0 is the image's colour by that reckoning (mean 14 degrees off). So
//! a retheme runs matugen twice: a `--dry-run` probe reads the ranking off
//! stderr, then the real run pins the source with `--prefer
//! closest-to-fallback --fallback-color <rank0>`, which resolves to that
//! exact candidate since its distance to itself is zero. The real run stays
//! `matugen image`, so a user template reading image-derived variables
//! still gets them. A probe with no ranking falls back to `--prefer
//! saturation` (the least bad scalar on the same corpus) and warns. The
//! choice depends on the wallpaper alone, never on the mode: a mode-matched
//! pair used to flip one wallpaper's hue family across a toggle, while `-m`
//! alone tones the scheme.
//!
//! A flat desktop (`wallpaper.color`, a picker swatch) seeds the scheme with
//! `matugen color hex` on that colour, the probe skipped: there is nothing
//! to rank.
//!
//! A wallpaper whose path names a pinned palette (flexoki, zenbones) still
//! runs matugen, but nothing reads its scheme: every template is rewritten
//! to the palette's own tones first, matugen renders the copies, and
//! theme.json plus the Hyprland colours take the palette's shadcn view
//! through the static write.

use std::cell::{Cell, RefCell};
use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use async_io::Timer;
use chrono::Local;
use fs_theme::chrome;
use fs_theme::matugen::{self, BuildOpts};
use fs_theme::palette::{self, Pin};
use fs_theme::sun::{self, Override, SunTimes};
use fs_theme::theme::Theme;
use futures_lite::future;
use serde_json::{Map, Value};

use crate::runtime::Ctx;
use crate::store;
use super::compositor::{self, Kind};
use super::state;

/// What surfaces read: the resolved theme, the palette under it and the
/// two facts `theme status` reports.
pub struct State {
    pub palette: Map<String, Value>,
    pub settings: Value,
    pub theme: Theme,
    pub json_present: bool,
    pub schedule: Option<SunTimes>,
}

impl Default for State {
    fn default() -> Self {
        let palette = palette::fallback("dark");
        let settings = Value::Object(Map::new());
        let theme = Theme::resolve(get(&settings), &palette);
        Self { palette, settings, theme, json_present: false, schedule: None }
    }
}

pub enum Diff {
    Palette(Map<String, Value>),
    Settings(Value),
    JsonPresent(bool),
    Schedule(Option<SunTimes>),
}

impl State {
    pub fn apply(&mut self, diff: Diff) -> bool {
        match diff {
            Diff::Palette(p) if p != self.palette => self.palette = p,
            Diff::Settings(s) if s != self.settings => self.settings = s,
            Diff::JsonPresent(p) if p != self.json_present => {
                self.json_present = p;
                return true;
            }
            Diff::Schedule(s) if s != self.schedule => {
                self.schedule = s;
                return true;
            }
            _ => return false,
        }
        let motion_scale = self.theme.motion_scale;
        self.theme = Theme::resolve(get(&self.settings), &self.palette);
        self.theme.motion_scale = motion_scale;
        true
    }
}

/// `Config.get` over a settings object: one dotted path, `None` when any
/// step of it is absent.
pub fn get(settings: &Value) -> impl Fn(&str) -> Option<Value> + '_ {
    move |path| {
        let mut node = settings;
        for part in path.split('.') {
            node = node.as_object()?.get(part)?;
        }
        Some(node.clone())
    }
}

/// [`get`] over a settings object it owns.
pub fn getter(settings: Value) -> impl Fn(&str) -> Option<Value> {
    move |path| get(&settings)(path)
}

/// Where the engine reads and writes, and the binaries it runs.
#[derive(Clone, Debug)]
pub struct Env {
    pub home: PathBuf,
    pub state_dir: PathBuf,
    pub config_dir: PathBuf,
    pub template_dir: PathBuf,
    /// The compositor asked to reload after a write, when the session is
    /// the backend's own.
    pub compositor: Option<Kind>,
    pub matugen: OsString,
    pub hyprctl: OsString,
    pub dconf: OsString,
}

impl Env {
    /// `FS_TEMPLATE_DIR` is the shell's own matugen templates, which the
    /// package wrapper points at.
    pub fn from_process() -> Self {
        let var = |k: &str| std::env::var_os(k).filter(|v| !v.is_empty());
        let home = PathBuf::from(var("HOME").unwrap_or_default());
        let state = var("XDG_STATE_HOME").map_or_else(|| home.join(".local/state"), PathBuf::from);
        let config = var("XDG_CONFIG_HOME").map_or_else(|| home.join(".config"), PathBuf::from);
        Self {
            template_dir: var("FS_TEMPLATE_DIR").map_or_else(|| PathBuf::from("crates/fs-theme/templates"), PathBuf::from),
            state_dir: state.join("formalshell"),
            config_dir: config,
            compositor: match compositor::kind() {
                Kind::Hyprland if var("HYPRLAND_INSTANCE_SIGNATURE").is_some() => Some(Kind::Hyprland),
                Kind::Mango if var("MANGO_INSTANCE_SIGNATURE").is_some() => Some(Kind::Mango),
                _ => None,
            },
            matugen: "matugen".into(),
            hyprctl: "hyprctl".into(),
            dconf: "dconf".into(),
            home,
        }
    }

    pub fn theme_json(&self) -> PathBuf {
        self.state_dir.join("theme.json")
    }
    fn theme_json_tmp(&self) -> PathBuf {
        self.state_dir.join("theme.json.tmp")
    }
    fn merged_config(&self) -> PathBuf {
        self.state_dir.join("matugen-merged.toml")
    }
    fn hypr_colors_tmp(&self) -> PathBuf {
        self.state_dir.join("formalshell-colors.lua.tmp")
    }
    pub fn hypr_colors(&self) -> PathBuf {
        self.config_dir.join("hypr/formalshell-colors.lua")
    }
    pub fn hypr_chrome(&self) -> PathBuf {
        self.config_dir.join("hypr/formalshell-chrome.lua")
    }
    /// mango reads `~/.config/mango` whatever XDG_CONFIG_HOME says.
    pub fn mango_colors(&self) -> PathBuf {
        self.home.join(".config/mango/formalshell-colors.conf")
    }
    pub fn mango_chrome(&self) -> PathBuf {
        self.home.join(".config/mango/formalshell-chrome.conf")
    }
    /// Where a pinned run stages its rewritten templates, regenerated per run.
    fn pinned_dir(&self) -> PathBuf {
        self.state_dir.join("pinned-templates")
    }
    fn user_config(&self) -> PathBuf {
        self.home.join(".config/matugen/config.toml")
    }
    fn drop_in_dir(&self) -> PathBuf {
        self.home.join(".config/formalshell/matugen.d")
    }
}

/// What the engine reacts to. Task 5 sends one whenever any of it moves.
#[derive(Clone, Debug, PartialEq)]
pub struct Inputs {
    pub settings: Value,
    /// `State.wallpaper`, empty with none set.
    pub wallpaper: String,
    /// `State.mode`, "dark" or "light".
    pub mode: String,
    pub mode_override: Option<Override>,
    /// Latitude and longitude, when one is known.
    pub location: Option<(f64, f64)>,
}

/// The state.json writes the engine asks for.
#[derive(Clone, Debug, PartialEq)]
pub enum Request {
    SetMode(String),
    SetModeOverride(Option<Override>),
}

/// `theme.mode`: "" leaves the mode to state.json, "dark" and "light" pin
/// it, "auto" hands it to the schedule. Anything else reads as "".
pub fn mode_key(settings: &Value) -> &'static str {
    match get(settings)("theme.mode").as_ref().and_then(Value::as_str) {
        Some("dark") => "dark",
        Some("light") => "light",
        Some("auto") => "auto",
        _ => "",
    }
}

/// `theme mode dark|light|toggle`: the mode to write, or the error string
/// the IPC target answers with. A pinned key says so rather than flipping
/// and snapping back a tick later.
pub fn request_mode(m: &str, current: &str, key: &str) -> Result<String, String> {
    let target = if m == "toggle" { if current == "dark" { "light" } else { "dark" } } else { m };
    if target != "dark" && target != "light" {
        return Err("error: mode must be dark, light, or toggle".into());
    }
    if key == "dark" || key == "light" {
        return Err(format!("error: theme.mode pins the mode to {key}"));
    }
    Ok(target.to_owned())
}

/// The schedule half of `theme status`, `None` under any key but "auto".
#[derive(Clone, Debug, PartialEq)]
pub struct ScheduleStatus {
    pub sunrise: String,
    pub sunset: String,
    pub polar: &'static str,
    pub source: &'static str,
}

pub fn schedule_status(key: &str, times: Option<&SunTimes>) -> Option<ScheduleStatus> {
    if key != "auto" {
        return None;
    }
    Some(match times {
        Some(t) => ScheduleStatus {
            sunrise: sun::hhmm(t.sunrise_minutes),
            sunset: sun::hhmm(t.sunset_minutes),
            polar: t.polar.as_str(),
            source: "location",
        },
        None => ScheduleStatus { sunrise: String::new(), sunset: String::new(), polar: "", source: "fallback" },
    })
}

/// What the key resolves to right now, which is what the mode already
/// carries except in the instant between a boundary and its write.
pub fn effective_mode(key: &str, times: Option<&SunTimes>, mode_override: Option<&Override>, current: &str) -> String {
    sun::effective_mode(key, &Local::now(), times, mode_override).map_or_else(|| current.to_owned(), str::to_owned)
}

fn warn(what: impl std::fmt::Display) {
    eprintln!("theme: {what}");
}

async fn blocking<T: Send + 'static>(ctx: &Ctx, job: impl FnOnce() -> io::Result<T> + Send + 'static) -> io::Result<T> {
    ctx.pool().run(job).await.unwrap_or_else(|| Err(io::Error::other("pool job panicked")))
}

/// Written beside the target and renamed over it, so a reader never sees it
/// half written. The staging name carries the pid: two publishes of one
/// path can overlap, and a fixed name loses the second rename.
fn publish_file(path: &Path, content: &str) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut staged = path.as_os_str().to_owned();
    staged.push(format!(".tmp.{}", std::process::id()));
    std::fs::write(&staged, content)?;
    std::fs::rename(&staged, path)
}

/// theme.json as read here: per key, a missing or malformed
/// value falls back to zinc and the rest stays.
pub(crate) fn read_palette(path: &Path) -> (Map<String, Value>, bool) {
    match std::fs::read_to_string(path) {
        Ok(text) => {
            let parsed: Option<Value> = serde_json::from_str(&text).ok();
            (palette::merge_with_fallback(parsed.as_ref()), true)
        }
        Err(_) => (palette::fallback("dark"), false),
    }
}

/// Publishes theme.json as it stands, the placeholder zinc when absent.
pub async fn watch(ctx: Ctx) {
    let env = Env::from_process();
    let path = env.theme_json();
    let read = blocking(&ctx, move || Ok(read_palette(&path))).await;
    let (palette, present) = read.unwrap_or_else(|_| (palette::fallback("dark"), false));
    ctx.publish(store::Diff::Theme(Diff::Palette(palette)));
    ctx.publish(store::Diff::Theme(Diff::JsonPresent(present)));
}

struct Engine {
    ctx: Ctx,
    env: Env,
    requests: async_channel::Sender<Request>,
    inputs: RefCell<Option<Inputs>>,
    times: RefCell<Option<SunTimes>>,
    retheme: async_channel::Sender<()>,
    reloading: Rc<Cell<bool>>,
}

static RETHEME: OnceLock<async_channel::Sender<()>> = OnceLock::new();
static INPUTS: OnceLock<async_channel::Sender<Inputs>> = OnceLock::new();

/// `theme retheme`: one more run, queued behind any in flight. Callable from
/// any thread; dropped before the engine has started.
pub fn retheme() {
    if let Some(tx) = RETHEME.get() {
        let _ = tx.try_send(());
    }
}

/// Hands the engine what it reacts to, from any thread.
pub fn send_inputs(inputs: Inputs) {
    if let Some(tx) = INPUTS.get() {
        let _ = tx.try_send(inputs);
    }
}

/// The engine wired to the shell: inputs from the store, its mode writes
/// into state.json.
pub fn start(ctx: &Ctx) {
    let (inputs_tx, inputs_rx) = async_channel::unbounded();
    let (requests_tx, requests_rx) = async_channel::unbounded::<Request>();
    let _ = INPUTS.set(inputs_tx);
    ctx.spawn(run(ctx.clone(), Env::from_process(), inputs_rx, requests_tx));
    ctx.spawn(async move {
        while let Ok(request) = requests_rx.recv().await {
            let field = match request {
                Request::SetMode(mode) => state::Field::Mode(mode),
                Request::SetModeOverride(None) => state::Field::ModeOverride(Value::Null),
                Request::SetModeOverride(Some(o)) => {
                    state::Field::ModeOverride(serde_json::json!({"mode": o.mode, "untilMs": o.until_ms}))
                }
            };
            state::set(vec![field]);
        }
    });
}

/// `State.modeOverride` as state.json holds it.
pub fn parse_override(value: &Value) -> Option<Override> {
    let mode = value.get("mode")?.as_str()?.to_owned();
    let until_ms = value.get("untilMs")?.as_f64()? as i64;
    Some(Override { mode, until_ms })
}

/// The engine. Returns when `inputs` closes.
pub async fn run(ctx: Ctx, env: Env, inputs: async_channel::Receiver<Inputs>, requests: async_channel::Sender<Request>) {
    let (retheme, queued) = async_channel::unbounded();
    let _ = RETHEME.set(retheme.clone());
    let engine = Rc::new(Engine {
        ctx: ctx.clone(),
        env,
        requests,
        inputs: RefCell::new(None),
        times: RefCell::new(None),
        retheme,
        reloading: Rc::new(Cell::new(false)),
    });
    let worker = engine.clone();
    ctx.spawn(async move {
        while queued.recv().await.is_ok() {
            while queued.try_recv().is_ok() {}
            worker.pipeline().await;
        }
    });

    let mut tick: Option<Instant> = None;
    loop {
        let timer = async {
            match tick {
                Some(at) => {
                    Timer::at(at).await;
                    None
                }
                None => future::pending().await,
            }
        };
        let next = future::or(async { Some(inputs.recv().await.ok()) }, timer).await;
        match next {
            Some(None) => return,
            Some(Some(new)) => engine.receive(new).await,
            None => engine.apply_mode_key(),
        }
        let auto = engine.inputs.borrow().as_ref().is_some_and(|i| mode_key(&i.settings) == "auto");
        tick = match (auto, tick) {
            (false, _) => None,
            (true, Some(at)) if at > Instant::now() => Some(at),
            (true, _) => Some(Instant::now() + Duration::from_secs(60)),
        };
    }
}

impl Engine {
    async fn receive(&self, new: Inputs) {
        let old = self.inputs.replace(Some(new.clone()));
        let Some(old) = old else {
            self.start().await;
            return;
        };
        if mode_key(&old.settings) != mode_key(&new.settings) || old.location != new.location {
            self.apply_mode_key();
        }
        if old.mode != new.mode {
            self.on_mode_written();
        }
        if old.wallpaper != new.wallpaper || old.mode != new.mode {
            let _ = self.retheme.try_send(());
        }
        if chrome_text(&old.settings) != chrome_text(&new.settings) {
            self.publish_chrome().await;
        }
    }

    async fn start(&self) {
        let path = self.env.theme_json();
        let present = blocking(&self.ctx, move || Ok(path.exists())).await.unwrap_or(false);
        self.ctx.publish(store::Diff::Theme(Diff::JsonPresent(present)));
        if !present {
            let _ = self.retheme.try_send(());
        } else if let Some(inputs) = self.snapshot() {
            // Another session (Plasma's gtkconfig) rewrites color-scheme to
            // its own theme's mode, and a present theme.json skips the
            // retheme that would assert ours.
            self.sync_system_scheme(&inputs);
        }
        self.publish_chrome().await;
        self.apply_mode_key();
    }

    fn snapshot(&self) -> Option<Inputs> {
        self.inputs.borrow().clone()
    }

    fn request(&self, r: Request) {
        let _ = self.requests.try_send(r);
    }

    /// Resolves the key and asks for the mode it wants, whose write comes
    /// back as new inputs, re-runs matugen and recolours every surface.
    fn apply_mode_key(&self) {
        let Some(inputs) = self.snapshot() else {
            return;
        };
        let key = mode_key(&inputs.settings);
        if key.is_empty() {
            return;
        }
        let now = Local::now();
        let times = match (key, inputs.location) {
            ("auto", Some((lat, lon))) => sun::sun_times(&now, lat, lon, None),
            _ => None,
        };
        self.times.replace(times);
        self.ctx.publish(store::Diff::Theme(Diff::Schedule(times)));
        let mut over = inputs.mode_override.clone();
        if let Some(o) = &over
            && (key != "auto" || o.until_ms <= now.timestamp_millis())
        {
            self.request(Request::SetModeOverride(None));
            over = None;
        }
        if let Some(want) = sun::effective_mode(key, &now, times.as_ref(), over.as_ref())
            && want != inputs.mode
        {
            self.request(Request::SetMode(want.to_owned()));
        }
    }

    /// Every mode write passes here. One that disagrees with "auto" is a
    /// snooze of one cycle, expiring at the next scheduled change; a pinned
    /// key takes the mode straight back.
    fn on_mode_written(&self) {
        let Some(inputs) = self.snapshot() else {
            return;
        };
        let key = mode_key(&inputs.settings);
        if key.is_empty() {
            return;
        }
        let now = Local::now();
        let times = *self.times.borrow();
        let resolved = sun::effective_mode(key, &now, times.as_ref(), inputs.mode_override.as_ref());
        match resolved {
            None => {}
            Some(r) if r == inputs.mode => {}
            Some(_) if key != "auto" => self.request(Request::SetMode(key.to_owned())),
            Some(_) => self.request(Request::SetModeOverride(Some(Override {
                mode: inputs.mode.clone(),
                until_ms: sun::next_change(&now, times.as_ref()).timestamp_millis(),
            }))),
        }
    }

    /// Neither compositor watches the files its config pulls the colours
    /// and chrome from, so every publish asks for the re-read. One at a
    /// time.
    fn reload_compositor(&self) {
        let Some(kind) = self.env.compositor else { return };
        if self.reloading.get() {
            return;
        }
        self.reloading.set(true);
        let reloading = self.reloading.clone();
        let hyprctl = self.env.hyprctl.clone();
        self.ctx.spawn(async move {
            match kind {
                Kind::Hyprland => {
                    if !run_status(&hyprctl, &["reload"]).await {
                        warn("hyprctl reload failed");
                    }
                }
                Kind::Mango => compositor::mango::reload().await,
            }
            reloading.set(false);
        });
    }

    /// mango's two files, written for any session since they cost nothing:
    /// its colours resolve the window role's palette roles, so they follow
    /// both theme.json and the chrome.
    async fn publish_mango(&self) {
        let Some(inputs) = self.snapshot() else {
            return;
        };
        let chrome = window_chrome(&inputs.settings);
        let (env, theme_json) = (self.env.clone(), self.env.theme_json());
        let result = blocking(&self.ctx, move || {
            let (palette, _) = read_palette(&theme_json);
            for (path, wanted) in [(env.mango_chrome(), chrome::mango_chrome(&chrome)), (env.mango_colors(), chrome::mango_colors(&chrome, &palette))] {
                if !std::fs::read_to_string(&path).is_ok_and(|current| current == wanted) {
                    publish_file(&path, &wanted)?;
                }
            }
            Ok(())
        })
        .await;
        if let Err(err) = result {
            warn(format!("failed to write the mango colours and chrome: {err}"));
        }
    }

    /// The chrome a hyprland.lua reads back, outside the matugen queue:
    /// every value comes from settings.json or the table, none from the
    /// wallpaper. Skips the write when the file already says the same.
    async fn publish_chrome(&self) {
        let Some(inputs) = self.snapshot() else {
            return;
        };
        let wanted = chrome_text(&inputs.settings);
        let path = self.env.hypr_chrome();
        let result = blocking(&self.ctx, move || {
            if std::fs::read_to_string(&path).is_ok_and(|current| current == wanted) {
                return Ok(());
            }
            publish_file(&path, &wanted)
        })
        .await;
        if let Err(err) = result {
            warn(format!("failed to write formalshell-chrome.lua: {err}"));
        }
        self.publish_mango().await;
        self.reload_compositor();
    }

    /// Ordinary apps learn light and dark and the GTK theme name from
    /// org.gnome.desktop.interface, so every retheme asserts it too. dconf
    /// rather than gsettings: on NixOS the schemas live under per-package
    /// paths a bare gsettings cannot see. Fire and forget.
    fn sync_system_scheme(&self, inputs: &Inputs) {
        let dark = inputs.mode != "light";
        let s = get(&inputs.settings);
        let name = |key: &str| s(key).and_then(|v| v.as_str().map(str::to_owned)).unwrap_or_default();
        let theme_name = fs_theme::gtk::gtk_theme_name(dark, &name("gtk.theme"), &name("gtk.themeDark"));
        let scheme = if dark { "'prefer-dark'" } else { "'prefer-light'" };
        let gtk = format!("'{theme_name}'");
        let dconf = self.env.dconf.clone();
        self.ctx.spawn(async move {
            let ok = run_status(&dconf, &["write", "/org/gnome/desktop/interface/color-scheme", scheme]).await
                && run_status(&dconf, &["write", "/org/gnome/desktop/interface/gtk-theme", gtk.as_str()]).await;
            if !ok {
                warn("dconf color-scheme sync failed");
            }
        });
    }

    async fn pipeline(&self) {
        let Some(inputs) = self.snapshot() else {
            return;
        };
        self.sync_system_scheme(&inputs);
        if inputs.wallpaper.is_empty() {
            self.publish_static(palette::fallback(&inputs.mode)).await;
            return;
        }
        if let Err(err) = self.matugen_run(&inputs).await {
            warn(err);
        }
    }

    /// The tail the no-wallpaper zinc path and a pinned run share: theme.json
    /// straight from the palette, then the Hyprland colours, then a reload.
    async fn publish_static(&self, written: Map<String, Value>) {
        let json = serde_json::to_string_pretty(&Value::Object(written.clone())).unwrap_or_default();
        let colors = matugen::hyprland_colors(&written);
        let (theme_json, hypr) = (self.env.theme_json(), self.env.hypr_colors());
        let wrote = blocking(&self.ctx, move || publish_file(&theme_json, &json)).await;
        match wrote {
            Ok(()) => {
                let merged = palette::merge_with_fallback(Some(&Value::Object(written)));
                self.ctx.publish(store::Diff::Theme(Diff::Palette(merged)));
                self.ctx.publish(store::Diff::Theme(Diff::JsonPresent(true)));
            }
            Err(err) => warn(format!("failed to write static theme.json: {err}")),
        }
        if let Err(err) = blocking(&self.ctx, move || publish_file(&hypr, &colors)).await {
            warn(format!("failed to write fallback formalshell-colors.lua: {err}"));
        }
        self.publish_mango().await;
        self.reload_compositor();
    }

    async fn matugen_run(&self, inputs: &Inputs) -> Result<(), String> {
        let (user, drop_ins) = (self.env.user_config(), self.env.drop_in_dir());
        let (user_text, drop_in) = blocking(&self.ctx, move || Ok(read_user_configs(&user, &drop_ins)))
            .await
            .map_err(|e| format!("failed to read the matugen configs: {e}"))?;
        let cfg = matugen::build_config(&BuildOpts {
            shell_template_dir: self.env.template_dir.to_string_lossy().into_owned(),
            state_dir: self.env.state_dir.to_string_lossy().into_owned(),
            home_dir: self.env.home.to_string_lossy().into_owned(),
            user_config_text: user_text,
            drop_in_texts: drop_in.into_iter().collect(),
        });
        let pin = palette::pinned_palette(&inputs.wallpaper);
        let cfg = match pin {
            Some(pin) => self.rewrite_templates(&cfg, &inputs.mode, pin).await?,
            None => cfg,
        };

        let merged = self.env.merged_config();
        let cfg_text = cfg.clone();
        let state_dir = self.env.state_dir.clone();
        blocking(&self.ctx, move || {
            std::fs::create_dir_all(&state_dir)?;
            std::fs::write(&merged, cfg_text)
        })
        .await
        .map_err(|e| format!("failed to write matugen-merged.toml: {e}"))?;
        let merged = self.env.merged_config().to_string_lossy().into_owned();

        let seed = pin.map(|p| p.source.as_str()).or_else(|| super::wallpaper::flat_hex(&inputs.wallpaper).map(|h| h.trim_start_matches('#')));
        let args: Vec<String> = match seed {
            Some(seed) => ["color", "hex", seed, "-m", inputs.mode.as_str(), "-c", merged.as_str()].map(str::to_owned).to_vec(),
            None => {
                // Extraction only: --dry-run writes no template and runs no
                // command, and -d is what prints the ranking (-q would
                // silence it).
                let probe = crate::services::proc::command(&self.env.matugen)
                    .args(["-d", "image", inputs.wallpaper.as_str(), "--dry-run", "--prefer", "saturation"])
                    .stdin(std::process::Stdio::null())
                    .output()
                    .await
                    .map_err(|e| format!("matugen not found (failed to start): {e}"))?;
                let source = probe
                    .status
                    .success()
                    .then(|| matugen::ranked_source_color(&String::from_utf8_lossy(&probe.stderr)))
                    .flatten();
                if source.is_none() {
                    warn(format!(
                        "no source-color ranking from matugen ({}), falling back to --prefer saturation",
                        probe.status
                    ));
                }
                let pick: Vec<String> = match source {
                    Some(s) => vec!["--prefer".into(), "closest-to-fallback".into(), "--fallback-color".into(), s],
                    None => vec!["--prefer".into(), "saturation".into()],
                };
                ["image", inputs.wallpaper.as_str(), "-m", inputs.mode.as_str(), "-c", merged.as_str()]
                    .map(str::to_owned)
                    .into_iter()
                    .chain(pick)
                    .collect()
            }
        };
        let status = crate::services::proc::command(&self.env.matugen)
            .args(&args)
            .stdin(std::process::Stdio::null())
            .status()
            .await
            .map_err(|e| format!("matugen not found (failed to start): {e}"))?;
        if !status.success() {
            return Err(format!("matugen exited with {status}"));
        }

        if let Some(pin) = pin {
            // The rewritten templates rendered these two in the pinned
            // palette too, but the palette's shadcn view is the shell's own
            // authority for them: the greeter reads the same table with no
            // matugen in reach, and its chart ramp walks accents no
            // Material role carries.
            let (a, b) = (self.env.theme_json_tmp(), self.env.hypr_colors_tmp());
            let _ = blocking(&self.ctx, move || {
                let _ = std::fs::remove_file(a);
                let _ = std::fs::remove_file(b);
                Ok(())
            })
            .await;
            self.publish_static(pin.shadcn(&inputs.mode)).await;
            return Ok(());
        }

        let env = self.env.clone();
        blocking(&self.ctx, move || {
            std::fs::rename(env.theme_json_tmp(), env.theme_json())?;
            if let Some(dir) = env.hypr_colors().parent() {
                std::fs::create_dir_all(dir)?;
            }
            std::fs::rename(env.hypr_colors_tmp(), env.hypr_colors())
        })
        .await
        .map_err(|e| format!("failed to publish theme.json/formalshell-colors.lua: {e}"))?;
        let path = self.env.theme_json();
        if let Ok((palette, _)) = blocking(&self.ctx, move || Ok(read_palette(&path))).await {
            self.ctx.publish(store::Diff::Theme(Diff::Palette(palette)));
        }
        self.ctx.publish(store::Diff::Theme(Diff::JsonPresent(true)));
        self.publish_mango().await;
        self.reload_compositor();
        Ok(())
    }

    /// Rewrites every template the merged config points at to the pinned
    /// palette's tones and stages the copies for matugen. post_hook strings
    /// go through matugen's engine too, so the config is rewritten as well.
    /// A template that cannot be read keeps its original input_path, so
    /// matugen fails on it exactly as it would have without the pin.
    async fn rewrite_templates(&self, cfg: &str, mode: &str, pin: &'static Pin) -> Result<String, String> {
        let rewritten = matugen::substitute_pinned(cfg, mode, pin);
        let home = self.env.home.to_string_lossy().into_owned();
        let inputs: Vec<String> =
            matugen::template_inputs(&rewritten.text).iter().map(|p| matugen::expand_home(p, &home)).collect();
        if inputs.is_empty() {
            return Ok(rewritten.text);
        }
        let to_read = inputs.clone();
        let bodies = blocking(&self.ctx, move || {
            Ok(to_read.iter().map(|p| std::fs::read_to_string(p).unwrap_or_default()).collect::<Vec<_>>())
        })
        .await
        .map_err(|e| format!("failed to read the pinned templates: {e}"))?;

        let mut skipped = rewritten.skipped.clone();
        let mut staged: Vec<Option<PathBuf>> = vec![None; inputs.len()];
        let mut pairs: Vec<(PathBuf, String)> = Vec::new();
        for (i, body) in bodies.iter().enumerate() {
            if body.is_empty() {
                continue;
            }
            let out = matugen::substitute_pinned(body, mode, pin);
            for expr in out.skipped {
                if !skipped.contains(&expr) {
                    skipped.push(expr);
                }
            }
            let path = self.env.pinned_dir().join(format!("{i}.tmpl"));
            staged[i] = Some(path.clone());
            pairs.push((path, out.text));
        }
        if !skipped.is_empty() {
            warn(format!(
                "{} pin left {} expression(s) on matugen's own scheme: {}",
                pin.name,
                skipped.len(),
                skipped.join(", ")
            ));
        }
        let cfg = matugen::rewrite_template_inputs(&rewritten.text, |_, i| {
            staged.get(i).cloned().flatten().map(|p| p.to_string_lossy().into_owned())
        });
        if pairs.is_empty() {
            return Ok(cfg);
        }
        let dir = self.env.pinned_dir();
        blocking(&self.ctx, move || {
            std::fs::create_dir_all(&dir)?;
            for entry in std::fs::read_dir(&dir)? {
                let path = entry?.path();
                if path.extension().is_some_and(|e| e == "tmpl") {
                    std::fs::remove_file(path)?;
                }
            }
            for (path, text) in pairs {
                std::fs::write(path, text)?;
            }
            Ok(())
        })
        .await
        .map_err(|e| format!("failed to stage rewritten pinned templates: {e}"))?;
        Ok(cfg)
    }
}

fn window_chrome(settings: &Value) -> chrome::Chrome {
    Theme::resolve(get(settings), &palette::fallback("dark")).window_chrome()
}

/// formalshell-chrome.lua for these settings. The palette plays no part.
fn chrome_text(settings: &Value) -> String {
    chrome::hyprland_chrome(&window_chrome(settings))
}

/// The user's own matugen config verbatim, and every drop-in under
/// matugen.d as one text, in the order a shell glob lists them.
fn read_user_configs(user: &Path, drop_ins: &Path) -> (Option<String>, Option<String>) {
    let user_text = std::fs::read_to_string(user).ok().filter(|t| !t.is_empty());
    let mut files: Vec<PathBuf> = std::fs::read_dir(drop_ins)
        .map(|rd| rd.filter_map(Result::ok).map(|e| e.path()).collect())
        .unwrap_or_default();
    files.retain(|p| p.extension().is_some_and(|e| e == "toml") && p.is_file());
    files.sort();
    let mut joined = String::new();
    for f in files {
        if let Ok(text) = std::fs::read_to_string(f) {
            joined.push_str(&text);
            joined.push('\n');
        }
    }
    let joined = joined.trim().to_owned();
    (user_text, (!joined.is_empty()).then_some(joined))
}

async fn run_status(bin: &OsString, args: &[&str]) -> bool {
    crate::services::proc::command(bin)
        .args(args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .await
        .is_ok_and(|s| s.success())
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::PermissionsExt;
    use std::time::{Duration, Instant};

    use calloop::EventLoop;
    use calloop::channel::{self, Event};
    use serde_json::json;

    use super::*;
    use crate::runtime::{Msg, Publisher, Runtime};

    fn scratch(name: &str) -> PathBuf {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../target/test-tmp")
            .join(format!("theme-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn shim(dir: &Path, name: &str, body: &str) -> OsString {
        let path = dir.join(name);
        std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path.into_os_string()
    }

    /// A home, a state dir and three shims that log their argv, with
    /// matugen printing a ranking on the probe and writing both outputs on
    /// a real run.
    fn rig(name: &str) -> (Env, PathBuf) {
        let root = scratch(name);
        let bin = root.join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        let log = root.join("argv.log");
        let state = root.join("state/formalshell");
        let matugen = format!(
            r##"echo "matugen $*" >> '{log}'
if [ "$1" = "-d" ]; then
  echo '[2026-10-06T10:00:00Z DEBUG matugen::color::color] Ranked colors:' >&2
  echo '[2026-10-06T10:00:00Z DEBUG matugen::color::color] 0: #648DB8' >&2
  echo '[2026-10-06T10:00:00Z DEBUG matugen::color::color] 1: #908a61' >&2
  exit 0
fi
sleep 0.3
mkdir -p '{state}'
printf '%s' '{{"mode":"dark","primary":"#648db8"}}' > '{state}/theme.json.tmp'
printf '%s' 'return {{}}' > '{state}/formalshell-colors.lua.tmp'"##,
            log = log.display(),
            state = state.display()
        );
        let logger = |tool: &str| format!("echo \"{tool} $*\" >> '{}'", log.display());
        let env = Env {
            home: root.join("home"),
            state_dir: state,
            config_dir: root.join("home/.config"),
            template_dir: Path::new(env!("CARGO_MANIFEST_DIR")).join("../fs-theme/templates"),
            compositor: Some(Kind::Hyprland),
            matugen: shim(&bin, "matugen", &matugen),
            hyprctl: shim(&bin, "hyprctl", &logger("hyprctl")),
            dconf: shim(&bin, "dconf", &logger("dconf")),
        };
        (env, log)
    }

    fn inputs(settings: Value, wallpaper: &str, mode: &str) -> Inputs {
        Inputs { settings, wallpaper: wallpaper.into(), mode: mode.into(), mode_override: None, location: None }
    }

    #[derive(Default)]
    struct Seen {
        palettes: Vec<Map<String, Value>>,
        present: Vec<bool>,
    }

    /// Runs the engine on a real runtime, feeds it `sends` (a delay before
    /// each), and dispatches until `done` holds or ten seconds pass.
    fn drive(
        env: Env,
        sends: Vec<(Duration, Inputs)>,
        done: impl Fn(&Seen, &[Request]) -> bool,
    ) -> (Seen, Vec<Request>) {
        let mut event_loop: EventLoop<Seen> = EventLoop::try_new().unwrap();
        let (sender, inbox) = channel::channel::<Msg>();
        event_loop
            .handle()
            .insert_source(inbox, |event, _, seen: &mut Seen| match event {
                Event::Msg(Msg::Diff(store::Diff::Theme(Diff::Palette(p)))) => seen.palettes.push(p),
                Event::Msg(Msg::Diff(store::Diff::Theme(Diff::JsonPresent(p)))) => seen.present.push(p),
                _ => {}
            })
            .unwrap();
        let (inputs_tx, inputs_rx) = async_channel::unbounded();
        let (requests_tx, requests_rx) = async_channel::unbounded();
        let _runtime = Runtime::start(Publisher::new(sender), move |ctx| {
            ctx.spawn(run(ctx.clone(), env, inputs_rx, requests_tx));
        });
        std::thread::spawn(move || {
            for (delay, i) in sends {
                std::thread::sleep(delay);
                inputs_tx.send_blocking(i).unwrap();
            }
            std::thread::sleep(Duration::from_secs(30));
        });
        let mut seen = Seen::default();
        let mut requests = Vec::new();
        let deadline = Instant::now() + Duration::from_secs(10);
        while Instant::now() < deadline {
            event_loop.dispatch(Some(Duration::from_millis(20)), &mut seen).unwrap();
            while let Ok(r) = requests_rx.try_recv() {
                requests.push(r);
            }
            if done(&seen, &requests) {
                break;
            }
        }
        (seen, requests)
    }

    fn log_lines(log: &Path, tool: &str) -> Vec<String> {
        std::fs::read_to_string(log)
            .unwrap_or_default()
            .lines()
            .filter(|l| l.starts_with(tool))
            .map(str::to_owned)
            .collect()
    }

    // --- The pure halves

    #[test]
    fn mode_key_reads_three_values_and_nothing_else() {
        assert_eq!(mode_key(&json!({ "theme": { "mode": "auto" } })), "auto");
        assert_eq!(mode_key(&json!({ "theme": { "mode": "dark" } })), "dark");
        assert_eq!(mode_key(&json!({ "theme": { "mode": "dusk" } })), "");
        assert_eq!(mode_key(&json!({})), "");
    }

    #[test]
    fn request_mode_answers_the_ipc_strings() {
        assert_eq!(request_mode("toggle", "dark", ""), Ok("light".into()));
        assert_eq!(request_mode("toggle", "light", "auto"), Ok("dark".into()));
        assert_eq!(request_mode("dusk", "dark", ""), Err("error: mode must be dark, light, or toggle".into()));
        assert_eq!(request_mode("light", "dark", "dark"), Err("error: theme.mode pins the mode to dark".into()));
    }

    #[test]
    fn schedule_status_is_the_auto_keys_alone() {
        assert_eq!(schedule_status("dark", None), None);
        let fallback = schedule_status("auto", None).unwrap();
        assert_eq!((fallback.source, fallback.polar, fallback.sunrise.as_str()), ("fallback", "", ""));
        let times = sun::sun_times(&Local::now(), 52.0, 4.9, None).unwrap();
        let located = schedule_status("auto", Some(&times)).unwrap();
        assert_eq!(located.source, "location");
        assert_eq!(located.sunrise, sun::hhmm(times.sunrise_minutes));
    }

    #[test]
    fn a_settings_or_palette_diff_resolves_the_theme_again() {
        let mut state = State::default();
        assert_eq!(state.theme.preset, "metamorphosis");
        assert!(state.apply(Diff::Settings(json!({ "theme": { "preset": "pantheon" } }))));
        assert_eq!(state.theme.preset, "pantheon");
        assert!(!state.apply(Diff::Settings(json!({ "theme": { "preset": "pantheon" } }))));
        assert!(state.apply(Diff::Palette(palette::fallback("light"))));
        assert_eq!(state.theme.colors.mode, "light");
    }

    #[test]
    fn drop_ins_join_in_glob_order() {
        let root = scratch("dropins");
        let d = root.join("matugen.d");
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("b.toml"), "[templates.b]").unwrap();
        std::fs::write(d.join("a.toml"), "[templates.a]").unwrap();
        std::fs::write(d.join("c.txt"), "ignored").unwrap();
        let (user, joined) = read_user_configs(&root.join("absent.toml"), &d);
        assert_eq!(user, None);
        assert_eq!(joined.as_deref(), Some("[templates.a]\n[templates.b]"));
    }

    // --- The engine, against shims

    /// No wallpaper: no matugen at all, the zinc variant for the mode
    /// written as theme.json, the Hyprland colours beside it, the chrome
    /// file, the system scheme asserted, one reload per publish.
    #[test]
    fn no_wallpaper_publishes_the_fallback_for_the_mode() {
        let (env, log) = rig("static");
        let theme_json = env.theme_json();
        // The palette is published before the Hyprland colours are written,
        // and the chrome writes on its own task, so the files are waited on
        // too. The dconf pair and the reload are children that outlive the
        // publish, and dropping the runtime drops them, so the run is held
        // until they have logged.
        let (hypr_colors, hypr_chrome) = (env.hypr_colors(), env.hypr_chrome());
        let mango_colors = env.mango_colors();
        let (seen, _) = drive(env.clone(), vec![(Duration::ZERO, inputs(json!({}), "", "light"))], |s, _| {
            s.present.contains(&true)
                && !s.palettes.is_empty()
                && hypr_colors.exists()
                && hypr_chrome.exists()
                && std::fs::read_to_string(&mango_colors).is_ok_and(|t| t.contains("focus_color=0x18181bff"))
                && log_lines(&log, "dconf").len() >= 2
                && !log_lines(&log, "hyprctl reload").is_empty()
        });
        let light = palette::fallback("light");
        assert_eq!(std::fs::read_to_string(&theme_json).unwrap(), serde_json::to_string_pretty(&light).unwrap());
        assert_eq!(seen.palettes.last().unwrap()["mode"], "light");
        assert_eq!(std::fs::read_to_string(env.hypr_colors()).unwrap(), matugen::hyprland_colors(&light));
        let chrome = std::fs::read_to_string(env.hypr_chrome()).unwrap();
        assert!(chrome.contains("rounding = 10,"));
        assert!(std::fs::read_to_string(env.mango_chrome()).unwrap().contains("\nborder_radius=10\n"));
        assert!(log_lines(&log, "matugen").is_empty());
        let dconf = log_lines(&log, "dconf");
        assert!(dconf.contains(&"dconf write /org/gnome/desktop/interface/color-scheme 'prefer-light'".to_owned()));
        assert!(dconf.contains(&"dconf write /org/gnome/desktop/interface/gtk-theme 'adw-gtk3'".to_owned()));
        assert!(!log_lines(&log, "hyprctl reload").is_empty());
    }

    /// A theme.json left from the last login skips the retheme, but the
    /// system scheme is still asserted over whatever another session wrote.
    #[test]
    fn present_theme_json_still_asserts_the_system_scheme() {
        let (env, log) = rig("present");
        std::fs::create_dir_all(&env.state_dir).unwrap();
        std::fs::write(env.theme_json(), r#"{"mode":"dark"}"#).unwrap();
        drive(env, vec![(Duration::ZERO, inputs(json!({}), "/wall.png", "dark"))], |_, _| {
            log_lines(&log, "dconf").len() >= 2
        });
        assert!(log_lines(&log, "matugen").is_empty());
        let dconf = log_lines(&log, "dconf");
        assert!(dconf.contains(&"dconf write /org/gnome/desktop/interface/color-scheme 'prefer-dark'".to_owned()));
        assert!(dconf.contains(&"dconf write /org/gnome/desktop/interface/gtk-theme 'adw-gtk3-dark'".to_owned()));
    }

    /// A wallpaper: the probe reads rank 0 off stderr and the real run pins
    /// the source to it, then both outputs are renamed into place.
    #[test]
    fn a_wallpaper_pins_matugen_to_its_rank_zero() {
        let (env, log) = rig("matugen");
        let theme_json = env.theme_json();
        let (seen, _) = drive(env.clone(), vec![(Duration::ZERO, inputs(json!({}), "/w/forest.png", "dark"))], |s, _| {
            s.palettes.iter().any(|p| p["primary"] == "#648db8")
        });
        assert!(seen.palettes.iter().any(|p| p["primary"] == "#648db8"));
        assert_eq!(std::fs::read_to_string(theme_json).unwrap(), r##"{"mode":"dark","primary":"#648db8"}"##);
        assert_eq!(std::fs::read_to_string(env.hypr_colors()).unwrap(), "return {}");
        let merged = env.merged_config().to_string_lossy().into_owned();
        let runs = log_lines(&log, "matugen");
        assert_eq!(runs[0], "matugen -d image /w/forest.png --dry-run --prefer saturation");
        assert_eq!(
            runs[1],
            format!("matugen image /w/forest.png -m dark -c {merged} --prefer closest-to-fallback --fallback-color #648db8")
        );
        let cfg = std::fs::read_to_string(env.merged_config()).unwrap();
        assert!(cfg.starts_with("[config]\n"));
        assert!(cfg.contains("theme.json.tmpl"));
    }

    /// A flat desktop: one `color hex` run on its colour, no probe, and the
    /// scheme it renders published like an image's.
    #[test]
    fn a_flat_desktop_seeds_matugen_with_its_colour() {
        let (env, log) = rig("flat");
        let (seen, _) = drive(env.clone(), vec![(Duration::ZERO, inputs(json!({}), "color:#402484", "dark"))], |s, _| {
            s.palettes.iter().any(|p| p["primary"] == "#648db8")
        });
        assert!(seen.palettes.iter().any(|p| p["primary"] == "#648db8"));
        let merged = env.merged_config().to_string_lossy().into_owned();
        assert_eq!(log_lines(&log, "matugen"), [format!("matugen color hex 402484 -m dark -c {merged}")]);
        assert!(!env.theme_json_tmp().exists());
    }

    /// A Flexoki wallpaper: `color hex` on the palette's source, every
    /// template staged rewritten, and theme.json the palette's own view.
    #[test]
    fn a_pinned_wallpaper_renders_the_palettes_own_tones() {
        let (env, log) = rig("pinned");
        let theme_json = env.theme_json();
        let pin = palette::pinned_palette("/w/flexoki-dusk.png").unwrap();
        let want = pin.shadcn("dark");
        let target = want.clone();
        let (seen, _) = drive(env.clone(), vec![(Duration::ZERO, inputs(json!({}), "/w/flexoki-dusk.png", "dark"))], move |s, _| {
            s.palettes.last().is_some_and(|p| p["primary"] == target["primary"])
        });
        assert!(!seen.palettes.is_empty());
        assert_eq!(std::fs::read_to_string(theme_json).unwrap(), serde_json::to_string_pretty(&want).unwrap());
        let merged = env.merged_config().to_string_lossy().into_owned();
        let runs = log_lines(&log, "matugen");
        assert_eq!(runs, [format!("matugen color hex {} -m dark -c {merged}", pin.source)]);
        let cfg = std::fs::read_to_string(env.merged_config()).unwrap();
        assert!(cfg.contains(&env.pinned_dir().join("0.tmpl").to_string_lossy().into_owned()));
        let staged = std::fs::read_to_string(env.pinned_dir().join("0.tmpl")).unwrap();
        assert!(!staged.contains("{{colors.surface.default.hex}}"));
        assert!(!env.theme_json_tmp().exists());
    }

    /// Two wallpaper flips while a run is in flight queue one rerun, never
    /// a second overlapping run.
    #[test]
    fn retheme_queues_behind_the_run_in_flight() {
        let (env, log) = rig("queue");
        let probe_log = log.clone();
        let sends = vec![
            (Duration::ZERO, inputs(json!({}), "/w/one.png", "dark")),
            (Duration::from_millis(100), inputs(json!({}), "/w/two.png", "dark")),
            (Duration::from_millis(50), inputs(json!({}), "/w/three.png", "dark")),
        ];
        drive(env, sends, move |_, _| {
            log_lines(&probe_log, "matugen image").iter().any(|l| l.contains("three.png"))
        });
        std::thread::sleep(Duration::from_millis(800));
        let runs = log_lines(&log, "matugen image");
        assert_eq!(runs.len(), 2, "{runs:?}");
        assert!(runs[0].contains("one.png"));
        assert!(runs[1].contains("three.png"));
    }

    /// A pinned key takes the mode back the moment it disagrees.
    #[test]
    fn a_pinned_mode_key_asks_for_its_mode() {
        let (env, _) = rig("pinkey");
        let settings = json!({ "theme": { "mode": "dark" } });
        let (_, requests) =
            drive(env, vec![(Duration::ZERO, inputs(settings, "", "light"))], |_, r| !r.is_empty());
        assert_eq!(requests.first(), Some(&Request::SetMode("dark".into())));
    }

    /// Under "auto", a write that disagrees with the schedule is a snooze
    /// until the next change rather than something to revert.
    #[test]
    fn a_manual_flip_under_auto_snoozes_one_cycle() {
        let (env, _) = rig("snooze");
        let settings = json!({ "theme": { "mode": "auto" } });
        let scheduled = sun::effective_mode("auto", &Local::now(), None, None).unwrap();
        let flipped = if scheduled == "dark" { "light" } else { "dark" };
        let sends = vec![
            (Duration::ZERO, inputs(settings.clone(), "", scheduled)),
            (Duration::from_millis(300), inputs(settings, "", flipped)),
        ];
        let (_, requests) = drive(env, sends, |_, r| r.iter().any(|q| matches!(q, Request::SetModeOverride(Some(_)))));
        let snooze = requests.iter().find_map(|q| match q {
            Request::SetModeOverride(Some(o)) => Some(o.clone()),
            _ => None,
        });
        let snooze = snooze.expect("a snooze was asked for");
        assert_eq!(snooze.mode, flipped);
        assert_eq!(snooze.until_ms, sun::next_change(&Local::now(), None).timestamp_millis());
    }
}
