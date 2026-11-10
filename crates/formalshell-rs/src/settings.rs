//! settings.json's keys, typed, each with its default beside it. A reader
//! asks for a key here rather than walking the document with a default of
//! its own, and the settings UI's catalogue (`services/overrides.rs`) takes
//! its defaults from the same constants. A feature model that validates a
//! whole subtree of its own (the bar's layout, the hot corners, the theme)
//! keeps doing so; this module only names, types and defaults keys.

use std::path::PathBuf;

use fs_chrome::hot_corners::corners::{self, Corner};
use fs_menu::actions::Run;
use fs_system::power::model::Thresholds;
use fs_types::Edge;
use serde_json::Value;

use crate::services::config::{self, lookup};

/// A settings document a key is read off: the resolved one on the store,
/// or a copy a service thread holds.
pub trait Doc {
    fn doc(&self) -> &Value;
}

impl Doc for Value {
    fn doc(&self) -> &Value {
        self
    }
}

impl Doc for config::State {
    fn doc(&self) -> &Value {
        self.settings()
    }
}

/// A boolean key: anything but `true` or `false` reads as the default.
#[derive(Clone, Copy, Debug)]
pub struct Flag {
    pub key: &'static str,
    pub default: bool,
}

impl Flag {
    const fn new(key: &'static str, default: bool) -> Self {
        Self { key, default }
    }

    pub fn get(self, doc: &(impl Doc + ?Sized)) -> bool {
        lookup(doc.doc(), self.key).and_then(Value::as_bool).unwrap_or(self.default)
    }
}

/// A string key: anything but a string reads as the default.
#[derive(Clone, Copy, Debug)]
pub struct Word {
    pub key: &'static str,
    pub default: &'static str,
}

impl Word {
    const fn new(key: &'static str, default: &'static str) -> Self {
        Self { key, default }
    }

    pub fn get<D: Doc + ?Sized>(self, doc: &D) -> &str {
        lookup(doc.doc(), self.key).and_then(Value::as_str).unwrap_or(self.default)
    }
}

/// A number key: anything but a number reads as the default.
#[derive(Clone, Copy, Debug)]
pub struct Number {
    pub key: &'static str,
    pub default: f64,
}

impl Number {
    const fn new(key: &'static str, default: f64) -> Self {
        Self { key, default }
    }

    pub fn get(self, doc: &(impl Doc + ?Sized)) -> f64 {
        lookup(doc.doc(), self.key).and_then(Value::as_f64).unwrap_or(self.default)
    }

    /// The key when it is above zero, else the default.
    pub fn positive(self, doc: &(impl Doc + ?Sized)) -> f64 {
        lookup(doc.doc(), self.key).and_then(Value::as_f64).filter(|n| *n > 0.0).unwrap_or(self.default)
    }

    /// Rounded and held to `lo..=hi`, the way LyricsService clamps.
    fn rounded(self, doc: &(impl Doc + ?Sized), lo: f64, hi: f64) -> f64 {
        self.get(doc).round().clamp(lo, hi)
    }
}

// Appearance

/// Unset leaves the mode to state.json, whose own default is dark; the
/// settings UI shows that as the row's default. `services::theme::mode_key`
/// reads it.
pub const THEME_MODE: Word = Word::new("theme.mode", "dark");
pub const REACTIVE_RIMS: Flag = Flag::new("theme.reactiveRims", true);
pub const MOTION: Flag = Flag::new("motion.enabled", true);
pub const MORPH: Flag = Flag::new("motion.morph", true);
pub const WALLPAPER_ANIMATE: Flag = Flag::new("wallpaper.animate", false);
pub const LAYOUT: Word = Word::new("layout", "bar");
pub const BAR_POSITION: Word = Word::new("bar.position", "top");
pub const BAR_STYLE: Word = Word::new("bar.style", "transparent");
pub const NOTIFICATIONS_POSITION: Word = Word::new("notifications.position", "bottom-right");
pub const HIDE_CHROME: Flag = Flag::new("fullscreen.hideChrome", true);
pub const GTK_THEME: Word = Word::new("gtk.theme", "");
pub const GTK_THEME_DARK: Word = Word::new("gtk.themeDark", "");
pub const RENDER_BACKEND: Word = Word::new("render.backend", "auto");

