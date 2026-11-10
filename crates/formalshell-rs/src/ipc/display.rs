//! `display` and `hdr`: the Display panel's
//! output controls without the panel, the brightness keys, and HDR for a
//! compositor keybind. Each answers "ok" or why nothing was sent.

use fs_system::display::outputs;

use super::registry::{Function, Reply, Target, Type, Value, ok, text};
use crate::services::display;
use crate::services::compositor::{self, Command};
use crate::wayland::App;

pub fn target() -> Target<App> {
    Target {
        name: "display",
        reads_screen: false,
        functions: vec![
            Function { name: "scale", params: &[("output", Type::String), ("scale", Type::Real)], ret: Type::String, call: scale },
            Function { name: "mirror", params: &[("output", Type::String), ("source", Type::String)], ret: Type::String, call: mirror },
            Function { name: "enable", params: &[("output", Type::String), ("enabled", Type::Bool)], ret: Type::String, call: enable },
            Function { name: "brightnessStep", params: &[("delta", Type::Int)], ret: Type::String, call: brightness_step },
        ],
    }
}

pub fn hdr() -> Target<App> {
    Target {
        name: "hdr",
        reads_screen: false,
        functions: vec![
            Function { name: "toggle", params: &[], ret: Type::String, call: hdr_toggle },
            Function { name: "enable", params: &[], ret: Type::String, call: hdr_enable },
            Function { name: "disable", params: &[], ret: Type::String, call: hdr_disable },
            Function {
                name: "setOutput",
                params: &[("output", Type::String), ("enabled", Type::Bool)],
                ret: Type::String,
                call: hdr_set_output,
            },
            Function { name: "rule", params: &[("output", Type::String)], ret: Type::String, call: hdr_rule },
            Function { name: "status", params: &[], ret: Type::String, call: hdr_status },
        ],
    }
}

/// A change nothing was sent for answers its reason as it is, without the
/// `error:` prefix the recorded replies never had.
fn refusal(r: Result<(), String>) -> Reply {
    match r {
        Ok(()) => ok(),
        Err(why) => text(why),
    }
}

/// Why the change may not go out, if it may not.
fn refuse(app: &App, output: &str) -> Result<(), String> {
    if !display::config_available() {
        return Err("no compositor".into());
    }
    unknown(app, output)
}

fn unknown(app: &App, output: &str) -> Result<(), String> {
    if outputs::find_output(&app.store.compositor.outputs, output).is_none() {
        return Err(format!("unknown output: {output}"));
    }
    Ok(())
}

fn scale(app: &mut App, args: &[Value]) -> Reply {
    let output = args[0].str();
    if let Err(why) = refuse(app, output) {
        return text(why);
    }
    compositor::send(Command::SetOutputScale(output.to_owned(), args[1].real()));
    ok()
}

/// An empty `source` clears the mirror.
fn mirror(app: &mut App, args: &[Value]) -> Reply {
    let output = args[0].str();
    if !display::config_available() {
        return text("mirroring unsupported");
    }
    if let Err(why) = refuse(app, output) {
        return text(why);
    }
    compositor::send(Command::SetOutputMirror(output.to_owned(), args[1].str().to_owned()));
    ok()
}

fn enable(app: &mut App, args: &[Value]) -> Reply {
    let output = args[0].str();
    if !display::config_available() {
        return text("no compositor");
    }
    if let Err(why) = unknown(app, output) {
        return text(why);
    }
    compositor::send(Command::SetOutputEnabled(output.to_owned(), args[1].bool()));
    ok()
}

/// `delta` percent on the focused output: its backlight on a laptop's own
/// panel, DDC on a monitor.
fn brightness_step(app: &mut App, args: &[Value]) -> Reply {
    let output = app.store.compositor.snapshot.focused_output_name.clone();
    refusal(app.brightness_step(&output, args[0].int().into()))
}

fn hdr_toggle(app: &mut App, _: &[Value]) -> Reply {
    refusal(app.hdr.toggle(&app.store))
}

fn hdr_enable(app: &mut App, _: &[Value]) -> Reply {
    refusal(app.hdr.set_all(&app.store, true))
}

fn hdr_disable(app: &mut App, _: &[Value]) -> Reply {
    refusal(app.hdr.set_all(&app.store, false))
}

fn hdr_set_output(app: &mut App, args: &[Value]) -> Reply {
    refusal(app.hdr.set(&app.store, args[0].str(), args[1].bool()))
}

fn hdr_rule(app: &mut App, args: &[Value]) -> Reply {
    match display::hdr_rule(&app.store, args[0].str()) {
        Ok(rule) => Ok(rule),
        Err(why) => text(why),
    }
}

fn hdr_status(app: &mut App, _: &[Value]) -> Reply {
    Ok(display::hdr_status(&app.store))
}
