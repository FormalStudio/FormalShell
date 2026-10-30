//! Resolver for the drop-in plugin directory
//! (`~/.config/formalshell/plugins/<id>/manifest.json`). One scan payload in,
//! resolved plugin records out.
//!
//! Manifest schema, exactly ten legal keys:
//!   apiVersion  number, required, must be [`API_VERSION`]
//!   id          string, required, must equal the plugin's directory name and
//!               match `^[a-z0-9][a-z0-9-]*$` (it is what `bar.layout` writes
//!               as `plugin:<id>` and what `panel open plugin:<id>` addresses)
//!   kind        string, required, one of [`Kind`]
//!   entry       string, required, a path inside the plugin directory
//!   name        string, optional, defaults to id
//!   region      string, kind "bar" only, default "right"
//!   keepLoaded  bool, kind "panel"/"overlay" only, default false
//!   width       string, kind "panel" only, one of [`Width`], default "default"
//!   mode        string, kind "launcher" only: the launcher mode's name, no
//!               mode when absent
//!   subjects    array of strings, kind "launcher" only: the subject kinds
//!               (`file`, `app`, ...) the plugin adds actions to, default []
//!
//! Failure contract: absent or malformed input is never fatal. Each problem
//! pushes one warning string and drops the smallest unit that fixes it. A
//! manifest that cannot be addressed at all (unparsable, missing a required
//! key, wrong apiVersion, id/dirname mismatch, unknown kind, escaping entry
//! path) drops the whole plugin; anything else drops one key back to its
//! default and keeps the plugin. An id listed in `plugins.disabled` is
//! skipped silently.
//!
//! A missing entry file is not detectable here, this module never touches the
//! filesystem. The "not valid JSON" warning carries serde_json's
//! message, and a manifest key named after an `Object.prototype` member
//! (`constructor`, `toString`) is an ordinary unknown key.

use std::sync::LazyLock;

use regex::Regex;
use serde::de::{self, Deserialize, Deserializer, IgnoredAny, MapAccess, SeqAccess, Visitor};
use serde_json::Value;

use fs_js as js;
use crate::types::Region;

pub const API_VERSION: i64 = 1;

/// Must stay byte-identical to the `plugin:` prefix `bar::layout` writes.
pub const PLUGIN_PREFIX: &str = "plugin:";

pub const RECORD_BOUNDARY: &str = "#--formalshell-plugin-boundary--";

/// One `sh` process enumerates the directory and reads every manifest, the
/// same drop-in-directory read the matugen.d scan performs. Per-plugin
/// structure is recovered from [`RECORD_BOUNDARY`]. A plugins directory that
/// does not exist leaves the glob unmatched, `[ -f ]` fails, nothing prints.
pub const SCAN_SCRIPT: &str = r#"for m in "$1"/*/manifest.json; do [ -f "$m" ] && echo "$2" && echo "${m%/manifest.json}" && cat "$m" && echo; done"#;

static ID_PATTERN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[a-z0-9][a-z0-9-]*$").expect("static pattern"));

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Bar,
    Panel,
    Overlay,
    Service,
    Launcher,
}

impl Kind {
    pub fn parse(s: &str) -> Option<Kind> {
        match s {
            "bar" => Some(Kind::Bar),
            "panel" => Some(Kind::Panel),
            "overlay" => Some(Kind::Overlay),
            "service" => Some(Kind::Service),
            "launcher" => Some(Kind::Launcher),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Bar => "bar",
            Kind::Panel => "panel",
            Kind::Overlay => "overlay",
            Kind::Service => "service",
            Kind::Launcher => "launcher",
        }
    }
}

/// One of the four popupWidth tokens a floating card snaps to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Width {
    Narrow,
    Default,
    Wide,
    Menu,
}

impl Width {
    pub fn parse(s: &str) -> Option<Width> {
        match s {
            "narrow" => Some(Width::Narrow),
            "default" => Some(Width::Default),
            "wide" => Some(Width::Wide),
            "menu" => Some(Width::Menu),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Width::Narrow => "narrow",
            Width::Default => "default",
            Width::Wide => "wide",
            Width::Menu => "menu",
        }
    }
}