/// `layout`: where the shell's chrome sits, a strip or the sidebar's card
/// column on a side edge.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Layout {
    Bar,
    Sidebar,
}

impl Layout {
    /// An unknown value reads as the default.
    pub fn parse(value: Option<&Value>) -> Self {
        match value.and_then(Value::as_str) {
            Some("sidebar") => Self::Sidebar,
            _ => Self::Bar,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Bar => "bar",
            Self::Sidebar => "sidebar",
        }
    }
}

/// `render.backend` as asked. Read once at start: the shell resolves it
/// to the backend drawing (`formalshell-rs`'s `gpu.rs`) before any surface.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Backend {
    Auto,
    Gpu,
    Cpu,
}

impl Backend {
    /// An unknown value reads as the default.
    pub fn parse(value: Option<&Value>) -> Self {
        match value.and_then(Value::as_str) {
            Some("gpu") => Self::Gpu,
            Some("cpu") => Self::Cpu,
            _ => Self::Auto,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Gpu => "gpu",
            Self::Cpu => "cpu",
        }
    }
}

/// `layout`: an unknown value reads as the default.
pub fn layout(doc: &(impl Doc + ?Sized)) -> Layout {
    Layout::parse(lookup(doc.doc(), LAYOUT.key))
}

/// `bar.position`: anything but one of the four edges reads as the
/// default, so a typo leaves the bar where it always was.
pub fn bar_position(doc: &(impl Doc + ?Sized)) -> Edge {
    Edge::parse(BAR_POSITION.get(doc)).unwrap_or(Edge::Top)
}

/// `render.backend`, picked before the first surface, so a new value takes
/// effect at the next start.
pub fn render_backend(doc: &(impl Doc + ?Sized)) -> Backend {
    Backend::parse(lookup(doc.doc(), RENDER_BACKEND.key))
}

/// `wallpaper.color`: a named colour, a hex or the sky, none for anything
/// else.
pub fn wallpaper_color(doc: &(impl Doc + ?Sized)) -> Option<String> {
    lookup(doc.doc(), "wallpaper.color").and_then(fs_theme::theme::wallpaper_color).flatten()
}

/// `location.latitude/longitude`, both or neither.
pub fn location(doc: &(impl Doc + ?Sized)) -> Option<(f64, f64)> {
    let num = |key| lookup(doc.doc(), key).and_then(Value::as_f64);
    num("location.latitude").zip(num("location.longitude"))
}

/// `avatar.path`, the picture the lock screen and the polkit card show;
/// `~/.face` when unset or empty.
pub fn avatar_path(doc: &(impl Doc + ?Sized), home: &str) -> String {
    match lookup(doc.doc(), "avatar.path").and_then(Value::as_str) {
        Some(p) if !p.is_empty() => p.to_owned(),
        _ => format!("{home}/.face"),
    }
}

/// `battery.warnPercent` and `battery.criticalPercent` over the laptop's
/// lines.
pub fn battery_thresholds(doc: &(impl Doc + ?Sized)) -> Thresholds {
    let num = |key| lookup(doc.doc(), key).and_then(Value::as_f64);
    Thresholds::LAPTOP.with(num("battery.warnPercent"), num("battery.criticalPercent"))
}

// Desktop

pub const CLOCK_ENABLED: Flag = Flag::new("desktopClock.enabled", false);
pub const CLOCK_FACE: Word = Word::new("desktopClock.face", "pixel");
pub const CLOCK_WIDTH: Word = Word::new("desktopClock.width", "wide");
pub const CLOCK_HOUR12: Flag = Flag::new("desktopClock.hour12", false);
/// Read by `services::wallpaper::DepthCut` under each of its prefixes.
pub const CLOCK_DEPTH: Flag = Flag::new("desktopClock.depth", true);
pub const DEPTH_GENERATE: Flag = Flag::new("wallpaper.depthGenerate", false);

