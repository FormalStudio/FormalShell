//! Resolver for `hotCorners` (settings.json). Takes the raw object in, returns
//! the enabled flag, trigger size, dwell delay, per-corner actions and
//! warnings out. Bad input is never fatal: an unrecognised action name or a
//! non-numeric size is dropped with one warning string, since a typo in
//! settings.json must never take the whole shell down with it.

use std::sync::LazyLock;

use regex::Regex;
use serde_json::Value;

use fs_js as js;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Corner {
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

impl Corner {
    pub const ALL: [Corner; 4] = [
        Corner::TopLeft,
        Corner::TopRight,
        Corner::BottomLeft,
        Corner::BottomRight,
    ];

    /// The settings.json key.
    pub fn name(self) -> &'static str {
        match self {
            Corner::TopLeft => "topLeft",
            Corner::TopRight => "topRight",
            Corner::BottomLeft => "bottomLeft",
            Corner::BottomRight => "bottomRight",
        }
    }
}

/// What a corner does. The first three are the surface's own; anything else is
/// a launcher action string, handed verbatim to the same resolver the launcher
/// uses.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    None,
    Screensaver,
    Lock,
    /// The compositor's workspace overview (HyprExpo), the same command as
    /// `workspaces overview open`.
    Overview,
    Launcher(String),
}

impl Action {
    pub fn as_str(&self) -> &str {
        match self {
            Action::None => "none",
            Action::Screensaver => "screensaver",
            Action::Lock => "lock",
            Action::Overview => "overview",
            Action::Launcher(s) => s,
        }
    }

    fn builtin(name: &str) -> Option<Action> {
        match name {
            "none" => Some(Action::None),
            "screensaver" => Some(Action::Screensaver),
            "lock" => Some(Action::Lock),
            "overview" => Some(Action::Overview),
            _ => None,
        }
    }
}

static IPC_ACTION: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^@ipc:[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+(:[^\n\r\u{2028}\u{2029}]+)?$")
        .expect("static pattern")
});

/// A launcher action string is either the in-process form
/// (`@ipc:<target>.<fn>` with an optional `:<arg>`) or a command line the
/// compositor spawns.
///
/// A bare word is neither, and stays a typo rather than becoming a command:
/// "lok" would otherwise spawn a shell that fails silently, on a surface the
/// pointer reaches by accident. A single binary gets in as an absolute path or
/// with its arguments.
pub fn is_launcher_action(action: &str) -> bool {
    if action.is_empty() {
        return false;
    }
    if action.starts_with("@ipc:") {
        return IPC_ACTION.is_match(action);
    }
    action.chars().any(js::is_space) || action.contains('/')
}

/// The trigger square, in pixels. Small on purpose and still not hard to hit:
/// the compositor clamps the cursor at the screen edge, so throwing the
/// pointer at a corner parks it on the last pixel regardless of how fast it
/// was moving. The size only decides how much of the screen stops being
/// clickable, never how hard the corner is to reach.
pub const DEFAULT_SIZE: i64 = 4;
pub const MAX_SIZE: i64 = 64;

/// Dwell before the action fires, so a pointer merely passing through a
/// corner on its way somewhere else never locks the session.
pub const DEFAULT_DELAY_MS: i64 = 400;
pub const MAX_DELAY_MS: i64 = 10000;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Corners {
    pub top_left: Action,
    pub top_right: Action,
    pub bottom_left: Action,
    pub bottom_right: Action,
}

impl Corners {
    pub fn get(&self, corner: Corner) -> &Action {
        match corner {
            Corner::TopLeft => &self.top_left,
            Corner::TopRight => &self.top_right,
            Corner::BottomLeft => &self.bottom_left,
            Corner::BottomRight => &self.bottom_right,
        }
    }

    fn set(&mut self, corner: Corner, action: Action) {
        match corner {
            Corner::TopLeft => self.top_left = action,
            Corner::TopRight => self.top_right = action,
            Corner::BottomLeft => self.bottom_left = action,
            Corner::BottomRight => self.bottom_right = action,
        }
    }
}

