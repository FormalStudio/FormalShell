//! `screensaver`: start, stop, status, the frame pin a
//! recorder steps through, and `frameInfo`, whose `cycles` is its last key
//! on purpose (the rig greps `"cycles":0}`).

use super::registry::{Function, Reply, Target, Type, Value, ok};
use crate::wayland::App;

fn start(app: &mut App, _: &[Value]) -> Reply {
    app.saver_start();
    ok()
}

fn stop(app: &mut App, _: &[Value]) -> Reply {
    app.saver_stop();
    ok()
}

fn status(app: &mut App, _: &[Value]) -> Reply {
    Ok(app.saver_status())
}

fn frame(app: &mut App, args: &[Value]) -> Reply {
    if !app.saver_active() {
        return Err("screensaver not active".into());
    }
    let n = args[0].int();
    if n < 0 {
        return Err("frame must be >= 0".into());
    }
    app.saver_pin(i64::from(n));
    ok()
}

fn frame_info(app: &mut App, _: &[Value]) -> Reply {
    Ok(app.saver_frame_info())
}

pub fn target() -> Target<App> {
    Target {
        name: "screensaver",
        reads_screen: false,
        functions: vec![
            Function { name: "start", params: &[], ret: Type::String, call: start },
            Function { name: "stop", params: &[], ret: Type::String, call: stop },
            Function { name: "status", params: &[], ret: Type::String, call: status },
            Function { name: "frame", params: &[("n", Type::Int)], ret: Type::String, call: frame },
            Function { name: "frameInfo", params: &[], ret: Type::String, call: frame_info },
        ],
    }
}
