//! The settings UI's catalogue and its writes: which keys it offers, on
//! which page, with which values, and the override each edit leaves in
//! state.json's `settings`. settings.json pins a key against all of it
//! (docs/superpowers/specs/2026-10-31-settings-ui.md).

use fs_chrome::bar::panels::PanelId;
use serde_json::{Value, json};

use super::{config, state, theme};
use crate::settings::{self, Flag, Word};
use crate::store::Store;

pub struct Page {
    pub id: &'static str,
    pub title: &'static str,
    /// The panel a row of which opens this page, where its back goes; none
    /// for a page the tile grid opens.
    pub parent: Option<PanelId>,
}

pub const PAGES: [Page; 4] = [
    Page { id: "appearance", title: "Appearance", parent: None },
    Page { id: "bar", title: "Bar", parent: Some(PanelId::SettingsAppearance) },
    Page { id: "desktop", title: "Desktop", parent: Some(PanelId::SettingsAppearance) },
    Page { id: "system", title: "System", parent: None },
];

pub enum Control {
    /// A switch, and the value a key nobody sets reads as.
    Switch(bool),
    /// One of these `(value, name)`, and the value nobody setting it reads as.
    Choice(&'static [(&'static str, &'static str)], &'static str),
}

pub struct Key {
    pub page: &'static str,
    pub section: &'static str,
    pub key: &'static str,
    pub label: &'static str,
    pub control: Control,
}

const fn switch(page: &'static str, section: &'static str, flag: Flag, label: &'static str) -> Key {
    Key { page, section, key: flag.key, label, control: Control::Switch(flag.default) }
}

const fn choice(page: &'static str, section: &'static str, word: Word, label: &'static str, values: &'static [(&'static str, &'static str)]) -> Key {
    Key { page, section, key: word.key, label, control: Control::Choice(values, word.default) }
}

/// `theme.mode`'s row: Light and Dark are the toggle's mode, Auto the key.
pub const MODE: &str = settings::THEME_MODE.key;

const CORNER: [(&str, &str); 4] = [("none", "None"), ("overview", "Overview"), ("screensaver", "Screensaver"), ("lock", "Lock")];

pub const KEYS: &[Key] = &[
    choice("appearance", "Theme", settings::THEME_MODE, "Style", &[("light", "Light"), ("dark", "Dark"), ("auto", "Auto")]),
    switch("appearance", "Theme", settings::REACTIVE_RIMS, "Reactive rims"),
    switch("appearance", "Motion", settings::MOTION, "Animations"),
    switch("appearance", "Motion", settings::MORPH, "Cards grow out of the bar"),
    switch("appearance", "Motion", settings::WALLPAPER_ANIMATE, "Animated backdrop"),
    choice("bar", "Placement", settings::LAYOUT, "Layout", &[("bar", "Bar"), ("sidebar", "Sidebar")]),
    choice("bar", "Placement", settings::BAR_POSITION, "Edge", &[("top", "Top"), ("bottom", "Bottom"), ("left", "Left"), ("right", "Right")]),
    choice("bar", "Placement", settings::BAR_STYLE, "Bar style", &[("transparent", "Clear"), ("solid", "Solid"), ("corners", "Corners")]),
    choice(
        "bar",
        "Notifications",
        settings::NOTIFICATIONS_POSITION,
        "Toasts",
        &[("bottom-right", "Bottom right"), ("top-right", "Top right"), ("bottom-left", "Bottom left"), ("top-left", "Top left")],
    ),
    switch("bar", "Notifications", settings::HIDE_CHROME, "Hide over fullscreen windows"),
    switch("desktop", "Clock", settings::CLOCK_ENABLED, "Desktop clock"),
    choice("desktop", "Clock", settings::CLOCK_FACE, "Face", &[("pixel", "Pixel"), ("gooey", "Gooey"), ("serif", "Serif")]),
    choice("desktop", "Clock", settings::CLOCK_WIDTH, "Digits", &[("wide", "Wide"), ("narrow", "Narrow")]),
    switch("desktop", "Clock", settings::CLOCK_HOUR12, "12-hour time"),
    switch("desktop", "Clock", settings::CLOCK_DEPTH, "Behind the subject"),
    switch("desktop", "Hot corners", settings::HOT_CORNERS_ENABLED, "Hot corners"),
    choice("desktop", "Hot corners", settings::HOT_CORNER_TOP_LEFT, "Top left", &CORNER),
    choice("desktop", "Hot corners", settings::HOT_CORNER_TOP_RIGHT, "Top right", &CORNER),
    choice("desktop", "Hot corners", settings::HOT_CORNER_BOTTOM_LEFT, "Bottom left", &CORNER),
    choice("desktop", "Hot corners", settings::HOT_CORNER_BOTTOM_RIGHT, "Bottom right", &CORNER),
    switch("system", "Windows", settings::SWITCHER, "Window switcher"),
    switch("system", "Windows", settings::SWITCHER_CURRENT_WORKSPACE, "Switch within the workspace"),
    switch("system", "Windows", settings::WORKSPACE_PREVIEW, "Workspace previews"),
    switch("system", "Launcher", settings::APP_GRID, "Apps as a grid"),
    switch("system", "Media", settings::LYRICS, "Lyrics"),
    switch("system", "Media", settings::LYRICS_BLUR, "Blur unlit lyrics"),
    switch("system", "Media", settings::ANIMATED_BAR_COVER, "Animated cover in the bar"),
    switch("system", "Screen", settings::GUARD_MEDIA, "No screensaver during playback"),
    switch("system", "Screen", settings::LOCK_BEFORE_SLEEP, "Lock before sleep"),
];

