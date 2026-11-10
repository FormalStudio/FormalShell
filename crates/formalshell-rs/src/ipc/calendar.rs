//! `calendar`'s `select` and `status`: the panel's own
//! day selection, headless. A panel that is not open answers from a fresh
//! one, which is what an idle panel holds until it opens.

use fs_chrome::bar::panels::PanelId;

use super::registry::{Function, Json, Reply, Target, Type, Value};
use crate::surfaces::panel::Panel;
use crate::surfaces::panel::calendar::Calendar;
use crate::wayland::App;

pub fn target() -> Target<App> {
    Target {
        name: "calendar",
        functions: vec![
            Function { name: "select", params: &[("date", Type::String)], ret: Type::String, call: select },
            Function { name: "status", params: &[], ret: Type::String, call: status },
        ],
    }
}

fn select(app: &mut App, args: &[Value]) -> Reply {
    let date = args[0].str();
    match app.panel_mut::<Calendar>(PanelId::Calendar) {
        Some(c) => c.select_iso(date)?,
        None => Calendar::new(false).select_iso(date)?,
    }
    Ok(Json::Null)
}

fn status(app: &mut App, _: &[Value]) -> Reply {
    Ok(app.panel_status(PanelId::Calendar).unwrap_or_else(|| Calendar::new(false).status()))
}
