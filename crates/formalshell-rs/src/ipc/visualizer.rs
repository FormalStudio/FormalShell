//! `visualizer`: the spectrum's style picked at runtime,
//! for exploring the styles without an edit to settings.json. The override is
//! in memory only, `style config` drops it.

use fs_media::visualizer::model::{self, BAR_COUNT};
use fs_media::visualizer::styles;
use serde_json::{Value as Json, json};

use super::registry::{Function, Target, Type, Value};
use crate::services::visualizer::{self, Diff};
use crate::wayland::App;

fn text(s: impl Into<String>) -> Value {
    Value::Str(s.into())
}

pub fn target() -> Target<App> {
    Target {
        name: "visualizer",
        functions: vec![
            Function { name: "style", params: &[("name", Type::String)], ret: Type::String, call: style },
            Function { name: "styles", params: &[], ret: Type::String, call: list },
            Function { name: "status", params: &[], ret: Type::String, call: status },
        ],
    }
}

fn configured(app: &App) -> String {
    app.store.config.str("media.visualizerStyle").unwrap_or("bars").to_owned()
}

fn current(app: &App) -> String {
    app.store.visualizer.style(&configured(app))
}

/// An id, `next`, `prev`, or `config` to follow settings.json again. Answers
/// the style now drawn, or the error line.
fn style(app: &mut App, args: &[Value]) -> Value {
    let name = args[0].str();
    let next = match name {
        "config" => String::new(),
        "next" => styles::step(&current(app), 1).to_owned(),
        "prev" => styles::step(&current(app), -1).to_owned(),
        id if styles::is_known(id) => id.to_owned(),
        id => return text(format!("error: unknown style {id} ({})", styles::ids().join("|"))),
    };
    app.store.visualizer.apply(Diff::Style(next));
    text(current(app))
}

fn list(_: &mut App, _: &[Value]) -> Value {
    text(styles::ids().join("\n"))
}

fn status(app: &mut App, _: &[Value]) -> Value {
    let v = &app.store.visualizer;
    let configured = configured(app);
    // cava hears nothing of the phone, so its frame is drawn off the track's
    // tempo, the way the bar cell draws it.
    let frame = match app.store.media.active().filter(|a| a.kind == "iphone" && v.running) {
        Some(a) => visualizer::Frame {
            mono: model::beat_frame(a.position, v.bpm, BAR_COUNT, 0),
            left: model::beat_frame(a.position, v.bpm, BAR_COUNT, -1),
            right: model::beat_frame(a.position, v.bpm, BAR_COUNT, 1),
        },
        None => visualizer::frame(),
    };
    let levels = |l: &[f64]| Json::Array(l.iter().map(|x| fs_js::json_number(*x)).collect());
    text(
        json!({
            "style": current(app),
            "override": v.style_override,
            "configured": configured,
            "configuredKnown": styles::is_known(&configured),
            "running": v.running,
            "state": v.avail.as_str(),
            "levels": levels(&frame.mono),
            "levelsLeft": levels(&frame.left),
            "levelsRight": levels(&frame.right),
        })
        .to_string(),
    )
}