pub fn find(key: &str) -> Option<&'static Key> {
    KEYS.iter().find(|k| k.key == key)
}

pub fn page(id: &str) -> Option<&'static Page> {
    PAGES.iter().find(|p| p.id == id)
}

pub fn keys(page: &str) -> impl Iterator<Item = &'static Key> + '_ {
    KEYS.iter().filter(move |k| k.page == page)
}

/// A value from the command line: JSON, or a bare word as a string.
pub fn parse_value(text: &str) -> Value {
    serde_json::from_str(text).unwrap_or_else(|_| Value::String(text.to_owned()))
}

/// What the key's control shows now: the switch's state, or the index of
/// the choice (the default where the resolved value is none of them).
pub fn current(store: &Store, key: &Key) -> Value {
    if key.key == MODE {
        let mode = match theme::mode_key(store.config.settings()) {
            fs_theme::theme::ModeKey::Unset => store.state.data.mode.as_str(),
            set => set.as_str(),
        };
        return Value::String(mode.to_owned());
    }
    let resolved = store.config.get(key.key);
    match key.control {
        Control::Switch(default) => Value::Bool(resolved.and_then(Value::as_bool).unwrap_or(default)),
        Control::Choice(values, default) => {
            let at = resolved.and_then(Value::as_str).filter(|v| values.iter().any(|(x, _)| x == v));
            Value::String(at.unwrap_or(default).to_owned())
        }
    }
}

pub fn override_of<'a>(store: &'a Store, key: &str) -> Option<&'a Value> {
    config::lookup(&store.state.data.settings, key)
}

fn pinned_error(key: &str) -> String {
    format!("{key} is set in settings.json")
}

fn check(key: &Key, value: &Value) -> Result<(), String> {
    match key.control {
        Control::Switch(_) if value.is_boolean() => Ok(()),
        Control::Switch(_) => Err(format!("{} takes true or false", key.key)),
        Control::Choice(values, _) => match value.as_str() {
            Some(v) if values.iter().any(|(x, _)| *x == v) => Ok(()),
            _ => {
                let names: Vec<&str> = values.iter().map(|(x, _)| *x).collect();
                Err(format!("{} takes one of {}", key.key, names.join(", ")))
            }
        },
    }
}

/// Writes `value` as `key`'s override, or says why not.
pub fn set(store: &Store, key: &str, value: Value) -> Result<(), String> {
    let Some(entry) = find(key) else { return Err(format!("unknown setting '{key}'")) };
    if store.config.pinned(key) {
        return Err(pinned_error(key));
    }
    check(entry, &value)?;
    if key == MODE && value != "auto" {
        let mode = fs_theme::theme::Mode::parse(value.as_str().unwrap_or("dark"));
        state::set(vec![
            state::Field::SettingKey(MODE.into(), None),
            state::Field::ModeOverride(Value::Null),
            state::Field::Mode(mode),
        ]);
        return Ok(());
    }
    state::set(vec![state::Field::SettingKey(key.into(), Some(value))]);
    Ok(())
}

/// Drops `key`'s override.
pub fn reset(store: &Store, key: &str) -> Result<(), String> {
    if find(key).is_none() {
        return Err(format!("unknown setting '{key}'"));
    }
    if store.config.pinned(key) {
        return Err(pinned_error(key));
    }
    state::set(vec![state::Field::SettingKey(key.into(), None)]);
    Ok(())
}

pub fn describe(store: &Store, key: &Key) -> Value {
    json!({
        "key": key.key,
        "page": key.page,
        "value": store.config.get(key.key),
        "shown": current(store, key),
        "override": override_of(store, key.key),
        "file": store.config.file_get(key.key),
        "pinned": store.config.pinned(key.key),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_key_is_on_a_page_once_and_every_choice_has_a_default() {
        for key in KEYS {
            assert!(page(key.page).is_some(), "{}", key.key);
            assert_eq!(KEYS.iter().filter(|k| k.key == key.key).count(), 1, "{}", key.key);
            if let Control::Choice(values, default) = key.control {
                assert!(values.len() >= 2, "{}", key.key);
                assert!(values.iter().any(|(v, _)| *v == default), "{}", key.key);
            }
        }
    }

    #[test]
    fn values_are_checked_against_the_control() {
        let style = find("bar.style").unwrap();
        assert!(check(style, &json!("solid")).is_ok());
        assert_eq!(check(style, &json!("glass")), Err("bar.style takes one of transparent, solid, corners".into()));
        assert_eq!(check(find("hotCorners.topLeft").unwrap(), &json!("none")), Ok(()));
        let morph = find("motion.morph").unwrap();
        assert!(check(morph, &json!(false)).is_ok());
        assert!(check(morph, &json!("false")).is_err());
    }

    #[test]
    fn bare_words_are_strings() {
        assert_eq!(parse_value("true"), json!(true));
        assert_eq!(parse_value("\"x\""), json!("x"));
        assert_eq!(parse_value("sidebar"), json!("sidebar"));
    }
}