pub const HOT_CORNERS_ENABLED: Flag = Flag::new("hotCorners.enabled", corners::DEFAULT_ENABLED);
pub const HOT_CORNER_TOP_LEFT: Word = corner("hotCorners.topLeft", Corner::TopLeft);
pub const HOT_CORNER_TOP_RIGHT: Word = corner("hotCorners.topRight", Corner::TopRight);
pub const HOT_CORNER_BOTTOM_LEFT: Word = corner("hotCorners.bottomLeft", Corner::BottomLeft);
pub const HOT_CORNER_BOTTOM_RIGHT: Word = corner("hotCorners.bottomRight", Corner::BottomRight);

const fn corner(key: &'static str, corner: Corner) -> Word {
    Word::new(key, corners::default_action(corner))
}

/// `hotCorners`, a corner's text that names no built-in read the way the
/// launcher reads an action: an `@ipc:` call checked against what answers
/// it, or a command line. A bare word is neither and stays a typo rather
/// than becoming a command: "lok" would otherwise spawn a shell that fails
/// silently, on a surface the pointer reaches by accident. A single binary
/// gets in as an absolute path or with its arguments.
pub fn hot_corners(settings: &impl Doc) -> corners::Config<Run> {
    corners::resolve(lookup(settings.doc(), "hotCorners"), |text| match Run::parse(text) {
        Run::Ipc(call) => crate::ipc::check(&call, false).map(Run::Ipc),
        Run::Shell(line) if line.contains(char::is_whitespace) || line.contains('/') => Ok(Run::Shell(line)),
        _ => Err(format!("unknown action {}", Value::String(text.to_owned()))),
    })
}

// Windows

pub const SWITCHER: Flag = Flag::new("switcher.enabled", true);
pub const SWITCHER_CURRENT_WORKSPACE: Flag = Flag::new("switcher.currentWorkspace", false);
pub const WORKSPACE_PREVIEW: Flag = Flag::new("workspaces.preview", true);

pub const CONSOLE_APP_ID: Word = Word::new("console.appId", "dev.formalshell.console");
pub const CONSOLE_SHARE: Number = Number::new("console.share", 0.5);

/// `console.command` as written: a list (a non-string item as its JSON
/// text), a lone non-empty string as a one-word list, any other value an
/// empty one, and none while the key is absent.
pub fn console_command_set(doc: &(impl Doc + ?Sized)) -> Option<Vec<String>> {
    Some(match lookup(doc.doc(), "console.command")? {
        Value::Array(items) => items.iter().map(|v| v.as_str().map_or_else(|| v.to_string(), str::to_owned)).collect(),
        Value::String(s) if !s.is_empty() => vec![s.clone()],
        _ => Vec::new(),
    })
}

/// The quake console's argv: [`console_command_set`], else ghostty under
/// the console's default app id.
pub fn console_command(doc: &(impl Doc + ?Sized)) -> Vec<String> {
    console_command_set(doc).unwrap_or_else(|| vec!["ghostty".into(), format!("--class={}", CONSOLE_APP_ID.default)])
}

// Launcher

pub const APP_GRID: Flag = Flag::new("menu.appGrid", false);
/// Empty is the picker's own default directory.
pub const PICKER_DIRECTORY: Word = Word::new("picker.directory", "");
pub const CLIPBOARD_PASTE: Flag = Flag::new("clipboard.paste", true);
pub const PASTE_CHORD: Word = Word::new("clipboard.pasteChord", "ctrl+v");
pub const CLIPSSH_ALIAS: Word = Word::new("clipssh.alias", "");
pub const CLIPSSH_AUTO_SEND: Flag = Flag::new("clipssh.autoSendImages", false);
pub const REMINDER_MESSAGE: Word = Word::new("reminders.defaultMessage", "Time's up");
pub const LOCALSEND_RECEIVE: Flag = Flag::new("localsend.receive", false);
pub const LOCALSEND_ALIAS: Word = Word::new("localsend.alias", "");
pub const LOCALSEND_DIR: Word = Word::new("localsend.dir", "");

// Media

