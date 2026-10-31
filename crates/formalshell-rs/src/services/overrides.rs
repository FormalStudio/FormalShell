//! The settings UI's catalogue and its writes: which keys it offers, on
//! which page, with which values, and the override each edit leaves in
//! state.json's `settings`. settings.json pins a key against all of it
//! (docs/superpowers/specs/2026-10-31-settings-ui.md).

use serde_json::{Value, json};

use super::{config, state, theme};
use crate::store::Store;

pub struct Page {
    pub id: &'static str,
    pub title: &'static str,
}

pub const PAGES: [Page; 4] = [
    Page { id: "appearance", title: "Appearance" },
    Page { id: "bar", title: "Bar" },
    Page { id: "desktop", title: "Desktop" },
    Page { id: "system", title: "System" },
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

const fn switch(page: &'static str, section: &'static str, key: &'static str, label: &'static str, default: bool) -> Key {
    Key { page, section, key, label, control: Control::Switch(default) }
}

const fn choice(
    page: &'static str,
    section: &'static str,
    key: &'static str,
    label: &'static str,
    values: &'static [(&'static str, &'static str)],
    default: &'static str,
) -> Key {
    Key { page, section, key, label, control: Control::Choice(values, default) }
}

/// `theme.mode`'s row: Light and Dark are the toggle's mode, Auto the key.
pub const MODE: &str = "theme.mode";

const CORNER: [(&str, &str); 4] = [("none", "None"), ("overview", "Overview"), ("screensaver", "Screensaver"), ("lock", "Lock")];

pub const KEYS: &[Key] = &[
    choice("appearance", "Theme", MODE, "Style", &[("light", "Light"), ("dark", "Dark"), ("auto", "Auto")], "dark"),
    switch("appearance", "Theme", "theme.reactiveRims", "Reactive rims", true),
    switch("appearance", "Motion", "motion.enabled", "Animations", true),
    switch("appearance", "Motion", "motion.morph", "Cards grow out of the bar", true),
    switch("appearance", "Motion", "wallpaper.animate", "Animated backdrop", false),
    choice("bar", "Placement", "layout", "Layout", &[("bar", "Bar"), ("sidebar", "Sidebar")], "bar"),
    choice("bar", "Placement", "bar.position", "Edge", &[("top", "Top"), ("bottom", "Bottom"), ("left", "Left"), ("right", "Right")], "top"),
    choice("bar", "Placement", "bar.style", "Bar style", &[("transparent", "Clear"), ("solid", "Solid"), ("corners", "Corners")], "transparent"),
    choice(
        "bar",
        "Notifications",
        "notifications.position",
        "Toasts",
        &[("bottom-right", "Bottom right"), ("top-right", "Top right"), ("bottom-left", "Bottom left"), ("top-left", "Top left")],
        "bottom-right",
    ),
    switch("bar", "Notifications", "fullscreen.hideChrome", "Hide over fullscreen windows", true),
    switch("desktop", "Clock", "desktopClock.enabled", "Desktop clock", false),
    choice("desktop", "Clock", "desktopClock.face", "Face", &[("pixel", "Pixel"), ("gooey", "Gooey"), ("serif", "Serif")], "pixel"),
    choice("desktop", "Clock", "desktopClock.width", "Digits", &[("wide", "Wide"), ("narrow", "Narrow")], "wide"),
    switch("desktop", "Clock", "desktopClock.hour12", "12-hour time", false),
    switch("desktop", "Clock", "desktopClock.depth", "Behind the subject", true),
    switch("desktop", "Hot corners", "hotCorners.enabled", "Hot corners", true),
    choice("desktop", "Hot corners", "hotCorners.topLeft", "Top left", &CORNER, "overview"),
    choice("desktop", "Hot corners", "hotCorners.topRight", "Top right", &CORNER, "none"),
    choice("desktop", "Hot corners", "hotCorners.bottomLeft", "Bottom left", &CORNER, "screensaver"),
    choice("desktop", "Hot corners", "hotCorners.bottomRight", "Bottom right", &CORNER, "lock"),
    switch("system", "Windows", "switcher.enabled", "Window switcher", true),
    switch("system", "Windows", "switcher.currentWorkspace", "Switch within the workspace", false),
    switch("system", "Windows", "workspaces.preview", "Workspace previews", true),
    switch("system", "Launcher", "menu.appGrid", "Apps as a grid", false),
    switch("system", "Media", "media.lyrics", "Lyrics", true),
    switch("system", "Media", "media.lyricsBlur", "Blur unlit lyrics", true),
    switch("system", "Media", "media.animatedBarCover", "Animated cover in the bar", true),
    switch("system", "Screen", "screensaver.guardMediaPlayback", "No screensaver during playback", true),
    switch("system", "Screen", "lock.beforeSleep", "Lock before sleep", true),
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
            "" => store.state.data.mode.as_str(),
            set => set,
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
    format!("error: {key} is set in settings.json")
}

fn check(key: &Key, value: &Value) -> Result<(), String> {
    match key.control {
        Control::Switch(_) if value.is_boolean() => Ok(()),
        Control::Switch(_) => Err(format!("error: {} takes true or false", key.key)),
        Control::Choice(values, _) => match value.as_str() {
            Some(v) if values.iter().any(|(x, _)| *x == v) => Ok(()),
            _ => {
                let names: Vec<&str> = values.iter().map(|(x, _)| *x).collect();
                Err(format!("error: {} takes one of {}", key.key, names.join(", ")))
            }
        },
    }
}

/// Writes `value` as `key`'s override, or says why not.
pub fn set(store: &Store, key: &str, value: Value) -> Result<(), String> {
    let Some(entry) = find(key) else { return Err(format!("error: unknown setting '{key}'")) };
    if store.config.pinned(key) {
        return Err(pinned_error(key));
    }
    check(entry, &value)?;
    if key == MODE && value != "auto" {
        let mode = value.as_str().unwrap_or("dark").to_owned();
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
        return Err(format!("error: unknown setting '{key}'"));
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
        assert_eq!(check(style, &json!("glass")), Err("error: bar.style takes one of transparent, solid, corners".into()));
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
