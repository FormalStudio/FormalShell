//! `notifications` and `reminder`.

use serde_json::json;

use super::registry::{Function, Reply, Target, Type, Value, ok, text};
use crate::services::notifications::{self, Op};
use crate::wayland::App;

fn on_off(on: bool) -> Reply {
    text(if on { "on" } else { "off" })
}

pub fn target() -> Target<App> {
    Target {
        name: "notifications",
        reads_screen: false,
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

fn status(app: &mut App, _: &[Value]) -> Reply {
    let center = app.center_numbers();
    Ok(notifications::status(&app.store.notifications, center))
}

fn dnd_state(app: &mut App, _: &[Value]) -> Reply {
    on_off(app.store.notifications.model.dnd)
}

fn toggle_dnd(app: &mut App, _: &[Value]) -> Reply {
    app.apply(Op::SetDnd(!app.store.notifications.model.dnd));
    on_off(app.store.notifications.model.dnd)
}

fn set_dnd(app: &mut App, args: &[Value]) -> Reply {
    app.apply(Op::SetDnd(args[0].bool()));
    on_off(app.store.notifications.model.dnd)
}

fn show_history(app: &mut App, _: &[Value]) -> Reply {
    let open = !app.store.notifications.center_open;
    app.set_center(open, None);
    ok()
}

fn clear(app: &mut App, _: &[Value]) -> Reply {
    app.apply(Op::Clear);
    ok()
}

fn clear_pending(app: &mut App, _: &[Value]) -> Reply {
    app.apply(Op::ClearPending);
    ok()
}

fn mark_all_seen(app: &mut App, _: &[Value]) -> Reply {
    app.apply(Op::MarkAllSeen);
    ok()
}

fn dismiss_all(app: &mut App, _: &[Value]) -> Reply {
    app.apply(Op::DismissAll);
    ok()
}

/// The front toast's whole group, which is what its close button closes.
/// `none` at an empty screen: a keybind fired there is not a failure.
fn dismiss_one(app: &mut App, _: &[Value]) -> Reply {
    let Some(ids) = app.store.notifications.front_group() else { return text("none") };
    app.apply(Op::DismissPopups(ids));
    ok()
}

fn invoke_last(app: &mut App, _: &[Value]) -> Reply {
    app.apply(Op::InvokeLast);
    ok()
}

fn expand(app: &mut App, args: &[Value]) -> Reply {
    let state = args[0].str();
    if state != "on" && state != "off" {
        return Err(format!("unknown expand state '{state}' (on|off)"));
    }
    app.apply(Op::Expand(state == "on"));
    on_off(app.store.notifications.stack_expanded)
}

pub fn reminder() -> Target<App> {
    Target {
        name: "reminder",
        reads_screen: false,
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

fn reminder_set(app: &mut App, args: &[Value]) -> Reply {
    let duration = args[0].str();
    let Some(seconds) = fs_info::reminders::parse_duration(duration) else {
        return Err(format!("could not read \"{duration}\" as a duration"));
    };
    let message = notifications::reminder_message(args[1].str(), crate::settings::REMINDER_MESSAGE.get(&app.store.config));
    app.apply(Op::SetReminder { seconds, message });
    let Some(e) = app.store.notifications.newest_reminder() else { return Err("the reminder was not stored".into()) };
    Ok(json!({
        "id": e.id,
        "message": e.message,
        "dueAt": fs_js::json_number(e.due_at),
        "dueClock": fs_info::reminders::due_clock(e.due_at),
    }))
}

fn reminder_show(app: &mut App, _: &[Value]) -> Reply {
    app.apply(Op::ShowReminders);
    ok()
}

fn reminder_clear(app: &mut App, _: &[Value]) -> Reply {
    let n = app.store.notifications.reminders.len();
    app.apply(Op::ClearReminders);
    text(format!("ok: cleared {n}"))
}

fn reminder_status(app: &mut App, _: &[Value]) -> Reply {
    Ok(app.store.notifications.reminder_status())
}