/// A resolved plugin. Keys with no meaning for the kind are `None`, which is
/// not the same answer as `false` or a default.
#[derive(Clone, Debug, PartialEq)]
pub struct Plugin {
    pub id: String,
    pub kind: Kind,
    pub entry: String,
    pub dir: String,
    pub name: String,
    pub region: Option<Region>,
    pub keep_loaded: Option<bool>,
    pub width: Option<Width>,
    pub mode: Option<String>,
    pub subjects: Option<Vec<String>>,
    pub entry_url: String,
}

/// One plugin as it arrives off the scan: its directory, the id derived from
/// it and the manifest bytes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Record {
    pub dir: String,
    pub id: String,
    pub text: String,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Resolved {
    /// Id-sorted, so auto-appending into a bar region is deterministic.
    pub plugins: Vec<Plugin>,
    pub warnings: Vec<String>,
}

impl Resolved {
    pub fn by_id(&self, id: &str) -> Option<&Plugin> {
        self.plugins.iter().find(|p| p.id == id)
    }
}

/// argv for the scan process. The directory only ever arrives as `$1`, never
/// interpolated into the script text.
pub fn scan_command(plugins_dir: &str) -> Vec<String> {
    ["sh", "-c", SCAN_SCRIPT, "sh", plugins_dir, RECORD_BOUNDARY]
        .map(String::from)
        .into()
}

/// Splits one collected payload into per-plugin records. Each record on the
/// wire is boundary, newline, dir, newline, manifest bytes.
pub fn split_scan(text: &str) -> Vec<Record> {
    let mut records = Vec::new();
    for chunk in text.split(RECORD_BOUNDARY).skip(1) {
        let chunk = chunk.trim_start_matches('\n');
        let Some(nl) = chunk.find('\n') else { continue };
        let dir = &chunk[..nl];
        if dir.is_empty() {
            continue;
        }
        let id = dir.rsplit_once('/').map_or(dir, |(_, last)| last);
        records.push(Record {
            dir: dir.into(),
            id: id.into(),
            text: chunk[nl + 1..].into(),
        });
    }
    records
}

/// The one place a plugin URL is built.
pub fn entry_url(plugin: &Plugin) -> String {
    format!("file://{}/{}", plugin.dir, plugin.entry)
}

/// The name both `bar.layout` and the panel registry address a plugin by.
pub fn surface_key(plugin: &Plugin) -> String {
    format!("{PLUGIN_PREFIX}{}", plugin.id)
}

/// A manifest's top level with its keys in the order JS would enumerate them.
enum Manifest {
    Object(Vec<(String, Value)>),
    Other,
}

impl<'de> Deserialize<'de> for Manifest {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = Manifest;

            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("any JSON value")
            }

            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Manifest, A::Error> {
                let mut entries: Vec<(String, Value)> = Vec::new();
                while let Some((k, v)) = map.next_entry::<String, Value>()? {
                    // A repeated key keeps its first place and takes the last value.
                    match entries.iter_mut().find(|(have, _)| *have == k) {
                        Some(slot) => slot.1 = v,
                        None => entries.push((k, v)),
                    }
                }
                Ok(Manifest::Object(js_key_order(entries)))
            }

            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Manifest, A::Error> {
                while seq.next_element::<IgnoredAny>()?.is_some() {}
                Ok(Manifest::Other)
            }

            fn visit_bool<E: de::Error>(self, _: bool) -> Result<Manifest, E> {
                Ok(Manifest::Other)
            }

            fn visit_i64<E: de::Error>(self, _: i64) -> Result<Manifest, E> {
                Ok(Manifest::Other)
            }

            fn visit_u64<E: de::Error>(self, _: u64) -> Result<Manifest, E> {
                Ok(Manifest::Other)
            }

            fn visit_f64<E: de::Error>(self, _: f64) -> Result<Manifest, E> {
                Ok(Manifest::Other)
            }

            fn visit_str<E: de::Error>(self, _: &str) -> Result<Manifest, E> {
                Ok(Manifest::Other)
            }

            fn visit_unit<E: de::Error>(self) -> Result<Manifest, E> {
                Ok(Manifest::Other)
            }
        }
        d.deserialize_any(V)
    }
}