pub const LYRICS: Flag = Flag::new("media.lyrics", true);
pub const LYRICS_BLUR: Flag = Flag::new("media.lyricsBlur", true);
pub const LYRICS_OFFSET_AUTO: Flag = Flag::new("media.lyricsOffsetAuto", true);
const LYRICS_OFFSET_MS: Number = Number::new("media.lyricsOffsetMs", 0.0);
const LYRICS_BLUR_STRENGTH: Number = Number::new("media.lyricsBlurStrength", 100.0);
pub const ANIMATED_BAR_COVER: Flag = Flag::new("media.animatedBarCover", true);
pub const APPLE_MUSIC_ART: Flag = Flag::new("media.appleMusicArt", false);
pub const VISUALIZER: Flag = Flag::new("media.visualizer", true);
/// Also the style drawn when the configured one is unknown.
pub const VISUALIZER_STYLE: Word = Word::new("media.visualizerStyle", "bars");
pub const AIRPLAY_ENABLE: Flag = Flag::new("airplay.enable", false);
pub const AIRPLAY_NAME: Word = Word::new("airplay.name", "");
pub const IPHONE_ENABLE: Flag = Flag::new("iphone.enable", true);

/// `media.lyricsOffsetMs`, -5000 to 5000.
pub fn lyrics_offset_ms(doc: &(impl Doc + ?Sized)) -> f64 {
    LYRICS_OFFSET_MS.rounded(doc, -5000.0, 5000.0)
}

/// `media.lyricsBlurStrength`, 0 to 200.
pub fn lyrics_blur_strength(doc: &(impl Doc + ?Sized)) -> f64 {
    LYRICS_BLUR_STRENGTH.rounded(doc, 0.0, 200.0)
}

// Screen

pub const GUARD_MEDIA: Flag = Flag::new("screensaver.guardMediaPlayback", true);
pub const LOCK_BEFORE_SLEEP: Flag = Flag::new("lock.beforeSleep", true);
pub const SAVER_EFFECT: Word = Word::new("screensaver.effect", "random");
pub const SAVER_HOLD_SECONDS: Number = Number::new("screensaver.holdSeconds", 4.0);
pub const SAVER_ASCII_PATH: Word = Word::new("screensaver.asciiPath", "");
pub const SAVER_LOCK_AFTER_SECONDS: Number = Number::new("screensaver.lockAfterSeconds", 0.0);
pub const SAVER_FRAME_RATE: Number = Number::new("screensaver.frameRate", 120.0);
/// Idle time before the screensaver, read with [`Number::positive`].
pub const SAVER_TIMEOUT_SECONDS: Number = Number::new("screensaver.timeoutSeconds", 300.0);
pub const CAFFEINATE_ON_STARTUP: Flag = Flag::new("caffeinate.onStartup", false);

pub const NIGHTLIGHT_SCHEDULE: Word = Word::new("nightlight.schedule", "sun");
pub const NIGHTLIGHT_START_ON: Flag = Flag::new("nightlight.startOn", false);
pub const NIGHTLIGHT_TEMP: Number = Number::new("nightlight.temp", 4000.0);

/// `nightlight.temp` in kelvin, truncated.
pub fn nightlight_temp(doc: &(impl Doc + ?Sized)) -> i64 {
    NIGHTLIGHT_TEMP.get(doc) as i64
}

// Capture

pub const SCREENSHOT_TIMEOUT_SECONDS: Number = Number::new("screenshot.timeoutSeconds", 90.0);
pub const CAPTURE_TIMEOUT_SECONDS: Number = Number::new("capture.timeoutSeconds", 90.0);
pub const SCREENSHOT_EDITOR: Word = Word::new("screenshot.editor", "tensaku-edit");
pub const OCR_LANGUAGE: Word = Word::new("capture.ocrLanguage", "eng");
pub const RECORDING_TIMEOUT_SECONDS: Number = Number::new("recording.timeoutSeconds", 90.0);
pub const RECORDING_AUDIO: Word = Word::new("recording.audio", "none");
pub const RECORDING_PLAYER: Word = Word::new("recording.player", "xdg-open");
/// Empty picks the codec off what the encoder offers.
pub const RECORDING_CODEC: Word = Word::new("recording.codec", "");
pub const RECORDING_AUDIO_BACKEND: Word = Word::new("recording.audioBackend", "");
/// Zero is the output's own height.
pub const RECORDING_MAX_HEIGHT: Number = Number::new("recording.maxHeight", 0.0);
pub const RECORDING_FRAMERATE: Number = Number::new("recording.framerate", 30.0);
pub const RECORDING_NO_DMABUF: Flag = Flag::new("recording.noDmabuf", false);
pub const RECORDING_FINALIZE: Flag = Flag::new("recording.finalize", true);
pub const GIF_FPS: Number = Number::new("recording.gifFps", 12.0);
pub const GIF_WIDTH: Number = Number::new("recording.gifWidth", 640.0);
pub const WEBCAM: Flag = Flag::new("recording.webcam", false);
pub const WEBCAM_SIZE: Word = Word::new("recording.webcamSize", "medium");
pub const WEBCAM_DEVICE: Word = Word::new("recording.webcamDevice", "");

