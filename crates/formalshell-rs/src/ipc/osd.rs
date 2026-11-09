//! `osd`: the bottom pill from outside. It shows itself on a
//! volume change and on `display brightnessStep`; this covers brightness set
//! by something else (it catches the cached backlight percent up and shows
//! it), media text, close and state.

use super::registry::{Function, Reply, Target, Type, Value, ok};
use crate::services::brightness;
use crate::surfaces::osd::Kind;
use crate::wayland::App;

fn volume(app: &mut App, _: &[Value]) -> Reply {
    app.osd_show(Kind::Volume, "");
    ok()
}

fn show_brightness(app: &mut App, _: &[Value]) -> Reply {
    if let Some(rt) = &app.runtime {
        rt.service(brightness::refresh);
    }
    app.osd_brightness_read();
    app.osd_show(Kind::Brightness, "");
    ok()
}

fn media(app: &mut App, a: &[Value]) -> Reply {
    app.osd_show(Kind::Media, a[0].str());
    ok()
}

fn close(app: &mut App, _: &[Value]) -> Reply {
    app.osd_close();
    ok()
}

fn state(app: &mut App, _: &[Value]) -> Reply {
    Ok(app.osd_state())
}

pub fn target() -> Target<App> {
    Target {
        name: "osd",
        functions: vec![
            Function { name: "volume", params: &[], ret: Type::String, call: volume },
            Function { name: "brightness", params: &[], ret: Type::String, call: show_brightness },
            Function { name: "media", params: &[("text", Type::String)], ret: Type::String, call: media },
            Function { name: "close", params: &[], ret: Type::String, call: close },
            Function { name: "state", params: &[], ret: Type::String, call: state },
        ],
    }
}
