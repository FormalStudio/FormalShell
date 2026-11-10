//! `visualizer`: the spectrum's style picked at runtime,
//! for exploring the styles without an edit to settings.json. The override is
//! in memory only, `style config` drops it.

use crate::ui::spectrum::styles;

use super::registry::{Function, Reply, Target, Type, Value, text};
use crate::services::visualizer::{self, Diff};
use crate::store;
use crate::wayland::App;

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
    crate::settings::VISUALIZER_STYLE.get(&app.store.config).to_owned()
}

fn current(app: &App) -> styles::Style {
    app.store.visualizer.style(&configured(app))
}

/// An id, `next`, `prev`, or `config` to follow settings.json again. Answers
/// the style now drawn, or the error line.
fn style(app: &mut App, args: &[Value]) -> Reply {
    let name = args[0].str();
    let next = match name {
        "config" => None,
        "next" => Some(current(app).step(1)),
        "prev" => Some(current(app).step(-1)),
        id => match styles::Style::parse(id) {
            Some(style) => Some(style),
            None => return Err(format!("unknown style {id} ({})", styles::ids().join("|"))),
        },
    };
    app.apply(store::Diff::Visualizer(Diff::Style(next)));
    text(current(app).id())
}

fn list(_: &mut App, _: &[Value]) -> Reply {
    text(styles::ids().join("\n"))
}

fn status(app: &mut App, _: &[Value]) -> Reply {
    let v = &app.store.visualizer;
    let configured = configured(app);
    let tempo_at = app.store.media.active().filter(|a| a.kind.tempo_driven() && v.running).map(|a| a.position.as_secs_f64());
    let frame = visualizer::frame_for(tempo_at, v.bpm);
    Ok(v.status(current(app), &configured, &frame))
}
