//! `tray`: the items and where the tray lives, an
//! item's Activate, and the shell-owned menu opened and walked headlessly.
//! Opening and closing the second bar is `panel open|close|toggle
//! trayoverflow`, so there is no verb for it here.

use serde_json::json;

use super::registry::{Function, Target, Type, Value};
use crate::wayland::App;

fn text(s: impl Into<String>) -> Value {
    Value::Str(s.into())
}

pub fn target() -> Target<App> {
    Target {
        name: "tray",
        functions: vec![
            Function { name: "status", params: &[], ret: Type::String, call: status },
            Function { name: "activate", params: &[("id", Type::String)], ret: Type::String, call: activate },
            Function { name: "menu", params: &[("id", Type::String)], ret: Type::String, call: menu },
            Function { name: "menucursor", params: &[("delta", Type::String)], ret: Type::String, call: menucursor },
            Function { name: "menuactivate", params: &[], ret: Type::String, call: menuactivate },
        ],
    }
}

/// `inline` is what the strip settled on: the whole tray (`hidden` empty)
/// or none of it, the whole tray in the second bar.
fn status(app: &mut App, _: &[Value]) -> Value {
    let items = &app.store.tray.items;
    let inline = app.tray_inline();
    let listed: Vec<_> = items.iter().map(|i| json!({"id": i.id, "title": i.title, "hasMenu": i.has_menu})).collect();
    let hidden: Vec<&str> = items.iter().skip(inline).map(|i| i.id.as_str()).collect();
    text(
        json!({
            "items": listed,
            "overflow": {"open": app.panel_open() == Some(crate::surfaces::panel::tray::ID), "inline": inline, "hidden": hidden},
        })
        .to_string(),
    )
}

fn activate(app: &mut App, args: &[Value]) -> Value {
    text(app.tray_activate(args[0].str()))
}

fn menu(app: &mut App, args: &[Value]) -> Value {
    text(app.tray_menu(args[0].str()))
}

/// JavaScript's `parseInt(s, 10)`: a leading integer, whatever follows it.
fn parse_int(s: &str) -> Option<i32> {
    let s = s.trim_start();
    let (sign, digits) = match s.strip_prefix('-') {
        Some(rest) => (-1, rest),
        None => (1, s.strip_prefix('+').unwrap_or(s)),
    };
    let end = digits.find(|c: char| !c.is_ascii_digit()).unwrap_or(digits.len());
    digits[..end].parse::<i64>().ok().map(|n| (n * sign).clamp(i32::MIN as i64, i32::MAX as i64) as i32)
}

fn menucursor(app: &mut App, args: &[Value]) -> Value {
    if app.menu.as_ref().is_none_or(|p| !p.card.is_open()) {
        return text("error: no tray menu open");
    }
    match parse_int(args[0].str()) {
        Some(step) => text(app.tray_menu_cursor(step)),
        None => text("error: delta must be an integer"),
    }
}

fn menuactivate(app: &mut App, _: &[Value]) -> Value {
    text(app.tray_menu_activate())
}

#[cfg(test)]
mod tests {
    use super::parse_int;

    #[test]
    fn parse_int_reads_a_leading_integer_like_javascript() {
        assert_eq!(parse_int("1"), Some(1));
        assert_eq!(parse_int(" -2"), Some(-2));
        assert_eq!(parse_int("+3x"), Some(3));
        assert_eq!(parse_int("1.5"), Some(1));
        assert_eq!(parse_int("x"), None);
        assert_eq!(parse_int(""), None);
    }
}
