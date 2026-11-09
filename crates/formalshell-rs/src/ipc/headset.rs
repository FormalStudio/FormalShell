//! `headset`: `dismiss` takes the connect card down, which is what its
//! Escape bind runs; "none" when no card was up.

use super::registry::{Function, Reply, Target, Type, Value, ok, text};
use crate::wayland::App;

fn dismiss(app: &mut App, _: &[Value]) -> Reply {
    if app.headset_escape() { ok() } else { text("none") }
}

pub fn target() -> Target<App> {
    Target { name: "headset", functions: vec![Function { name: "dismiss", params: &[], ret: Type::String, call: dismiss }] }
}