/// Integer-like keys first in ascending order, the rest as written: the order
/// `for (key in object)` walks.
fn js_key_order(entries: Vec<(String, Value)>) -> Vec<(String, Value)> {
    fn index(k: &str) -> Option<u32> {
        let n: u32 = k.parse().ok()?;
        (n != u32::MAX && n.to_string() == k).then_some(n)
    }
    let (mut indexed, rest): (Vec<_>, Vec<_>) =
        entries.into_iter().partition(|(k, _)| index(k).is_some());
    indexed.sort_by_key(|(k, _)| index(k));
    indexed.extend(rest);
    indexed
}

fn is_str(v: Option<&Value>, want: &str) -> bool {
    v.and_then(Value::as_str) == Some(want)
}

/// One record in, one plugin (or none) plus its warnings out.
pub fn validate_record(record: &Record) -> (Option<Plugin>, Vec<String>) {
    let mut warnings = Vec::new();
    let prefix = format!("plugins/{}: ", record.id);
    let reject = |warnings: &mut Vec<String>, msg: String| {
        warnings.push(format!("{prefix}{msg}"));
        (None, std::mem::take(warnings))
    };

    let entries = match serde_json::from_str::<Manifest>(&record.text) {
        Err(e) => {
            return reject(
                &mut warnings,
                format!("manifest.json is not valid JSON: {e}"),
            );
        }
        Ok(Manifest::Other) => {
            return reject(&mut warnings, "manifest.json must be a JSON object".into());
        }
        Ok(Manifest::Object(entries)) => entries,
    };
    let get = |key: &str| entries.iter().find(|(k, _)| k == key).map(|(_, v)| v);

    for key in ["apiVersion", "id", "kind", "entry"] {
        if get(key).is_none() {
            return reject(
                &mut warnings,
                format!("manifest is missing required key \"{key}\""),
            );
        }
    }

    let api = get("apiVersion").expect("checked");
    if api.as_f64() != Some(API_VERSION as f64) {
        return reject(
            &mut warnings,
            format!(
                "apiVersion {} is not supported (this shell speaks {API_VERSION})",
                js::to_str(api)
            ),
        );
    }

    if !is_str(get("id"), &record.id) {
        return reject(
            &mut warnings,
            format!(
                "manifest id \"{}\" does not match its directory name",
                js::str_or_undefined(get("id"))
            ),
        );
    }

    if !ID_PATTERN.is_match(&record.id) {
        return reject(
            &mut warnings,
            format!(
                "id \"{}\" must be lowercase letters, digits and dashes",
                record.id
            ),
        );
    }

    let kind_value = get("kind").expect("checked");
    let Some(kind) = kind_value.as_str().and_then(Kind::parse) else {
        return reject(
            &mut warnings,
            format!("unknown kind \"{}\"", js::to_str(kind_value)),
        );
    };

    let entry_value = get("entry").expect("checked");
    let entry = match entry_value.as_str() {
        Some(e) if !e.is_empty() && !e.starts_with('/') && !format!("/{e}/").contains("/../") => e,
        _ => {
            return reject(
                &mut warnings,
                format!(
                    "entry \"{}\" must be a path inside the plugin directory",
                    js::to_str(entry_value)
                ),
            );
        }
    };

    for (key, _) in &entries {
        let allowed: &[Kind] = match key.as_str() {
            "apiVersion" | "id" | "kind" | "entry" => continue,
            "name" => &[Kind::Bar, Kind::Panel, Kind::Overlay, Kind::Service, Kind::Launcher],
            "region" => &[Kind::Bar],
            "keepLoaded" => &[Kind::Panel, Kind::Overlay],
            "width" => &[Kind::Panel],
            "mode" | "subjects" => &[Kind::Launcher],
            _ => {
                warnings.push(format!("{prefix}unknown manifest key \"{key}\""));
                continue;
            }
        };
        if !allowed.contains(&kind) {
            warnings.push(format!(
                "{prefix}\"{key}\" is not a valid key for kind \"{}\"",
                kind.as_str()
            ));
        }
    }

    let mut name = record.id.clone();
    if let Some(v) = get("name") {
        match v.as_str() {
            Some(s) if !s.is_empty() => name = s.into(),
            _ => warnings.push(format!(
                "{prefix}\"name\" must be a non-empty string, using \"{}\"",
                record.id
            )),
        }
    }

    let mut region = None;
    if kind == Kind::Bar {
        region = Some(Region::Right);
        if let Some(v) = get("region") {
            match v.as_str().and_then(Region::parse) {
                Some(r) => region = Some(r),
                None => warnings.push(format!(
                    "{prefix}unknown region \"{}\", using \"right\"",
                    js::to_str(v)
                )),
            }
        }
    }

    let mut keep_loaded = None;
    if matches!(kind, Kind::Panel | Kind::Overlay) {
        keep_loaded = Some(false);
        if let Some(v) = get("keepLoaded") {
            match v.as_bool() {
                Some(b) => keep_loaded = Some(b),
                None => warnings.push(format!(
                    "{prefix}\"keepLoaded\" must be true or false, using false"
                )),
            }
        }
    }

    let mut width = None;
    if kind == Kind::Panel {
        width = Some(Width::Default);
        if let Some(v) = get("width") {
            match v.as_str().and_then(Width::parse) {
                Some(w) => width = Some(w),
                None => warnings.push(format!(
                    "{prefix}unknown width \"{}\", using \"default\"",
                    js::to_str(v)
                )),
            }
        }
    }

    let mut mode = None;
    let mut subjects = None;
    if kind == Kind::Launcher {
        if let Some(v) = get("mode") {
            match v.as_str() {
                Some(s) if !s.is_empty() => mode = Some(s.to_owned()),
                _ => warnings.push(format!("{prefix}\"mode\" must be a non-empty string, the plugin gets no mode")),
            }
        }
        let mut kinds = Vec::new();
        if let Some(v) = get("subjects") {
            match v.as_array().filter(|a| a.iter().all(|k| k.as_str().is_some_and(|k| !k.is_empty()))) {
                Some(a) => kinds = a.iter().filter_map(Value::as_str).map(str::to_owned).collect(),
                None => warnings.push(format!("{prefix}\"subjects\" must be an array of subject kinds, using []")),
            }
        }
        if mode.is_none() && kinds.is_empty() {
            warnings.push(format!("{prefix}a launcher plugin with no \"mode\" and no \"subjects\" adds nothing"));
        }
        subjects = Some(kinds);
    }

    let mut plugin = Plugin {
        id: record.id.clone(),
        kind,
        entry: entry.into(),
        dir: record.dir.clone(),
        name,
        region,
        keep_loaded,
        width,
        mode,
        subjects,
        entry_url: String::new(),
    };
    plugin.entry_url = entry_url(&plugin);
    (Some(plugin), warnings)
}