/// `screenshot.directory`, else `~/Pictures/Screenshots`.
pub fn screenshot_dir(doc: &(impl Doc + ?Sized)) -> String {
    directory(doc, "screenshot.directory", "Pictures/Screenshots")
}

/// `recording.directory`, else `~/Videos`.
pub fn recording_dir(doc: &(impl Doc + ?Sized)) -> String {
    directory(doc, "recording.directory", "Videos")
}

fn directory(doc: &(impl Doc + ?Sized), key: &str, under_home: &str) -> String {
    match lookup(doc.doc(), key).and_then(Value::as_str) {
        Some(d) if !d.is_empty() => d.to_owned(),
        _ => format!("{}/{under_home}", std::env::var("HOME").unwrap_or_default()),
    }
}

// The greeter's own keys.

/// `greeter.sessionCommand`: a list of non-empty strings, else `["Hyprland"]`.
pub fn greeter_session_command(doc: &(impl Doc + ?Sized)) -> Vec<String> {
    let argv: Option<Vec<String>> = lookup(doc.doc(), "greeter.sessionCommand")
        .and_then(Value::as_array)
        .and_then(|items| items.iter().map(|v| v.as_str().filter(|s| !s.is_empty()).map(str::to_owned)).collect());
    argv.filter(|a| !a.is_empty()).unwrap_or_else(|| vec!["Hyprland".into()])
}

/// `greeter.sessionDirs`, else `default`.
pub fn greeter_session_dirs(doc: &(impl Doc + ?Sized), default: &[&str]) -> Vec<PathBuf> {
    let dirs: Option<Vec<PathBuf>> =
        lookup(doc.doc(), "greeter.sessionDirs").and_then(Value::as_array).map(|items| items.iter().filter_map(Value::as_str).map(PathBuf::from).collect());
    dirs.filter(|d| !d.is_empty()).unwrap_or_else(|| default.iter().map(PathBuf::from).collect())
}

/// `greeter.wallpaper`, none when unset or empty.
pub fn greeter_wallpaper(doc: &(impl Doc + ?Sized)) -> Option<String> {
    lookup(doc.doc(), "greeter.wallpaper").and_then(Value::as_str).filter(|s| !s.is_empty()).map(str::to_owned)
}

#[cfg(test)]
mod tests {
        use serde_json::json;

    use super::*;

    #[test]
    fn a_malformed_value_reads_as_the_default() {
        let doc = json!({"media": {"lyrics": "no", "visualizerStyle": 3}, "console": {"share": "half"}});
        assert!(LYRICS.get(&doc));
        assert_eq!(VISUALIZER_STYLE.get(&doc), "bars");
        assert_eq!(CONSOLE_SHARE.get(&doc), 0.5);
        let set = json!({"media": {"lyrics": false, "visualizerStyle": "led"}, "console": {"share": 0.3}});
        assert!(!LYRICS.get(&set));
        assert_eq!(VISUALIZER_STYLE.get(&set), "led");
        assert_eq!(CONSOLE_SHARE.get(&set), 0.3);
    }

    #[test]
    fn an_unknown_position_is_the_top() {
        let at = |v: Value| bar_position(&json!({"bar": {"position": v}}));
        assert_eq!(bar_position(&json!({})), Edge::Top);
        assert_eq!(at(json!("middle")), Edge::Top);
        assert_eq!(at(json!("bottom")), Edge::Bottom);
        assert_eq!(at(json!("left")), Edge::Left);
        assert_eq!(at(json!("right")), Edge::Right);
    }