/// A top corner's square comes out of the bar's own input region (the bar
/// anchors top/left/right): the overview's top-left one takes the leftmost
/// pixels of the first cell, and the top-right stays "none" so the
/// indicators keep theirs.
fn default_corner(corner: Corner) -> Action {
    match corner {
        Corner::TopLeft => Action::Overview,
        Corner::TopRight => Action::None,
        Corner::BottomLeft => Action::Screensaver,
        Corner::BottomRight => Action::Lock,
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Config {
    pub enabled: bool,
    pub size: i64,
    pub delay_ms: i64,
    pub corners: Corners,
    pub warnings: Vec<String>,
}

fn clamped_number(
    value: Option<&Value>,
    fallback: i64,
    min: i64,
    max: i64,
    key: &str,
    warnings: &mut Vec<String>,
) -> i64 {
    let Some(value) = value.filter(|v| !v.is_null()) else {
        return fallback;
    };
    let n = js::to_number(Some(value));
    if !n.is_finite() {
        warnings.push(format!(
            "hotCorners.{key}: expected a number, got {}",
            js::stringify(value)
        ));
        return fallback;
    }
    (js::round(n) as i64).clamp(min, max)
}

pub fn resolve(hot_corners: Option<&Value>) -> Config {
    let mut warnings = Vec::new();
    let empty = serde_json::Map::new();
    let raw = hot_corners.and_then(Value::as_object).unwrap_or(&empty);

    let mut corners = Corners {
        top_left: Action::None,
        top_right: Action::None,
        bottom_left: Action::None,
        bottom_right: Action::None,
    };
    for corner in Corner::ALL {
        let name = corner.name();
        let action = match raw.get(name).filter(|v| !v.is_null()) {
            None => default_corner(corner),
            Some(value) => match value.as_str().and_then(|s| {
                Action::builtin(s)
                    .or_else(|| is_launcher_action(s).then(|| Action::Launcher(s.into())))
            }) {
                Some(action) => action,
                None => {
                    warnings.push(format!(
                        "hotCorners.{name}: unknown action {}, leaving the corner inert",
                        js::stringify(value)
                    ));
                    Action::None
                }
            },
        };
        corners.set(corner, action);
    }

    Config {
        enabled: raw.get("enabled").is_none_or(|v| *v == Value::Bool(true)),
        size: clamped_number(
            raw.get("size"),
            DEFAULT_SIZE,
            1,
            MAX_SIZE,
            "size",
            &mut warnings,
        ),
        delay_ms: clamped_number(
            raw.get("delayMs"),
            DEFAULT_DELAY_MS,
            0,
            MAX_DELAY_MS,
            "delayMs",
            &mut warnings,
        ),
        corners,
        warnings,
    }
}

/// The corner's two live layer-shell anchors. The other two stay false, and
/// that is what makes the window size itself from its implicit size instead
/// of stretching across the output.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Edges {
    pub top: bool,
    pub bottom: bool,
    pub left: bool,
    pub right: bool,
}

pub fn edges(corner: Corner) -> Edges {
    Edges {
        top: matches!(corner, Corner::TopLeft | Corner::TopRight),
        bottom: matches!(corner, Corner::BottomLeft | Corner::BottomRight),
        left: matches!(corner, Corner::TopLeft | Corner::BottomLeft),
        right: matches!(corner, Corner::TopRight | Corner::BottomRight),
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CornerWindow {
    pub screen: String,
    pub corner: Corner,
    pub action: Action,
}

/// One entry per window the surface actually has to create, so a corner left
/// at "none" (or the whole feature switched off) costs no layer surface at
/// all rather than a mapped but inert one holding an input region over live
/// pixels.
pub fn windows(config: &Config, screens: &[String]) -> Vec<CornerWindow> {
    let mut out = Vec::new();
    if !config.enabled {
        return out;
    }
    for screen in screens {
        for corner in Corner::ALL {
            let action = config.corners.get(corner);
            if *action == Action::None {
                continue;
            }
            out.push(CornerWindow {
                screen: screen.clone(),
                corner,
                action: action.clone(),
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn run(v: Value) -> Config {
        resolve(Some(&v))
    }

    fn screens(names: &[&str]) -> Vec<String> {
        names.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn defaults_when_key_is_absent() {
        let c = resolve(None);
        assert!(c.enabled);
        assert_eq!(c.size, 4);
        assert_eq!(c.delay_ms, 400);
        assert_eq!(c.corners.top_left, Action::Overview);
        assert_eq!(c.corners.top_right, Action::None);
        assert_eq!(c.corners.bottom_left, Action::Screensaver);
        assert_eq!(c.corners.bottom_right, Action::Lock);
        assert_eq!(c.warnings.len(), 0);
    }

    #[test]
    fn partial_config_keeps_the_other_defaults() {
        let c = run(json!({ "topLeft": "lock" }));
        assert_eq!(c.corners.top_left, Action::Lock);
        assert_eq!(c.corners.bottom_left, Action::Screensaver);
        assert_eq!(c.corners.bottom_right, Action::Lock);
        assert_eq!(c.warnings.len(), 0);
    }

    #[test]
    fn explicit_none_disables_one_corner() {
        let c = run(json!({ "bottomRight": "none" }));
        assert_eq!(c.corners.bottom_right, Action::None);
        assert_eq!(c.corners.bottom_left, Action::Screensaver);
    }

    // The launcher action strings: the same two forms the launcher's own
    // action runner resolves, kept verbatim so the surface hands it the
    // string it was configured with.
    #[test]
    fn an_ipc_action_string_is_kept_verbatim() {
        let c = run(json!({ "topLeft": "@ipc:notifications.showHistory" }));
        assert_eq!(
            c.corners.top_left.as_str(),
            "@ipc:notifications.showHistory"
        );
        assert_eq!(c.warnings.len(), 0);
        assert!(is_launcher_action("@ipc:notifications.showHistory"));
        assert!(is_launcher_action("@ipc:clipboard.copy:3"));
    }

    #[test]
    fn a_command_line_is_kept_verbatim() {
        let c = run(json!({ "topRight": "hyprctl dispatch workspace 1" }));
        assert_eq!(c.corners.top_right.as_str(), "hyprctl dispatch workspace 1");
        assert_eq!(c.warnings.len(), 0);
        assert!(is_launcher_action("/usr/bin/hyprlock"));
    }

    #[test]
    fn a_custom_action_reaches_the_window_list() {
        let c = run(json!({ "bottomLeft": "@ipc:theme.toggleMode" }));
        let wins = windows(&c, &screens(&["HDMI-1"]));
        assert_eq!(wins.len(), 3);
        assert_eq!(wins[1].action.as_str(), "@ipc:theme.toggleMode");
    }

    // The built-ins are the surface's own, never the launcher's: routing
    // "lock" through the resolver would spawn a shell command called lock.
    #[test]
    fn the_built_in_names_are_not_launcher_actions() {
        assert!(!is_launcher_action("lock"));
        assert!(!is_launcher_action("screensaver"));
        assert!(!is_launcher_action("overview"));
        assert!(!is_launcher_action("none"));
    }

    // A malformed one is a typo like any other, so it takes the same path.
    #[test]
    fn a_malformed_ipc_string_is_refused() {
        assert!(!is_launcher_action("@ipc:showHistory"));
        let c = run(json!({ "topLeft": "@ipc:showHistory" }));
        assert_eq!(c.corners.top_left, Action::None);
        assert_eq!(c.warnings.len(), 1);
    }

    #[test]
    fn unknown_action_warns_and_falls_back_to_none() {
        let c = run(json!({ "bottomLeft": "explode" }));
        assert_eq!(c.corners.bottom_left, Action::None);
        assert_eq!(c.warnings.len(), 1);
        assert_eq!(
            c.warnings[0],
            "hotCorners.bottomLeft: unknown action \"explode\", leaving the corner inert"
        );
        assert!(!is_launcher_action("explode"));
    }

    #[test]
    fn non_string_action_warns_rather_than_throwing() {
        let c = run(json!({ "topRight": 7 }));
        assert_eq!(c.corners.top_right, Action::None);
        assert_eq!(c.warnings.len(), 1);
    }

    #[test]
    fn size_and_delay_are_clamped() {
        let big = run(json!({ "size": 4000, "delayMs": 999999 }));
        assert_eq!(big.size, 64);
        assert_eq!(big.delay_ms, 10000);
        let small = run(json!({ "size": 0, "delayMs": -50 }));
        assert_eq!(small.size, 1);
        assert_eq!(small.delay_ms, 0);
        assert_eq!(big.warnings.len(), 0);
    }

    #[test]
    fn non_numeric_size_warns_and_keeps_the_default() {
        let c = run(json!({ "size": "wide" }));
        assert_eq!(c.size, 4);
        assert_eq!(
            c.warnings,
            ["hotCorners.size: expected a number, got \"wide\""]
        );
    }

    #[test]
    fn edges_are_the_corner_two_and_only_those() {
        let bl = edges(Corner::BottomLeft);
        assert_eq!(
            (bl.bottom, bl.left, bl.top, bl.right),
            (true, true, false, false)
        );
        let tr = edges(Corner::TopRight);
        assert_eq!(
            (tr.top, tr.right, tr.bottom, tr.left),
            (true, true, false, false)
        );
    }

    #[test]
    fn windows_covers_every_screen_and_skips_none() {
        let c = resolve(None);
        let wins = windows(&c, &screens(&["HDMI-1", "DP-1"]));
        assert_eq!(wins.len(), 6);
        assert_eq!(wins[0].screen, "HDMI-1");
        assert_eq!(wins[0].corner, Corner::TopLeft);
        assert_eq!(wins[0].action, Action::Overview);
        assert_eq!(wins[1].corner, Corner::BottomLeft);
        assert_eq!(wins[1].action, Action::Screensaver);
        assert_eq!(wins[2].corner, Corner::BottomRight);
        assert_eq!(wins[2].action, Action::Lock);
        assert_eq!(wins[3].screen, "DP-1");
        assert_eq!(wins[5].screen, "DP-1");
    }

    #[test]
    fn windows_is_empty_when_disabled() {
        let c = run(json!({ "enabled": false }));
        assert_eq!(windows(&c, &screens(&["HDMI-1"])).len(), 0);
    }

    #[test]
    fn windows_is_empty_when_every_corner_is_none() {
        let c = run(json!({ "topLeft": "none", "bottomLeft": "none", "bottomRight": "none" }));
        assert_eq!(windows(&c, &screens(&["HDMI-1"])).len(), 0);
    }
}