/// The one function the plugin service calls.
pub fn resolve(text: Option<&str>, disabled: &[String]) -> Resolved {
    let mut plugins = Vec::new();
    let mut warnings = Vec::new();
    for record in split_scan(text.unwrap_or("")) {
        if disabled.contains(&record.id) {
            continue;
        }
        let (plugin, mut w) = validate_record(&record);
        warnings.append(&mut w);
        plugins.extend(plugin);
    }
    plugins.sort_by(|a, b| a.id.cmp(&b.id));
    Resolved { plugins, warnings }
}

pub fn bar_plugins(plugins: &[Plugin]) -> Vec<Plugin> {
    plugins
        .iter()
        .filter(|p| p.kind == Kind::Bar)
        .cloned()
        .collect()
}

pub fn surface_plugins(plugins: &[Plugin]) -> Vec<Plugin> {
    plugins
        .iter()
        .filter(|p| matches!(p.kind, Kind::Panel | Kind::Overlay))
        .cloned()
        .collect()
}

pub fn service_plugins(plugins: &[Plugin]) -> Vec<Plugin> {
    plugins
        .iter()
        .filter(|p| p.kind == Kind::Service)
        .cloned()
        .collect()
}

pub fn launcher_plugins(plugins: &[Plugin]) -> impl Iterator<Item = &Plugin> {
    plugins.iter().filter(|p| p.kind == Kind::Launcher)
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::*;

    const DIR: &str = "/home/u/.config/formalshell/plugins";

    /// Rebuilds exactly what SCAN_SCRIPT prints: boundary, plugin dir, then
    /// the manifest bytes, once per plugin.
    fn scan(records: &[(&str, String)]) -> String {
        records
            .iter()
            .map(|(id, text)| format!("{RECORD_BOUNDARY}\n{DIR}/{id}\n{text}\n"))
            .collect()
    }

    fn one(id: &str, manifest: Value) -> String {
        scan(&[(id, manifest.to_string())])
    }

    fn bar_manifest() -> Value {
        json!({ "apiVersion": 1, "id": "diskwatch", "kind": "bar", "entry": "DiskWatch.sh" })
    }

    fn run(text: &str) -> Resolved {
        resolve(Some(text), &[])
    }

    fn only_warning(r: &Resolved, needle: &str) {
        assert_eq!(r.plugins.len(), 0);
        assert_eq!(r.warnings.len(), 1, "{:?}", r.warnings);
        assert!(
            r.warnings[0].contains(needle),
            "{:?} lacks {needle}",
            r.warnings[0]
        );
    }

    #[test]
    fn empty_scan_output_is_zero_plugins_zero_warnings() {
        let r = resolve(Some(""), &[]);
        assert_eq!((r.plugins.len(), r.warnings.len()), (0, 0));
        let u = resolve(None, &[]);
        assert_eq!((u.plugins.len(), u.warnings.len()), (0, 0));
    }

    #[test]
    fn scan_command_argv_shape() {
        let argv = scan_command(DIR);
        assert_eq!(argv.len(), 6);
        assert_eq!(argv[0], "sh");
        assert_eq!(argv[1], "-c");
        assert_eq!(argv[2], SCAN_SCRIPT);
        assert_eq!(argv[3], "sh");
        assert_eq!(argv[4], DIR);
        assert_eq!(argv[5], RECORD_BOUNDARY);
        // The directory only ever arrives as $1: nothing caller-supplied is
        // interpolated into the script text.
        assert!(!SCAN_SCRIPT.contains(DIR));
    }

    #[test]
    fn split_scan_recovers_dir_and_body_per_record() {
        let text = scan(&[
            ("alpha", "{\"a\":1}".into()),
            // Boundary-adjacent text inside the payload must not split it.
            (
                "beta",
                "{\"note\":\"#--formalshell-plugin-boundary\"}".into(),
            ),
        ]);
        let records = split_scan(&text);
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].id, "alpha");
        assert_eq!(records[0].dir, format!("{DIR}/alpha"));
        let a: Value = serde_json::from_str(&records[0].text).unwrap();
        assert_eq!(a["a"], 1);
        assert_eq!(records[1].id, "beta");
        let b: Value = serde_json::from_str(&records[1].text).unwrap();
        assert_eq!(b["note"], "#--formalshell-plugin-boundary");
    }

    #[test]
    fn valid_bar_manifest_defaults() {
        let r = run(&one("diskwatch", bar_manifest()));
        assert_eq!(r.warnings.len(), 0);
        assert_eq!(r.plugins.len(), 1);
        let p = &r.plugins[0];
        assert_eq!(p.id, "diskwatch");
        assert_eq!(p.kind, Kind::Bar);
        assert_eq!(p.name, "diskwatch");
        assert_eq!(p.region, Some(Region::Right));
        assert_eq!(p.keep_loaded, None);
        assert_eq!(p.width, None);
        assert_eq!(r.by_id("diskwatch"), Some(p));
    }

    #[test]
    fn valid_panel_manifest_defaults() {
        let r = run(&one(
            "notes",
            json!({ "apiVersion": 1, "id": "notes", "kind": "panel", "entry": "Notes.sh" }),
        ));
        assert_eq!(r.warnings.len(), 0);
        let p = &r.plugins[0];
        assert_eq!(p.keep_loaded, Some(false));
        assert_eq!(p.width, Some(Width::Default));
        assert_eq!(p.region, None);
    }

    #[test]
    fn unparsable_json_drops_plugin_with_one_warning() {
        let r = run(&scan(&[("broken", "{ not json".into())]));
        only_warning(&r, "not valid JSON");
        assert!(r.warnings[0].starts_with("plugins/broken:"));
    }

    #[test]
    fn json_array_drops_plugin_with_one_warning() {
        let r = run(&scan(&[("listy", "[]".into())]));
        only_warning(&r, "must be a JSON object");
    }

    #[test]
    fn missing_required_key_drops_plugin_with_one_warning() {
        for key in ["apiVersion", "id", "kind", "entry"] {
            let mut m = bar_manifest();
            m.as_object_mut().unwrap().remove(key);
            let r = run(&one("diskwatch", m));
            only_warning(&r, &format!("missing required key \"{key}\""));
        }
    }

    #[test]
    fn wrong_api_version_drops_plugin_with_one_warning() {
        let mut m = bar_manifest();
        m["apiVersion"] = json!(2);
        only_warning(&run(&one("diskwatch", m)), "apiVersion 2 is not supported");
    }

    #[test]
    fn id_must_equal_directory_name() {
        let mut m = bar_manifest();
        m["id"] = json!("somethingelse");
        only_warning(
            &run(&one("diskwatch", m)),
            "does not match its directory name",
        );
    }

    #[test]
    fn id_with_illegal_characters_drops_plugin() {
        let m = json!({ "apiVersion": 1, "id": "Disk Watch", "kind": "bar", "entry": "D.sh" });
        only_warning(
            &run(&one("Disk Watch", m)),
            "lowercase letters, digits and dashes",
        );
    }

    #[test]
    fn unknown_kind_drops_plugin_with_one_warning() {
        let mut m = bar_manifest();
        m["kind"] = json!("shellscript");
        only_warning(&run(&one("diskwatch", m)), "unknown kind \"shellscript\"");
    }

    #[test]
    fn entry_escaping_plugin_dir_is_rejected() {
        for bad in [
            "/etc/passwd",
            "../Other.sh",
            "sub/../../Other.sh",
            "..",
            "",
        ] {
            let mut m = bar_manifest();
            m["entry"] = json!(bad);
            only_warning(
                &run(&one("diskwatch", m)),
                "must be a path inside the plugin directory",
            );
        }
    }

    #[test]
    fn entry_with_dots_in_a_filename_is_accepted() {
        let mut m = bar_manifest();
        m["entry"] = json!("widgets/Disk..Watch.sh");
        let r = run(&one("diskwatch", m));
        assert_eq!((r.warnings.len(), r.plugins.len()), (0, 1));
    }

    #[test]
    fn wrong_kind_key_is_dropped_to_default_but_plugin_survives() {
        let cases = [
            (
                "svc",
                json!({ "apiVersion": 1, "id": "svc", "kind": "service", "entry": "S.sh", "region": "left" }),
                "region",
            ),
            (
                "diskwatch",
                json!({ "apiVersion": 1, "id": "diskwatch", "kind": "bar", "entry": "D.sh", "keepLoaded": true }),
                "keepLoaded",
            ),
            (
                "over",
                json!({ "apiVersion": 1, "id": "over", "kind": "overlay", "entry": "O.sh", "width": "wide" }),
                "width",
            ),
        ];
        for (id, manifest, key) in &cases {
            let r = run(&one(id, manifest.clone()));
            assert_eq!(r.plugins.len(), 1);
            assert_eq!(r.warnings.len(), 1);
            assert!(r.warnings[0].contains(&format!("\"{key}\" is not a valid key for kind")));
        }
        // The offending key never leaks into the resolved shape.
        let svc = &run(&one("svc", cases[0].1.clone())).plugins[0].clone();
        assert_eq!(svc.region, None);
        let bar = &run(&one("diskwatch", cases[1].1.clone())).plugins[0].clone();
        assert_eq!(bar.keep_loaded, None);
        let over = &run(&one("over", cases[2].1.clone())).plugins[0].clone();
        assert_eq!(over.width, None);
    }

    fn launcher_manifest() -> Value {
        json!({ "apiVersion": 1, "id": "notes", "kind": "launcher", "entry": "N.sh", "mode": "Notes", "subjects": ["file", "app"] })
    }

    #[test]
    fn valid_launcher_manifest_carries_its_mode_and_subjects() {
        let r = run(&one("notes", launcher_manifest()));
        assert_eq!(r.warnings, Vec::<String>::new());
        let p = &r.plugins[0];
        assert_eq!(p.kind, Kind::Launcher);
        assert_eq!(p.mode.as_deref(), Some("Notes"));
        assert_eq!(p.subjects, Some(vec!["file".to_owned(), "app".to_owned()]));
        assert_eq!((p.region, p.keep_loaded, p.width), (None, None, None));
    }

    #[test]
    fn launcher_keys_default_and_mean_nothing_elsewhere() {
        let mut m = launcher_manifest();
        m.as_object_mut().unwrap().remove("subjects");
        let p = run(&one("notes", m)).plugins.remove(0);
        assert_eq!(p.subjects, Some(Vec::new()));
        let p = run(&one("diskwatch", bar_manifest())).plugins.remove(0);
        assert_eq!((p.mode, p.subjects), (None, None));
        for key in ["mode", "subjects"] {
            let mut m = bar_manifest();
            m[key] = json!("x");
            let r = run(&one("diskwatch", m));
            assert_eq!(r.plugins.len(), 1);
            assert!(r.warnings[0].contains(&format!("\"{key}\" is not a valid key for kind \"bar\"")), "{:?}", r.warnings);
            assert_eq!((r.plugins[0].mode.clone(), r.plugins[0].subjects.clone()), (None, None));
        }
    }

    #[test]
    fn bad_launcher_values_drop_to_their_default_with_a_warning() {
        let mut m = launcher_manifest();
        m["mode"] = json!("");
        m["subjects"] = json!(["file", 3]);
        let r = run(&one("notes", m));
        assert_eq!(r.plugins.len(), 1);
        assert_eq!((r.plugins[0].mode.clone(), r.plugins[0].subjects.clone()), (None, Some(Vec::new())));
        assert_eq!(r.warnings.len(), 3, "{:?}", r.warnings);
        assert!(r.warnings[0].contains("\"mode\" must be a non-empty string"));
        assert!(r.warnings[1].contains("\"subjects\" must be an array"));
        assert!(r.warnings[2].contains("adds nothing"));
    }

    #[test]
    fn a_launcher_plugin_with_only_subjects_has_no_mode() {
        let mut m = launcher_manifest();
        m.as_object_mut().unwrap().remove("mode");
        let r = run(&one("notes", m));
        assert_eq!(r.warnings.len(), 0);
        assert_eq!(r.plugins[0].mode, None);
    }

    #[test]
    fn illegal_enum_value_falls_back_with_warning() {
        let mut m = bar_manifest();
        m["region"] = json!("middle");
        let r = run(&one("diskwatch", m));
        assert_eq!(r.plugins.len(), 1);
        assert_eq!(r.plugins[0].region, Some(Region::Right));
        assert_eq!(r.warnings.len(), 1);
        assert!(r.warnings[0].contains("unknown region \"middle\""));

        let w = run(&one(
            "notes",
            json!({ "apiVersion": 1, "id": "notes", "kind": "panel", "entry": "N.sh", "width": "huge" }),
        ));
        assert_eq!(w.plugins.len(), 1);
        assert_eq!(w.plugins[0].width, Some(Width::Default));
        assert_eq!(w.warnings.len(), 1);
        assert!(w.warnings[0].contains("unknown width \"huge\""));
    }

    #[test]
    fn unknown_manifest_key_warns_but_keeps_plugin() {
        let mut m = bar_manifest();
        m["enabled"] = json!(false);
        let r = run(&one("diskwatch", m));
        assert_eq!(r.plugins.len(), 1);
        assert_eq!(r.warnings.len(), 1);
        assert!(r.warnings[0].contains("unknown manifest key \"enabled\""));
    }

    #[test]
    fn unknown_keys_warn_in_the_order_written() {
        let text =
            r#"{"zeta":1,"apiVersion":1,"id":"diskwatch","kind":"bar","entry":"D.sh","alpha":2}"#;
        let r = run(&scan(&[("diskwatch", text.into())]));
        assert_eq!(r.warnings.len(), 2);
        assert!(r.warnings[0].contains("\"zeta\""));
        assert!(r.warnings[1].contains("\"alpha\""));
    }

    #[test]
    fn name_defaults_to_id_and_a_non_string_warns() {
        let mut m = bar_manifest();
        m["name"] = json!(7);
        let r = run(&one("diskwatch", m));
        assert_eq!(r.plugins.len(), 1);
        assert_eq!(r.plugins[0].name, "diskwatch");
        assert_eq!(r.warnings.len(), 1);
        assert!(r.warnings[0].contains("\"name\" must be a non-empty string"));
    }

    fn plain(id: &str, kind: &str, entry: &str) -> (String, String) {
        (
            id.into(),
            json!({ "apiVersion": 1, "id": id, "kind": kind, "entry": entry }).to_string(),
        )
    }

    fn scan_owned(records: &[(String, String)]) -> String {
        scan(
            &records
                .iter()
                .map(|(i, t)| (i.as_str(), t.clone()))
                .collect::<Vec<_>>(),
        )
    }

    #[test]
    fn disabled_ids_are_excluded_without_warning() {
        let text = scan_owned(&[
            plain("alpha", "bar", "A.sh"),
            plain("beta", "bar", "B.sh"),
        ]);
        let r = resolve(Some(&text), &["beta".into()]);
        assert_eq!(r.plugins.len(), 1);
        assert_eq!(r.plugins[0].id, "alpha");
        assert!(r.by_id("beta").is_none());
        assert_eq!(r.warnings.len(), 0);
    }

    #[test]
    fn disabled_id_with_a_broken_manifest_never_warns() {
        let r = resolve(
            Some(&scan(&[("broken", "{ not json".into())])),
            &["broken".into()],
        );
        assert_eq!((r.plugins.len(), r.warnings.len()), (0, 0));
    }

    #[test]
    fn entry_url_is_file_scheme_joined_to_dir() {
        let p = run(&one("diskwatch", bar_manifest())).plugins.remove(0);
        assert_eq!(p.entry_url, format!("file://{DIR}/diskwatch/DiskWatch.sh"));
        assert_eq!(entry_url(&p), p.entry_url);
    }

    #[test]
    fn surface_key_carries_the_plugin_prefix() {
        let p = run(&one("diskwatch", bar_manifest())).plugins.remove(0);
        assert_eq!(surface_key(&p), "plugin:diskwatch");
        assert_eq!(PLUGIN_PREFIX, "plugin:");
    }

    #[test]
    fn plugins_are_id_sorted() {
        let text = scan_owned(&[
            plain("zulu", "bar", "Z.sh"),
            plain("alpha", "bar", "A.sh"),
        ]);
        let ids: Vec<_> = run(&text).plugins.into_iter().map(|p| p.id).collect();
        assert_eq!(ids.join(","), "alpha,zulu");
    }

    #[test]
    fn selectors_partition_by_kind() {
        let text = scan_owned(&[
            plain("b", "bar", "B.sh"),
            plain("p", "panel", "P.sh"),
            plain("o", "overlay", "O.sh"),
            plain("s", "service", "S.sh"),
            plain("l", "launcher", "L.sh"),
        ]);
        let all = run(&text).plugins;
        assert_eq!(all.len(), 5);
        assert_eq!(bar_plugins(&all).len(), 1);
        assert_eq!(surface_plugins(&all).len(), 2);
        assert_eq!(service_plugins(&all).len(), 1);
        assert_eq!(launcher_plugins(&all).count(), 1);
        assert_eq!(
            bar_plugins(&all).len() + surface_plugins(&all).len() + service_plugins(&all).len() + launcher_plugins(&all).count(),
            all.len()
        );
        assert_eq!(bar_plugins(&[]).len(), 0);
    }

    #[test]
    fn one_broken_plugin_never_drops_its_neighbours() {
        let text = scan_owned(&[
            plain("alpha", "bar", "A.sh"),
            ("broken".into(), "{ not json".into()),
            plain("zulu", "bar", "Z.sh"),
        ]);
        let r = run(&text);
        let ids: Vec<_> = r.plugins.iter().map(|p| p.id.as_str()).collect();
        assert_eq!(ids.join(","), "alpha,zulu");
        assert_eq!(r.warnings.len(), 1);
    }
}
