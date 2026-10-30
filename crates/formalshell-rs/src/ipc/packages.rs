//! `packages`: what the shell asked fspec of (`services/packages.rs`):
//! the last search answered, each install or uninstall it started with its
//! operation and the app it brought, and the owner answers by desktop id;
//! the Software card's spec check, and its Apply and Undo.

use serde_json::json;

use super::registry::{Function, Target, Type, Value};
use crate::services::packages::{self, Change, Cmd};
use crate::wayland::App;

fn status(app: &mut App, _: &[Value]) -> Value {
    let p = &app.store.packages;
    let search = p.search.as_ref().map(|(q, found)| match found {
        Ok(v) => json!({"query": q, "ids": v.iter().map(|f| f.id.as_str()).collect::<Vec<_>>()}),
        Err(why) => json!({"query": q, "error": why}),
    });
    let started: Vec<_> = p
        .started
        .iter()
        .map(|s| {
            let (op, error) = match &s.op {
                Ok(op) => (op.clone(), None),
                Err(why) => (None, Some(why.clone())),
            };
            json!({"id": s.id, "verb": s.verb.as_str(), "op": op, "error": error, "app": s.app})
        })
        .collect();
    let owners: serde_json::Map<String, serde_json::Value> = p.owners.iter().map(|(k, o)| (k.clone(), json!({"id": o.id, "removable": o.removable}))).collect();
    let sw = &p.software;
    let (behind, upstream, error) = match &sw.behind {
        None => (None, None, None),
        Some(Ok((n, up))) => (Some(*n), Some(up.clone()), None),
        Some(Err(why)) => (None, None, Some(why.clone())),
    };
    let last = sw.last.as_ref().map(|(verb, name)| json!({"verb": verb.as_str(), "name": name}));
    let software = json!({"checked": sw.behind.is_some(), "behind": behind, "upstream": upstream, "error": error, "last": last, "applying": sw.applying, "failed": sw.failed.as_ref().map(|(change, why)| json!({"change": format!("{change:?}").to_lowercase(), "why": why}))});
    let info: serde_json::Map<String, serde_json::Value> = p
        .info
        .iter()
        .map(|(id, i)| {
            let v = match i {
                Ok(i) => json!({"name": i.name, "license": i.license, "developer": i.developer, "download": i.download, "installed": i.installed, "screenshot": i.screenshot}),
                Err(why) => json!({"error": why}),
            };
            (id.clone(), v)
        })
        .collect();
    Value::Str(json!({"fspec": app.store.activity.fspec.installed, "search": search, "started": started, "owners": owners, "software": software, "info": info, "shots": p.shots}).to_string())
}

fn send(cmd: Cmd) -> Value {
    packages::send(cmd);
    Value::Str("ok".into())
}

fn check(_: &mut App, _: &[Value]) -> Value {
    send(Cmd::Check)
}

fn apply(_: &mut App, _: &[Value]) -> Value {
    send(Cmd::Change(Change::Apply))
}

fn undo(_: &mut App, _: &[Value]) -> Value {
    send(Cmd::Change(Change::Undo))
}

pub fn target() -> Target<App> {
    Target {
        name: "packages",
        functions: vec![
            Function { name: "status", params: &[], ret: Type::String, call: status },
            Function { name: "check", params: &[], ret: Type::String, call: check },
            Function { name: "apply", params: &[], ret: Type::String, call: apply },
            Function { name: "undo", params: &[], ret: Type::String, call: undo },
        ],
    }
}
