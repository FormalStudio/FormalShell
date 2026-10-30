//! `notifications` and `reminder`.

use serde_json::json;

use super::registry::{Function, Target, Type, Value};
use crate::services::notifications;
use crate::store::Topic;
use crate::surfaces;
use crate::wayland::App;

fn text(s: impl Into<String>) -> Value {
    Value::Str(s.into())
}

fn on_off(on: bool) -> Value {
    text(if on { "on" } else { "off" })
}

fn moved(app: &mut App) {
    surfaces::changed(app, Topic::Notifications);
}

pub fn target() -> Target<App> {
    Target {
        name: "notifications",
        functions: vec![
            Function { name: "status", params: &[], ret: Type::String, call: status },
            Function { name: "dndState", params: &[], ret: Type::String, call: dnd_state },
            Function { name: "toggleDnd", params: &[], ret: Type::String, call: toggle_dnd },
            Function { name: "setDnd", params: &[("on", Type::Bool)], ret: Type::String, call: set_dnd },
            Function { name: "showHistory", params: &[], ret: Type::String, call: show_history },
            Function { name: "clear", params: &[], ret: Type::String, call: clear },
            Function { name: "clearPending", params: &[], ret: Type::String, call: clear_pending },
            Function { name: "markAllSeen", params: &[], ret: Type::String, call: mark_all_seen },
            Function { name: "dismissAll", params: &[], ret: Type::String, call: dismiss_all },
            Function { name: "dismissOne", params: &[], ret: Type::String, call: dismiss_one },
            Function { name: "invokeLast", params: &[], ret: Type::String, call: invoke_last },
            Function { name: "expand", params: &[("state", Type::String)], ret: Type::String, call: expand },
        ],
    }
}

fn status(app: &mut App, _: &[Value]) -> Value {
    let center = app.center_numbers();
    text(notifications::status(&app.store.notifications, center))
}

fn dnd_state(app: &mut App, _: &[Value]) -> Value {
    on_off(app.store.notifications.model.dnd)
}

fn toggle_dnd(app: &mut App, _: &[Value]) -> Value {
    let on = !app.store.notifications.model.dnd;
    app.store.notifications.set_dnd(on);
    moved(app);
    on_off(app.store.notifications.model.dnd)
}

fn set_dnd(app: &mut App, args: &[Value]) -> Value {
    app.store.notifications.set_dnd(args[0].bool());
    moved(app);
    on_off(app.store.notifications.model.dnd)
}

fn show_history(app: &mut App, _: &[Value]) -> Value {
    let open = !app.store.notifications.center_open;
    text(app.set_center(open, None))
}

fn clear(app: &mut App, _: &[Value]) -> Value {
    app.store.notifications.dismiss_all();
    app.store.notifications.clear_pending();
    moved(app);
    text("ok")
}

fn clear_pending(app: &mut App, _: &[Value]) -> Value {
    app.store.notifications.clear_pending();
    moved(app);
    text("ok")
}

fn mark_all_seen(app: &mut App, _: &[Value]) -> Value {
    app.store.notifications.mark_all_seen();
    moved(app);
    text("ok")
}

fn dismiss_all(app: &mut App, _: &[Value]) -> Value {
    app.store.notifications.dismiss_all();
    moved(app);
    text("ok")
}

/// The front toast's whole group, which is what its close button closes.
/// `none` at an empty screen: a keybind fired there is not a failure.
fn dismiss_one(app: &mut App, _: &[Value]) -> Value {
    let Some(ids) = app.store.notifications.front_group() else { return text("none") };
    app.store.notifications.dismiss_popup_group(&ids);
    moved(app);
    text("ok")
}

fn invoke_last(app: &mut App, _: &[Value]) -> Value {
    app.store.notifications.invoke_last();
    moved(app);
    text("ok")
}

fn expand(app: &mut App, args: &[Value]) -> Value {
    let state = args[0].str();
    if state != "on" && state != "off" {
        return text(format!("error: unknown expand state '{state}' (on|off)"));
    }
    app.store.notifications.stack_expanded = state == "on";
    moved(app);
    on_off(app.store.notifications.stack_expanded)
}

pub fn reminder() -> Target<App> {
    Target {
        name: "reminder",
        functions: vec![
            Function {
                name: "set",
                params: &[("duration", Type::String), ("message", Type::String)],
                ret: Type::String,
                call: reminder_set,
            },
            Function { name: "show", params: &[], ret: Type::String, call: reminder_show },
            Function { name: "clear", params: &[], ret: Type::String, call: reminder_clear },
            Function { name: "status", params: &[], ret: Type::String, call: reminder_status },
        ],
    }
}

fn reminder_set(app: &mut App, args: &[Value]) -> Value {
    let duration = args[0].str().to_owned();
    let fallback = app.store.config.str("reminders.defaultMessage").unwrap_or("Time's up").to_owned();
    let Some(e) = app.store.notifications.set_reminder(&duration, args[1].str(), &fallback) else {
        return text(format!("error: could not read \"{duration}\" as a duration"));
    };
    moved(app);
    text(format!(
        r#"{{"id":{},"message":{},"dueAt":{},"dueClock":{}}}"#,
        fs_js::stringify(&json!(e.id)),
        fs_js::stringify(&json!(e.message)),
        fs_js::num_str(e.due_at),
        fs_js::stringify(&json!(fs_info::reminders::due_clock(e.due_at))),
    ))
}

fn reminder_show(app: &mut App, _: &[Value]) -> Value {
    app.store.notifications.show_reminders();
    moved(app);
    text("ok")
}

fn reminder_clear(app: &mut App, _: &[Value]) -> Value {
    let n = app.store.notifications.clear_reminders();
    moved(app);
    text(format!("ok: cleared {n}"))
}

fn reminder_status(app: &mut App, _: &[Value]) -> Value {
    text(app.store.notifications.reminder_status())
}