    #[test]
    fn layout_and_backend_defaults_are_their_first_variants() {
        assert_eq!(LAYOUT.default, Layout::Bar.as_str());
        assert_eq!(RENDER_BACKEND.default, Backend::Auto.as_str());
        assert_eq!(layout(&json!({"layout": "sidebar"})), Layout::Sidebar);
        assert_eq!(layout(&json!({"layout": "dock"})), Layout::Bar);
        assert_eq!(render_backend(&json!({"render": {"backend": "gpu"}})), Backend::Gpu);
        assert_eq!(render_backend(&json!({"render": {"backend": "vulkan"}})), Backend::Auto);
    }

    #[test]
    fn the_feature_models_read_the_same_defaults() {
        let toasts = fs_info::notifications::position_spec(None).name;
        assert_eq!(fs_info::notifications::position_spec(Some(NOTIFICATIONS_POSITION.default)).name, toasts);
        assert_eq!(crate::services::theme::mode_key(&json!({})), "");
        assert!(crate::ui::spectrum::styles::is_known(VISUALIZER_STYLE.default));
        assert_eq!(battery_thresholds(&json!({})), Thresholds::LAPTOP);
    }

    #[test]
    fn console_command_defaults_to_ghostty_only_when_absent() {
        assert_eq!(console_command(&json!({})), ["ghostty", "--class=dev.formalshell.console"]);
        assert_eq!(console_command_set(&json!({})), None);
        assert_eq!(console_command(&json!({"console": {"command": ["foot", 1]}})), ["foot", "1"]);
        assert_eq!(console_command(&json!({"console": {"command": "foot"}})), ["foot"]);
        assert!(console_command(&json!({"console": {"command": ""}})).is_empty());
        assert!(console_command(&json!({"console": {"command": null}})).is_empty());
    }

    #[test]
    fn session_command_falls_back_to_hyprland() {
        assert_eq!(greeter_session_command(&json!({})), ["Hyprland"]);
        assert_eq!(greeter_session_command(&json!({"greeter": {"sessionCommand": ["sway", "-d"]}})), ["sway", "-d"]);
        assert_eq!(greeter_session_command(&json!({"greeter": {"sessionCommand": ["sway", ""]}})), ["Hyprland"]);
        assert_eq!(greeter_session_command(&json!({"greeter": {"sessionCommand": []}})), ["Hyprland"]);
    }

    #[test]
    fn avatar_defaults_to_home_face() {
        assert_eq!(avatar_path(&json!({}), "/home/k"), "/home/k/.face");
        assert_eq!(avatar_path(&json!({"avatar": {"path": ""}}), "/home/k"), "/home/k/.face");
        assert_eq!(avatar_path(&json!({"avatar": {"path": "/p.png"}}), "/home/k"), "/p.png");
    }

    #[test]
    fn a_hot_corner_naming_nothing_is_refused_with_its_reason() {
        use fs_menu::actions::IpcCall;
        let config = hot_corners(&json!({"hotCorners": {
            "topLeft": "@ipc:nope.verb",
            "topRight": "lok",
            "bottomLeft": "@ipc:notifications.showHistory",
            "bottomRight": "foot -e htop",
        }}));
        assert_eq!(config.warnings, [
            "hotCorners.topLeft: no IPC target \"nope\", leaving the corner inert",
            "hotCorners.topRight: unknown action \"lok\", leaving the corner inert",
        ]);
        assert_eq!(config.corners.top_left, corners::Action::None);
        assert_eq!(config.corners.top_right, corners::Action::None);
        let launcher = |run: Run| corners::Action::Launcher { text: run.to_string(), run };
        assert_eq!(config.corners.bottom_left, launcher(Run::Ipc(IpcCall::new("notifications", "showHistory"))));
        assert_eq!(config.corners.bottom_right, launcher(Run::Shell("foot -e htop".into())));
    }

    #[test]
    fn lyrics_numbers_round_and_clamp() {
        assert_eq!(lyrics_offset_ms(&json!({})), 0.0);
        assert_eq!(lyrics_offset_ms(&json!({"media": {"lyricsOffsetMs": 9000.4}})), 5000.0);
        assert_eq!(lyrics_blur_strength(&json!({"media": {"lyricsBlurStrength": 49.6}})), 50.0);
        assert_eq!(lyrics_blur_strength(&json!({"media": {"lyricsBlurStrength": "x"}})), 100.0);
    }
}
