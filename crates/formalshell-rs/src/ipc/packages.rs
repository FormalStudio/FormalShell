//! `packages`: what the shell asked fspec of (`services/packages.rs`):
//! the last search answered, each install or uninstall it started with its
//! operation and the app it brought, and the owner answers by desktop id.

use serde_json::json;

use super::registry::{Function, Target, Type, Value};
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
    Value::Str(json!({"fspec": app.store.activity.fspec.installed, "search": search, "started": started, "owners": owners}).to_string())
}

pub fn target() -> Target<App> {
    Target { name: "packages", functions: vec![Function { name: "status", params: &[], ret: Type::String, call: status }] }
}
