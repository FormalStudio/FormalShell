//! `tests/ipc-golden.jsonl`, the fixed wire contract, replayed through the
//! client's parsing and the registry against the stub handlers below.

use serde::Deserialize;

use super::cli;
use super::registry::{Function, Registry, Target, Type, Value};
use super::wire::Request;

#[derive(Deserialize)]
struct Case {
    argv: Vec<String>,
    live: bool,
    stdout: String,
    stderr: String,
    exit: i32,
}

fn s(v: impl Into<String>) -> Value {
    Value::Str(v.into())
}

/// How a JS string concatenation prints a number.
fn js_number(v: f64) -> String {
    if v.is_nan() {
        "NaN".into()
    } else if v.is_infinite() {
        if v > 0.0 { "Infinity".into() } else { "-Infinity".into() }
    } else {
        v.to_string()
    }
}

const fn f(name: &'static str, params: &'static [(&'static str, Type)], ret: Type, call: fn(&mut (), &[Value]) -> Value) -> Function<()> {
    Function { name, params, ret, call }
}

const STR: (&str, Type) = ("a", Type::String);

/// The stub handlers the recordings were taken against, function for function.
fn stub() -> Registry<()> {
    Registry {
        targets: vec![
            Target {
                name: "debug",
                functions: vec![
                    f("dump", &[], Type::String, |_, _| s("{}")),
                    f("join", &[("edge", Type::String), ("x", Type::Int), ("width", Type::Int)], Type::String, |_, a| {
                        s(format!("join {} {} {}", a[0].str(), a[1].int(), a[2].int()))
                    }),
                    f("joinClear", &[], Type::String, |_, _| s("ok")),
                    f("motionScale", &[("percent", Type::Int)], Type::String, |_, a| s(format!("ms {}", a[0].int()))),
                    f("query", &[("q", Type::String)], Type::String, |_, a| {
                        s(serde_json::to_string(&[a[0].str()]).unwrap())
                    }),
                ],
            },
            Target {
                name: "theme",
                functions: vec![
                    f("retheme", &[], Type::String, |_, _| s("ok")),
                    f("mode", &[("m", Type::String)], Type::String, |_, a| s(format!("mode {}", a[0].str()))),
                    f("status", &[], Type::String, |_, _| s("{}")),
                ],
            },
            Target {
                name: "wallpaper",
                functions: vec![
                    f("set", &[("path", Type::String)], Type::String, |_, a| s(format!("set {}", a[0].str()))),
                    f("get", &[], Type::String, |_, _| s("")),
                ],
            },
            Target {
                name: "bar",
                functions: vec![
                    f("chevron", &[("action", Type::String)], Type::String, |_, a| s(format!("chevron {}", a[0].str()))),
                    f("chevronAt", &[("action", Type::String), ("region", Type::String)], Type::String, |_, a| {
                        s(format!("chevronAt {} {}", a[0].str(), a[1].str()))
                    }),
                    f("room", &[], Type::String, |_, _| s("[]")),
                    f("paint", &[], Type::String, |_, _| s("[]")),
                ],
            },
            Target {
                name: "panel",
                functions: vec![
                    f("open", &[("name", Type::String)], Type::String, |_, a| s(format!("open {}", a[0].str()))),
                    f("close", &[], Type::String, |_, _| s("ok")),
                    f("toggle", &[("name", Type::String)], Type::String, |_, a| s(format!("toggle {}", a[0].str()))),
                    f("toggleAt", &[("n", Type::Int)], Type::String, |_, a| s(format!("toggleAt {}", a[0].int()))),
                    f("state", &[], Type::String, |_, _| s("")),
                ],
            },
            Target {
                name: "media",
                functions: vec![
                    f("playPause", &[], Type::String, |_, _| s("playPause")),
                    f("next", &[], Type::String, |_, _| s("next")),
                    f("previous", &[], Type::String, |_, _| s("previous")),
                    f("shuffle", &[("mode", Type::String)], Type::String, |_, a| s(format!("shuffle {}", a[0].str()))),
                    f("loop", &[("mode", Type::String)], Type::String, |_, a| s(format!("loop {}", a[0].str()))),
                    f("volume", &[("percent", Type::Int)], Type::String, |_, a| s(format!("volume {}", a[0].int()))),
                    f("raise", &[], Type::String, |_, _| s("raise")),
                    f("select", &[("id", Type::String)], Type::String, |_, a| s(format!("select {}", a[0].str()))),
                    f("output", &[("name", Type::String)], Type::String, |_, a| s(format!("output {}", a[0].str()))),
                    f("outputs", &[], Type::String, |_, _| s("[]")),
                    f("players", &[], Type::String, |_, _| s("[]")),
                    f("status", &[], Type::String, |_, _| s("{}")),
                    f("lyrics", &[], Type::String, |_, _| s("{}")),
                ],
            },
            Target {
                name: "workspaces",
                functions: vec![
                    f("peek", &[("n", Type::Int)], Type::String, |_, a| s(format!("peek {}", a[0].int()))),
                    f("close", &[], Type::String, |_, _| s("ok")),
                    f("status", &[], Type::String, |_, _| s("{}")),
                ],
            },
            Target {
                name: "tray",
                functions: vec![
                    f("status", &[], Type::String, |_, _| s("{}")),
                    f("activate", &[("id", Type::String)], Type::String, |_, a| s(format!("activate {}", a[0].str()))),
                    f("menu", &[("id", Type::String)], Type::String, |_, a| s(format!("menu {}", a[0].str()))),
                    f("menucursor", &[("delta", Type::String)], Type::String, |_, a| s(format!("menucursor {}", a[0].str()))),
                    f("menuactivate", &[], Type::String, |_, _| s("ok")),
                ],
            },
            Target {
                name: "overnight",
                functions: vec![
                    f("toggle", &[], Type::String, |_, _| s("ok")),
                    f("enable", &[], Type::String, |_, _| s("ok")),
                    f("disable", &[], Type::String, |_, _| s("ok")),
                    f("status", &[], Type::String, |_, _| s("{}")),
                ],
            },
            Target {
                name: "earbuds",
                functions: vec![
                    f("status", &[], Type::String, |_, _| s("{}")),
                    f("devices", &[], Type::String, |_, _| s("[]")),
                    f("select", &[("key", Type::String)], Type::String, |_, a| s(format!("select {}", a[0].str()))),
                    f("set", &[("control", Type::String), ("value", Type::String)], Type::String, |_, a| {
                        s(format!("set {} {}", a[0].str(), a[1].str()))
                    }),
                ],
            },
            Target {
                name: "radio",
                functions: vec![
                    f("status", &[], Type::String, |_, _| s("{}")),
                    f("play", &[("id", Type::String)], Type::String, |_, a| s(format!("play {}", a[0].str()))),
                    f("toggle", &[], Type::String, |_, _| s("toggle")),
                    f("random", &[], Type::String, |_, _| s("random")),
                    f("stop", &[], Type::String, |_, _| s("stop")),
                ],
            },
            Target { name: "airplay", functions: vec![f("status", &[], Type::String, |_, _| s("{}"))] },
            Target {
                name: "visualizer",
                functions: vec![
                    f("style", &[("name", Type::String)], Type::String, |_, a| s(format!("style {}", a[0].str()))),
                    f("styles", &[], Type::String, |_, _| s("bars\nline")),
                    f("status", &[], Type::String, |_, _| s("{}")),
                ],
            },
            Target {
                name: "bluetooth",
                functions: vec![
                    f("toggle", &[], Type::String, |_, _| s("ok")),
                    f("power", &[("state", Type::String)], Type::String, |_, a| s(format!("power {}", a[0].str()))),
                    f("trust", &[("address", Type::String)], Type::String, |_, a| s(format!("trust {}", a[0].str()))),
                    f("untrust", &[("address", Type::String)], Type::String, |_, a| s(format!("untrust {}", a[0].str()))),
                    f("status", &[], Type::String, |_, _| s("{}")),
                ],
            },
            Target {
                name: "monitor",
                functions: vec![
                    f("status", &[], Type::String, |_, _| s("{}")),
                    f("processes", &[("query", Type::String)], Type::String, |_, a| s(format!("processes {}", a[0].str()))),
                    f("kill", &[("pid", Type::String), ("signal", Type::String)], Type::String, |_, a| {
                        s(format!("kill {} {}", a[0].str(), a[1].str()))
                    }),
                    f("restart", &[("pid", Type::String)], Type::String, |_, a| s(format!("restart {}", a[0].str()))),
                    f("gpu", &[], Type::String, |_, _| s("{}")),
                    f("launch", &[("desktopId", Type::String), ("card", Type::String)], Type::String, |_, a| {
                        s(format!("launch {} {}", a[0].str(), a[1].str()))
                    }),
                ],
            },
            Target {
                name: "network",
                functions: vec![
                    f("status", &[], Type::String, |_, _| s("{}")),
                    f("connect", &[("ssid", Type::String), ("psk", Type::String)], Type::String, |_, a| {
                        s(format!("connect {} {}", a[0].str(), a[1].str()))
                    }),
                    f(
                        "connectEap",
                        &[("ssid", Type::String), ("identity", Type::String), ("password", Type::String)],
                        Type::String,
                        |_, a| s(format!("connectEap {} {} {}", a[0].str(), a[1].str(), a[2].str())),
                    ),
                    f("forget", &[("ssid", Type::String)], Type::String, |_, a| s(format!("forget {}", a[0].str()))),
                    f("wifi", &[("enabled", Type::Bool)], Type::String, |_, a| s(format!("wifi {}", a[0].bool()))),
                    f("speedtest", &[], Type::String, |_, _| s("ok")),
                    f("speedstatus", &[], Type::String, |_, _| s("{}")),
                ],
            },
            Target {
                name: "plugins",
                functions: vec![
                    f("list", &[], Type::String, |_, _| s("[]")),
                    f("status", &[], Type::String, |_, _| s("{}")),
                    f("reload", &[], Type::String, |_, _| s("ok")),
                ],
            },
            Target {
                name: "caffeinate",
                functions: vec![
                    f("toggle", &[], Type::String, |_, _| s("ok")),
                    f("enable", &[], Type::String, |_, _| s("ok")),
                    f("disable", &[], Type::String, |_, _| s("ok")),
                    f("status", &[], Type::String, |_, _| s("{}")),
                ],
            },
            Target {
                name: "lock",
                functions: vec![
                    f("lock", &[], Type::String, |_, _| s("ok")),
                    f("isLocked", &[], Type::String, |_, _| s("false")),
                    f("status", &[], Type::String, |_, _| s("{}")),
                ],
            },
            Target {
                name: "switcher",
                functions: vec![
                    f("next", &[], Type::String, |_, _| s("ok")),
                    f("prev", &[], Type::String, |_, _| s("ok")),
                    f("commit", &[], Type::String, |_, _| s("ok")),
                    f("cancel", &[], Type::String, |_, _| s("ok")),
                    f("state", &[], Type::String, |_, _| s("{}")),
                ],
            },
            Target {
                name: "gallery",
                functions: vec![
                    f("open", &[], Type::String, |_, _| s("ok")),
                    f("close", &[], Type::String, |_, _| s("ok")),
                    f("toggle", &[], Type::String, |_, _| s("ok")),
                    f("status", &[], Type::String, |_, _| s("{\"isOpen\":false}")),
                ],
            },
            Target {
                name: "menu",
                functions: vec![
                    f("toggle", &[], Type::String, |_, _| s("ok")),
                    f("summon", &[("route", Type::String)], Type::String, |_, a| s(format!("summon {}", a[0].str()))),
                    f("activate", &[("index", Type::Int)], Type::String, |_, a| s(format!("activate {}", a[0].int()))),
                    f("activateAlternate", &[("index", Type::Int)], Type::String, |_, a| s(format!("activateAlternate {}", a[0].int()))),
                    f("filter", &[("text", Type::String)], Type::String, |_, a| s(format!("filter {}", a[0].str()))),
                    f("close", &[], Type::String, |_, _| s("ok")),
                    f("refresh", &[], Type::String, |_, _| s("ok")),
                    f("ping", &[], Type::String, |_, _| s("pong")),
                    f("status", &[], Type::String, |_, _| s("{}")),
                    f(
                        "select",
                        &[("prompt", Type::String), ("optionsJson", Type::String), ("token", Type::String)],
                        Type::String,
                        |_, a| s(format!("select {} {} {}", a[0].str(), a[1].str(), a[2].str())),
                    ),
                    f("input", &[("prompt", Type::String), ("token", Type::String)], Type::String, |_, a| {
                        s(format!("input {} {}", a[0].str(), a[1].str()))
                    }),
                ],
            },
            Target {
                name: "calendar",
                functions: vec![
                    f("select", &[("date", Type::String)], Type::String, |_, a| s(format!("select {}", a[0].str()))),
                    f("status", &[], Type::String, |_, _| s("{}")),
                ],
            },
            Target {
                name: "iphone",
                functions: vec![
                    f("status", &[], Type::String, |_, _| s("{}")),
                    f("pair", &[], Type::String, |_, _| s("ok")),
                    f("invoke", &[("id", Type::String), ("action", Type::String)], Type::String, |_, a| {
                        s(format!("invoke {} {}", a[0].str(), a[1].str()))
                    }),
                    f("dismiss", &[("id", Type::String)], Type::String, |_, a| s(format!("dismiss {}", a[0].str()))),
                    f("clear", &[], Type::String, |_, _| s("ok")),
                    f("markRead", &[], Type::String, |_, _| s("ok")),
                ],
            },
            Target {
                name: "screenshot",
                functions: vec![
                    f("full", &[("processing", Type::String)], Type::String, |_, a| s(format!("full {}", a[0].str()))),
                    f("region", &[("processing", Type::String)], Type::String, |_, a| s(format!("region {}", a[0].str()))),
                    f("cancel", &[], Type::String, |_, _| s("ok")),
                    f("pick", &[("mode", Type::String), ("processing", Type::String)], Type::String, |_, a| {
                        s(format!("pick {} {}", a[0].str(), a[1].str()))
                    }),
                    f("key", &[("name", Type::String)], Type::String, |_, a| s(format!("key {}", a[0].str()))),
                    f("pickerStatus", &[], Type::String, |_, _| s("{}")),
                    f("edit", &[("path", Type::String)], Type::String, |_, a| s(format!("edit {}", a[0].str()))),
                    f("status", &[], Type::String, |_, _| s("{}")),
                ],
            },
            Target {
                name: "display",
                functions: vec![
                    f("scale", &[("output", Type::String), ("scale", Type::Real)], Type::String, |_, a| {
                        s(format!("scale {} {}", a[0].str(), js_number(a[1].real())))
                    }),
                    f("mirror", &[("output", Type::String), ("source", Type::String)], Type::String, |_, a| {
                        s(format!("mirror {} {}", a[0].str(), a[1].str()))
                    }),
                    f("enable", &[("output", Type::String), ("enabled", Type::Bool)], Type::String, |_, a| {
                        s(format!("enable {} {}", a[0].str(), a[1].bool()))
                    }),
                    f("brightnessStep", &[("delta", Type::Int)], Type::String, |_, a| s(format!("brightnessStep {}", a[0].int()))),
                ],
            },
            Target {
                name: "hdr",
                functions: vec![
                    f("toggle", &[], Type::String, |_, _| s("ok")),
                    f("enable", &[], Type::String, |_, _| s("ok")),
                    f("disable", &[], Type::String, |_, _| s("ok")),
                    f("setOutput", &[("output", Type::String), ("enabled", Type::Bool)], Type::String, |_, a| {
                        s(format!("setOutput {} {}", a[0].str(), a[1].bool()))
                    }),
                    f("rule", &[("output", Type::String)], Type::String, |_, a| s(format!("rule {}", a[0].str()))),
                    f("status", &[], Type::String, |_, _| s("{}")),
                ],
            },
            Target {
                name: "capture",
                functions: vec![
                    f("text", &[], Type::String, |_, _| s("ok")),
                    f("color", &[], Type::String, |_, _| s("ok")),
                    f("textAt", &[("geometry", Type::String)], Type::String, |_, a| s(format!("textAt {}", a[0].str()))),
                    f("colorAt", &[("geometry", Type::String)], Type::String, |_, a| s(format!("colorAt {}", a[0].str()))),
                    f("cancel", &[], Type::String, |_, _| s("ok")),
                    f("status", &[], Type::String, |_, _| s("{}")),
                ],
            },
            Target {
                name: "record",
                functions: vec![
                    f("start", &[("scope", Type::String), ("audio", Type::String)], Type::String, |_, a| {
                        s(format!("start {} {}", a[0].str(), a[1].str()))
                    }),
                    f("startCapped", &[("scope", Type::String), ("audio", Type::String), ("maxHeight", Type::String)], Type::String, |_, a| {
                        s(format!("startCapped {} {} {}", a[0].str(), a[1].str(), a[2].str()))
                    }),
                    f("startAt", &[("geometry", Type::String), ("audio", Type::String)], Type::String, |_, a| {
                        s(format!("startAt {} {}", a[0].str(), a[1].str()))
                    }),
                    f("stop", &[], Type::String, |_, _| s("ok")),
                    f("toggle", &[("scope", Type::String), ("audio", Type::String)], Type::String, |_, a| {
                        s(format!("toggle {} {}", a[0].str(), a[1].str()))
                    }),
                    f("gif", &[("path", Type::String)], Type::String, |_, a| s(format!("gif {}", a[0].str()))),
                    f("status", &[], Type::String, |_, _| s("{}")),
                ],
            },
            Target {
                name: "notifications",
                functions: vec![
                    f("status", &[], Type::String, |_, _| s("{}")),
                    f("dndState", &[], Type::String, |_, _| s("off")),
                    f("toggleDnd", &[], Type::String, |_, _| s("on")),
                    f("setDnd", &[("on", Type::Bool)], Type::String, |_, a| s(if a[0].bool() { "on" } else { "off" })),
                    f("showHistory", &[], Type::String, |_, _| s("ok")),
                    f("clear", &[], Type::String, |_, _| s("ok")),
                    f("clearPending", &[], Type::String, |_, _| s("ok")),
                    f("markAllSeen", &[], Type::String, |_, _| s("ok")),
                    f("dismissAll", &[], Type::String, |_, _| s("ok")),
                    f("dismissOne", &[], Type::String, |_, _| s("none")),
                    f("invokeLast", &[], Type::String, |_, _| s("ok")),
                    f("expand", &[("state", Type::String)], Type::String, |_, a| s(format!("expand {}", a[0].str()))),
                ],
            },
            Target {
                name: "reminder",
                functions: vec![
                    f("set", &[("duration", Type::String), ("message", Type::String)], Type::String, |_, a| {
                        s(format!("set {} {}", a[0].str(), a[1].str()))
                    }),
                    f("show", &[], Type::String, |_, _| s("ok")),
                    f("clear", &[], Type::String, |_, _| s("ok: cleared 0")),
                    f("status", &[], Type::String, |_, _| s("{}")),
                ],
            },
            Target {
                name: "activity",
                functions: vec![
                    f("status", &[], Type::String, |_, _| s("{}")),
                    f("seen", &[], Type::String, |_, _| s("ok")),
                    f("open", &[("source", Type::String)], Type::String, |_, a| s(format!("error: no activity from '{}'", a[0].str()))),
                    f("dismiss", &[("source", Type::String)], Type::String, |_, a| s(format!("error: no activity from '{}'", a[0].str()))),
                    f("retry", &[("source", Type::String)], Type::String, |_, a| s(format!("error: no activity from '{}'", a[0].str()))),
                ],
            },
            Target {
                name: "mirror",
                functions: vec![
                    f("toggle", &[], Type::String, |_, _| s("ok")),
                    f("open", &[], Type::String, |_, _| s("ok")),
                    f("close", &[], Type::String, |_, _| s("ok")),
                    f("next", &[], Type::String, |_, _| s("ok")),
                    f("previous", &[], Type::String, |_, _| s("ok")),
                    f("status", &[], Type::String, |_, _| s("{}")),
                ],
            },
            Target {
                name: "localsend",
                functions: vec![
                    f("status", &[], Type::String, |_, _| s("{}")),
                    f("peers", &[], Type::String, |_, _| s("[]")),
                    f("scan", &[], Type::String, |_, _| s("ok")),
                    f("send", &[("peer", Type::String), ("path", Type::String)], Type::String, |_, a| {
                        s(format!("send {} {}", a[0].str(), a[1].str()))
                    }),
                ],
            },
            Target {
                name: "picker",
                functions: vec![
                    f("summon", &[], Type::String, |_, _| s("ok")),
                    f("select", &[("directory", Type::String), ("token", Type::String)], Type::String, |_, a| {
                        s(format!("select {} {}", a[0].str(), a[1].str()))
                    }),
                    f("choose", &[("path", Type::String)], Type::String, |_, a| s(format!("choose {}", a[0].str()))),
                    f("variant", &[("name", Type::String)], Type::String, |_, a| s(format!("variant {}", a[0].str()))),
                    f("close", &[], Type::String, |_, _| s("ok")),
                    f("status", &[], Type::String, |_, _| s("{}")),
                ],
            },
            Target {
                name: "clipboard",
                functions: vec![
                    f("list", &[], Type::String, |_, _| s("[]")),
                    f("copy", &[("id", Type::String)], Type::String, |_, _| s("ok")),
                    f("remove", &[("id", Type::String)], Type::String, |_, _| s("ok")),
                    f("clear", &[], Type::String, |_, _| s("ok")),
                ],
            },
            Target {
                name: "osd",
                functions: vec![
                    f("volume", &[], Type::String, |_, _| s("ok")),
                    f("brightness", &[], Type::String, |_, _| s("ok")),
                    f("media", &[("text", Type::String)], Type::String, |_, a| s(format!("media {}", a[0].str()))),
                    f("close", &[], Type::String, |_, _| s("ok")),
                    f("state", &[], Type::String, |_, _| s("{}")),
                ],
            },
            Target {
                name: "nightlight",
                functions: vec![
                    f("toggle", &[], Type::String, |_, _| s("ok")),
                    f("enable", &[], Type::String, |_, _| s("ok")),
                    f("disable", &[], Type::String, |_, _| s("ok")),
                    f("status", &[], Type::String, |_, _| s("{}")),
                ],
            },
            Target {
                name: "console",
                functions: vec![
                    f("toggle", &[], Type::String, |_, _| s("ok")),
                    f("show", &[], Type::String, |_, _| s("ok")),
                    f("hide", &[], Type::String, |_, _| s("ok")),
                    f("status", &[], Type::String, |_, _| s("{}")),
                ],
            },
            Target {
                name: "lights",
                functions: vec![
                    f("toggle", &[], Type::String, |_, _| s("ok")),
                    f("effect", &[("id", Type::String)], Type::String, |_, a| s(format!("effect {}", a[0].str()))),
                    f("color", &[("hex", Type::String)], Type::String, |_, a| s(format!("color {}", a[0].str()))),
                    f("source", &[("value", Type::String)], Type::String, |_, a| s(format!("source {}", a[0].str()))),
                    f("speed", &[("value", Type::String)], Type::String, |_, a| s(format!("speed {}", a[0].str()))),
                    f("brightness", &[("level", Type::String)], Type::String, |_, a| s(format!("brightness {}", a[0].str()))),
                    f("refresh", &[], Type::String, |_, _| s("ok")),
                    f("status", &[], Type::String, |_, _| s("{}")),
                ],
            },
            Target {
                name: "screensaver",
                functions: vec![
                    f("start", &[], Type::String, |_, _| s("ok")),
                    f("stop", &[], Type::String, |_, _| s("ok")),
                    f("status", &[], Type::String, |_, _| s("{}")),
                    f("frame", &[("n", Type::Int)], Type::String, |_, a| s(format!("frame {}", a[0].int()))),
                    f("frameInfo", &[], Type::String, |_, _| s("{}")),
                ],
            },
            Target {
                name: "probe",
                functions: vec![
                    f("s", &[STR], Type::String, |_, a| s(format!("[{}]", a[0].str()))),
                    f("ss", &[STR, ("b", Type::String)], Type::String, |_, a| {
                        s(format!("[{}][{}]", a[0].str(), a[1].str()))
                    }),
                    f("sss", &[STR, ("b", Type::String), ("c", Type::String)], Type::String, |_, a| {
                        s(format!("[{}][{}][{}]", a[0].str(), a[1].str(), a[2].str()))
                    }),
                    f("i", &[("a", Type::Int)], Type::String, |_, a| s(a[0].int().to_string())),
                    f("b", &[("a", Type::Bool)], Type::String, |_, a| s(a[0].bool().to_string())),
                    f("r", &[("a", Type::Real)], Type::String, |_, a| s(js_number(if let Value::Real(r) = a[0] { r } else { 0.0 }))),
                    f("ri", &[("a", Type::Real)], Type::Real, |_, a| a[0].clone()),
                    f("ii", &[("a", Type::Int)], Type::Int, |_, a| a[0].clone()),
                    f("bb", &[("a", Type::Bool)], Type::Bool, |_, a| a[0].clone()),
                    f("v", &[], Type::Void, |_, _| Value::Void),
                    f("empty", &[], Type::String, |_, _| s("")),
                    f("nl", &[], Type::String, |_, _| s("a\nb\n")),
                    f("show", &[], Type::String, |_, _| s("probe-show")),
                    f("call", &[], Type::String, |_, _| s("probe-call")),
                ],
            },
        ],
    }
}

/// stdout, stderr and the exit code, as formalshell-ipc would leave them.
fn run(registry: &Registry<()>, argv: &[String], live: bool) -> (String, String, i32, bool) {
    let request = match cli::parse(argv) {
        Ok(request) => request,
        Err(err) => return (String::new(), err.stderr(), err.exit, false),
    };
    if !live {
        return ("No running instances for \"\"\n".into(), String::new(), 255, false);
    }
    let listing = matches!(request, Request::Show { .. });
    if let Some(message) = cli::missing(&request) {
        return (message.into(), String::new(), 0, listing);
    }
    (registry.answer(&mut (), &request), String::new(), 0, listing)
}

/// The quoted path is the instance's own, so only the words around it are
/// held.
fn no_instance(text: &str) -> bool {
    text.starts_with("No running instances for \"") && text.ends_with("\"\n")
}

/// The recorded listing comes out of a hash, so only the set holds.
/// Each recording saw the targets of its own branch, so every target block
/// it lists must appear in ours with the same functions; ours may list more.
fn blocks(text: &str) -> std::collections::BTreeMap<&str, Vec<&str>> {
    let mut out = std::collections::BTreeMap::new();
    let mut current: Option<&str> = None;
    for line in text.lines() {
        if let Some(name) = line.strip_prefix("target ") {
            current = Some(name);
            out.entry(name).or_insert_with(Vec::new);
        } else if let Some(name) = current {
            out.get_mut(name).expect("opened above").push(line);
        }
    }
    for functions in out.values_mut() {
        functions.sort();
    }
    out
}

#[test]
fn matches_qs_ipc() {
    let registry = stub();
    let cases: Vec<Case> = include_str!("../../tests/ipc-golden.jsonl")
        .lines()
        .map(|line| serde_json::from_str(line).expect("a golden line"))
        .collect();
    assert!(cases.len() > 100, "only {} golden cases", cases.len());
    for case in &cases {
        let (stdout, stderr, exit, listing) = run(&registry, &case.argv, case.live);
        let what = format!("{:?} live={}", case.argv, case.live);
        assert_eq!(exit, case.exit, "exit code of {what}");
        assert_eq!(stderr, case.stderr, "stderr of {what}");
        if !case.live {
            assert!(no_instance(&stdout) && no_instance(&case.stdout), "stdout of {what}: {stdout:?}");
        } else if listing {
            let ours = blocks(&stdout);
            // A recording made on an older branch knows fewer functions of a
            // target; every one it lists must still be served, signature intact.
            for (name, functions) in blocks(&case.stdout) {
                let served = ours.get(name).unwrap_or_else(|| panic!("target {name} in stdout of {what}"));
                for f in functions {
                    assert!(served.contains(&f), "{f:?} of target {name} in stdout of {what}");
                }
            }
        } else {
            assert_eq!(stdout, case.stdout, "stdout of {what}");
        }
    }
}

/// Each real target declares every function of its stub target exactly as the
/// stub above does.
#[test]
fn signatures_match_golden() {
    let stub = stub();
    for real in [
        super::debug::target(),
        super::theme::target(),
        super::theme::wallpaper(),
        super::bar::target(),
        super::panel::target(),
        super::media::target(),
        super::tray::target(),
        super::overnight::target(),
        super::earbuds::target(),
        super::workspaces::target(),
        super::radio::target(),
        super::airplay::target(),
        super::visualizer::target(),
        super::monitor::target(),
        super::network::target(),
        super::bluetooth::target(),
        super::plugins::target(),
        super::caffeinate::target(),
        super::gallery::target(),
        super::menu::target(),
        super::calendar::target(),
        super::iphone::target(),
        super::screenshot::target(),
        super::capture::target(),
        super::record::target(),
        super::display::target(),
        super::display::hdr(),
        super::notifications::target(),
        super::notifications::reminder(),
        super::activity::target(),
        super::lock::target(),
        super::osd::target(),
        super::nightlight::target(),
        super::lights::target(),
        super::switcher::target(),
        super::console::target(),
        super::screensaver::target(),
        super::clipboard::target(),
        super::picker::target(),
        super::localsend::target(),
        super::mirror::target(),
    ] {
        let defs: Vec<String> = real.functions.iter().map(|f| f.definition()).collect();
        let golden = stub.targets.iter().find(|t| t.name == real.name).expect("a stub target");
        for function in &golden.functions {
            assert!(defs.contains(&function.definition()), "{} lacks {}", real.name, function.definition());
        }
    }
}
