//! `media`: the verbs over the active source. Every verb that acts
//! on a source it does not have (no player at all, or one that does not
//! implement that part of MPRIS) answers with an error string naming which,
//! rather than "ok" over a call that went nowhere.

use fs_types::Fraction;

use super::registry::{Function, Reply, Target, Type, Value, ok};
use crate::services::kopuz;
use crate::wayland::App;

pub fn target() -> Target<App> {
    Target {
        name: "media",
        reads_screen: false,
        functions: vec![
            Function { name: "playPause", params: &[], ret: Type::String, call: play_pause },
            Function { name: "next", params: &[], ret: Type::String, call: next },
            Function { name: "previous", params: &[], ret: Type::String, call: previous },
            Function { name: "shuffle", params: &[("mode", Type::String)], ret: Type::String, call: shuffle },
            Function { name: "loop", params: &[("mode", Type::String)], ret: Type::String, call: loop_mode },
            Function { name: "volume", params: &[("percent", Type::Int)], ret: Type::String, call: volume },
            Function { name: "raise", params: &[], ret: Type::String, call: raise },
            Function { name: "select", params: &[("id", Type::String)], ret: Type::String, call: select },
            Function { name: "output", params: &[("name", Type::String)], ret: Type::String, call: output },
            Function { name: "outputs", params: &[], ret: Type::String, call: outputs },
            Function { name: "players", params: &[], ret: Type::String, call: players },
            Function { name: "status", params: &[], ret: Type::String, call: status },
            Function { name: "lyrics", params: &[], ret: Type::String, call: lyrics },
            Function { name: "like", params: &[("mode", Type::String)], ret: Type::String, call: like },
            Function { name: "dontRecommend", params: &[], ret: Type::String, call: dont_recommend },
        ],
    }
}

fn play_pause(app: &mut App, _: &[Value]) -> Reply {
    app.store.media.play_pause();
    ok()
}

fn next(app: &mut App, _: &[Value]) -> Reply {
    app.store.media.next();
    ok()
}

fn previous(app: &mut App, _: &[Value]) -> Reply {
    app.store.media.previous();
    ok()
}

fn shuffle(app: &mut App, args: &[Value]) -> Reply {
    let Some(a) = app.store.media.active() else { return Err("no player".into()) };
    let Some(on) = a.shuffle else { return Err("player does not support shuffle".into()) };
    let mode = args[0].str();
    match mode {
        "on" => app.store.media.set_shuffle(true),
        "off" => app.store.media.set_shuffle(false),
        "toggle" => app.store.media.set_shuffle(!on),
        _ => return Err(format!("unknown mode {mode} (on|off|toggle)")),
    }
    ok()
}

fn loop_mode(app: &mut App, args: &[Value]) -> Reply {
    let Some(a) = app.store.media.active() else { return Err("no player".into()) };
    let Some(current) = a.loop_name else { return Err("player does not support loop".into()) };
    let mode = args[0].str();
    match mode {
        "cycle" => app.store.media.set_loop(fs_media::media::next_loop(current)),
        "none" | "track" | "playlist" => app.store.media.set_loop(mode),
        _ => return Err(format!("unknown mode {mode} (none|track|playlist|cycle)")),
    }
    ok()
}

/// Percent on the wire, the player's own 0..1 underneath: a CLI argument reads
/// better as 30 than 0.3.
fn volume(app: &mut App, args: &[Value]) -> Reply {
    let Some(a) = app.store.media.active() else { return Err("no player".into()) };
    if a.volume.is_none() {
        return Err("player does not support volume".into());
    }
    if a.volume_fixed {
        return Err("player does not let its volume be set".into());
    }
    let percent = args[0].int();
    if !(0..=100).contains(&percent) {
        return Err(format!("volume out of range {percent} (0-100)"));
    }
    app.store.media.set_volume(Fraction::from_fraction(f64::from(percent) / 100.0));
    ok()
}

fn raise(app: &mut App, _: &[Value]) -> Reply {
    let Some(a) = app.store.media.active() else { return Err("no player".into()) };
    if !a.can_raise {
        return Err("player does not support raise".into());
    }
    app.store.media.raise();
    ok()
}

/// A bus name, "radio", "iphone", "airplay" or a `bluez:` id, exactly as `status` and
/// `players` report it; "" puts the pick back on auto. An unknown one is an
/// error rather than a selection nothing can satisfy.
fn select(app: &mut App, args: &[Value]) -> Reply {
    let id = args[0].str();
    if !id.is_empty() && !app.store.media.players().iter().any(|r| r.id == id) {
        return Err(format!("no player {id}"));
    }
    app.apply(crate::store::Diff::Media(crate::services::media::Diff::Select(id.to_owned())));
    ok()
}

/// The source's output, by sink name as `outputs` lists it. Only listed while
/// the media panel is open or a stream is the picked source.
fn output(app: &mut App, args: &[Value]) -> Reply {
    let name = args[0].str();
    if app.store.media.active().is_none() {
        return Err("no player".into());
    }
    if !app.store.media.can_route() {
        return Err("source has no stream to move".into());
    }
    if !app.store.media.outputs().iter().any(|(id, _)| id == name) {
        return Err(format!("no output {name}"));
    }
    app.store.media.set_output(name);
    ok()
}

fn outputs(app: &mut App, _: &[Value]) -> Reply {
    Ok(app.store.media.outputs_json())
}

fn players(app: &mut App, _: &[Value]) -> Reply {
    Ok(app.store.media.players_json())
}

fn status(app: &mut App, _: &[Value]) -> Reply {
    Ok(app.store.media.status())
}

/// The lyrics read for headless checks: the display set, the lit
/// main line and the secondary ones worked out against the held position at
/// the moment of the call, the follow state, the output latency and the hold.
fn lyrics(app: &mut App, _: &[Value]) -> Reply {
    use crate::services::lyrics;
    let active = app.store.media.active();
    let position = active.as_ref().map_or(0.0, |a| a.position.as_secs_f64());
    let stale = active.as_ref().is_some_and(|a| a.position_stale);
    let settings = lyrics::Settings::read(&app.store.config);
    Ok(lyrics::status(&app.store.lyrics, position, stale, &settings))
}

/// kopuzd's heart and "don't recommend" for the playing Kopuz track, the
/// two actions Kopuz's own player bar offers (`media status`'s
/// `kopuz.actions`).
fn kopuz_act(app: &mut App, pick: impl FnOnce(&kopuz::State) -> Result<kopuz::Action, String>) -> Reply {
    let Some(k) = app.store.media.kopuz() else { return Err("no kopuz track".into()) };
    kopuz::send(pick(k)?);
    ok()
}

fn like(app: &mut App, args: &[Value]) -> Reply {
    let mode = args[0].str().to_owned();
    kopuz_act(app, |k| k.like(&mode))
}

fn dont_recommend(app: &mut App, _: &[Value]) -> Reply {
    kopuz_act(app, kopuz::State::dont_recommend_action)
}
