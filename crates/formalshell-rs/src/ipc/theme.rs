//! `theme` and `wallpaper`: their verbs.

use super::registry::{Function, Reply, Target, Type, Value, ok, text};
use crate::services::{state, theme};
use crate::wayland::App;

pub fn target() -> Target<App> {
    Target {
        name: "theme",
        functions: vec![
            Function { name: "retheme", params: &[], ret: Type::String, call: retheme },
            Function { name: "mode", params: &[("m", Type::String)], ret: Type::String, call: mode },
            Function { name: "status", params: &[], ret: Type::String, call: status },
        ],
    }
}

pub fn wallpaper() -> Target<App> {
    Target {
        name: "wallpaper",
        functions: vec![
            Function { name: "set", params: &[("path", Type::String)], ret: Type::String, call: wallpaper_set },
            Function { name: "pair", params: &[("light", Type::String), ("dark", Type::String)], ret: Type::String, call: wallpaper_pair },
            Function { name: "setFor", params: &[("mode", Type::String), ("path", Type::String)], ret: Type::String, call: wallpaper_set_for },
            Function { name: "get", params: &[], ret: Type::String, call: wallpaper_get },
        ],
    }
}

fn retheme(_: &mut App, _: &[Value]) -> Reply {
    theme::retheme();
    ok()
}

/// Every mode write goes through the engine's rules: under `theme.mode:
/// "auto"` the write is a snooze the engine records once it lands, and a
/// pinned key refuses one outright.
fn mode(app: &mut App, args: &[Value]) -> Reply {
    let ask = theme::Ask::parse(args[0].str())?;
    text(theme::set_mode(&app.store, ask)?.as_str())
}

fn status(app: &mut App, _: &[Value]) -> Reply {
    Ok(theme::status(&app.store))
}

/// An absolute picture path, or `sky` (or the picker's `sky:<step>` cell)
/// for the sky over the picture, as `wallpaper.color: "sky"` sets it.
fn wallpaper_set(_: &mut App, args: &[Value]) -> Reply {
    let path = args[0].str();
    let sky = path == fs_theme::theme::SKY || crate::services::wallpaper::sky(path).is_some();
    if !path.starts_with('/') && !sky {
        return Err("path must be absolute or sky".into());
    }
    state::set_wallpaper(state::Choice::Both(path.into()), None);
    ok()
}

/// Both slots in one write, the mode left alone.
fn wallpaper_pair(_: &mut App, args: &[Value]) -> Reply {
    let (light, dark) = (args[0].str(), args[1].str());
    if !light.starts_with('/') || !dark.starts_with('/') {
        return Err("paths must be absolute".into());
    }
    state::set_wallpaper(state::Choice::Pair(light.into(), dark.into()), None);
    ok()
}

/// One mode's slot, the mode left alone.
fn wallpaper_set_for(_: &mut App, args: &[Value]) -> Reply {
    let Some(half) = fs_theme::theme::Mode::parse_exact(args[0].str()) else {
        return Err("mode must be light or dark".into());
    };
    let path = args[1].str();
    if !path.starts_with('/') {
        return Err("path must be absolute".into());
    }
    state::set_wallpaper(state::Choice::Slot(half, path.into()), None);
    ok()
}

/// The picture shown now, the mode's half of a pair.
fn wallpaper_get(app: &mut App, _: &[Value]) -> Reply {
    let data = &app.store.state.data;
    text(data.picture(data.mode))
}
