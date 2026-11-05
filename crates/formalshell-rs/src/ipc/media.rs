//! `media`: the verbs over the active source. Every verb that acts
//! on a source it does not have (no player at all, or one that does not
//! implement that part of MPRIS) answers with an error string naming which,
//! rather than "ok" over a call that went nowhere.

use super::registry::{Function, Target, Type, Value};
use crate::services::kopuz;
use crate::store::Topic;
use crate::wayland::App;

fn text(s: impl Into<String>) -> Value {
    Value::Str(s.into())
}

const OK: &str = "ok";

pub fn target() -> Target<App> {
    Target {
        name: "media",
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

fn play_pause(app: &mut App, _: &[Value]) -> Value {
    app.store.media.play_pause();
    text(OK)
}

fn next(app: &mut App, _: &[Value]) -> Value {
    app.store.media.next();
    text(OK)
}

fn previous(app: &mut App, _: &[Value]) -> Value {
    app.store.media.previous();
    text(OK)
}

fn shuffle(app: &mut App, args: &[Value]) -> Value {
    let Some(a) = app.store.media.active() else { return text("error: no player") };
    let Some(on) = a.shuffle else { return text("error: player does not support shuffle") };
    let mode = args[0].str();
    match mode {
        "on" => app.store.media.set_shuffle(true),
        "off" => app.store.media.set_shuffle(false),
        "toggle" => app.store.media.set_shuffle(!on),
        _ => return text(format!("error: unknown mode {mode} (on|off|toggle)")),
    }
    text(OK)
}

fn loop_mode(app: &mut App, args: &[Value]) -> Value {
    let Some(a) = app.store.media.active() else { return text("error: no player") };
    let Some(current) = a.loop_name else { return text("error: player does not support loop") };
    let mode = args[0].str();
    match mode {
        "cycle" => app.store.media.set_loop(fs_media::media::next_loop(current)),
        "none" | "track" | "playlist" => app.store.media.set_loop(mode),
        _ => return text(format!("error: unknown mode {mode} (none|track|playlist|cycle)")),
    }
    text(OK)
}

/// Percent on the wire, the player's own 0..1 underneath: a CLI argument reads
/// better as 30 than 0.3.
fn volume(app: &mut App, args: &[Value]) -> Value {
    let Some(a) = app.store.media.active() else { return text("error: no player") };
    if a.volume.is_none() {
        return text("error: player does not support volume");
    }
    if a.volume_fixed {
        return text("error: player does not let its volume be set");
    }
    let percent = args[0].int();
    if !(0..=100).contains(&percent) {
        return text(format!("error: volume out of range {percent} (0-100)"));
    }
    app.store.media.set_volume(f64::from(percent) / 100.0);
    text(OK)
}

fn raise(app: &mut App, _: &[Value]) -> Value {
    let Some(a) = app.store.media.active() else { return text("error: no player") };
    if !a.can_raise {
        return text("error: player does not support raise");
    }
    app.store.media.raise();
    text(OK)
}

/// A bus name, "radio", "iphone", "airplay" or a `bluez:` id, exactly as `status` and
/// `players` report it; "" puts the pick back on auto. An unknown one is an
/// error rather than a selection nothing can satisfy.
fn select(app: &mut App, args: &[Value]) -> Value {
    let id = args[0].str();
    if !id.is_empty() && !app.store.media.players().iter().any(|r| r.id == id) {
        return text(format!("error: no player {id}"));
    }
    app.store.media.select(id);
    crate::services::kopuz::sync(&app.store.media);
    crate::services::lyrics::sync(&app.store.media, &app.store.config, &app.store.lyrics);
    crate::surfaces::changed(app, Topic::Media);
    text(OK)
}

/// The source's output, by sink name as `outputs` lists it. Only listed while
/// the media panel is open or a stream is the picked source.
fn output(app: &mut App, args: &[Value]) -> Value {
    let name = args[0].str();
    if app.store.media.active().is_none() {
        return text("error: no player");
    }
    if !app.store.media.can_route() {
        return text("error: source has no stream to move");
    }
    if !app.store.media.outputs().iter().any(|(id, _)| id == name) {
        return text(format!("error: no output {name}"));
    }
    app.store.media.set_output(name);
    text(OK)
}

fn outputs(app: &mut App, _: &[Value]) -> Value {
    text(app.store.media.outputs_json().to_string())
}

fn players(app: &mut App, _: &[Value]) -> Value {
    text(app.store.media.players_json().to_string())
}

fn status(app: &mut App, _: &[Value]) -> Value {
    text(app.store.media.status().to_string())
}

/// LyricsService's own read for headless checks: the display set, the lit
/// main line and the secondary ones worked out against the held position at
/// the moment of the call, the follow state, the output latency and the hold.
fn lyrics(app: &mut App, _: &[Value]) -> Value {
    use crate::services::lyrics;
    let position = app.store.media.active().map_or(0.0, |a| a.position);
    let settings = lyrics::Settings::read(&app.store.config);
    text(lyrics::status(&app.store.lyrics, position, &settings).to_string())
}

/// kopuzd's heart and "don't recommend" for the playing Kopuz track, the
/// two actions Kopuz's own player bar offers (`media status`'s
/// `kopuz.actions`).
fn kopuz_act(app: &mut App, pick: impl FnOnce(&kopuz::State) -> Result<kopuz::Action, String>) -> Value {
    let Some(k) = app.store.media.kopuz() else { return text("error: no kopuz track") };
    match pick(k) {
        Ok(action) => {
            if let Some(rt) = &app.runtime {
                rt.service(move |ctx| kopuz::act(ctx, action));
            }
            text(OK)
        }
        Err(e) => text(e),
    }
}

fn like(app: &mut App, args: &[Value]) -> Value {
    let mode = args[0].str().to_owned();
    kopuz_act(app, |k| k.like(&mode))
}

fn dont_recommend(app: &mut App, _: &[Value]) -> Value {
    kopuz_act(app, kopuz::State::dont_recommend_action)
}
