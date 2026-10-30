//! Install mode's half on the UI thread (launcher spec §8): the rows off
//! the packages service, a package row's ring while its install runs, and
//! what the shell does once one lands.

use std::collections::HashSet;

use fs_activity::{Item, State as Run};
use fs_menu::node::{Kind as NodeKind, Node};
use fs_menu::providers as p;
use serde_json::json;

use crate::services::menu::{self as index, Ask};
use crate::services::packages::{self, Cmd, NO_FSPEC, Verb};
use crate::services::state;
use crate::store::Store;
use crate::ui::el::Ring;

/// How many apps already here head the packages for the same words.
const APPS_FIRST: usize = 3;

/// The app subjects matching `q`, then fspec's packages for it, or the one
/// dim row saying why there are none.
pub fn install_rows(store: &Store, q: &str) -> Vec<Node> {
    if !store.activity.fspec.installed {
        return vec![p::install_no_fspec_row()];
    }
    let q = q.trim();
    if q.chars().count() < 2 {
        return Vec::new();
    }
    packages::send(Cmd::Search(q.to_owned()));
    let found = match &store.packages.search {
        Some((answered, found)) if answered == q => found,
        _ => return vec![p::install_searching_row()],
    };
    let found = match found {
        Err(why) if why == NO_FSPEC => return vec![p::install_no_fspec_row()],
        Err(why) => return vec![p::install_failed_row(why)],
        Ok(found) => found,
    };
    let icons: Vec<String> = found.iter().filter_map(|f| f.icon.clone()).filter(|i| i.starts_with('/') && !store.menu.row_icons.contains_key(i)).collect();
    if !icons.is_empty() {
        index::ask(Ask::Icons(icons));
    }
    let boost = fs_menu::search::Boost { abbrev: None, picks: &[], now: 0.0 };
    let apps = fs_menu::search::rank_with(store.menu.nodes(), q, &store.menu.cond, Some("apps"), boost)
        .into_iter()
        .filter(|n| n.kind == NodeKind::App)
        .take(APPS_FIRST)
        .cloned();
    let mut packages = p::package_rows(found);
    for row in &mut packages {
        // Install again only once the last try failed.
        if live(store, &row.path).is_some_and(|l| l.ring.is_some() || l.installed == Some(true)) {
            row.action = None;
        }
    }
    let rows: Vec<Node> = apps.chain(packages).collect();
    if rows.is_empty() { vec![p::install_no_packages_row(q)] } else { rows }
}

/// A package row the shell is installing or removing: the ring in its icon
/// slot, the words on its right, and whether it now reads installed.
#[derive(Clone, Debug, PartialEq)]
pub struct Live {
    pub ring: Option<Ring>,
    pub value: String,
    pub installed: Option<bool>,
}

/// The operation's item, still live or folded into the history.
pub fn op_item<'a>(store: &'a Store, op: &str) -> Option<&'a Item> {
    let id = fs_activity::fspec::item_id(op);
    let a = &store.activity.activity;
    a.get(&id).or_else(|| a.history().find(|i| i.id == id))
}

/// fspec's phase as the row says it (`Downloading 40%`).
fn phase_words(verb: Verb, item: &Item) -> String {
    let phase = match item.phase.as_deref().unwrap_or("") {
        "queued" => "Queued",
        "resolving" => "Resolving",
        "evaluating" | "planning" => "Preparing",
        "preparing" => "Downloading",
        "committing" if verb == Verb::Uninstall => "Removing",
        "committing" => "Installing",
        "pushing" => "Saving",
        _ if verb == Verb::Uninstall => "Removing",
        _ => "Installing",
    };
    match item.progress.fraction() {
        Some(f) => format!("{phase} {}%", f.as_percent()),
        None => phase.to_owned(),
    }
}

pub fn live(store: &Store, id: &str) -> Option<Live> {
    let started = store.packages.started(id)?;
    let starting = || Live { ring: Some(Ring::Unknown), value: "Starting".into(), installed: None };
    let op = match &started.op {
        Ok(None) => return Some(starting()),
        Err(why) => return Some(Live { ring: None, value: why.clone(), installed: None }),
        Ok(Some(op)) => op,
    };
    let Some(item) = op_item(store, op) else { return Some(starting()) };
    Some(match item.state {
        Run::Running => Live { ring: Some(crate::surfaces::activity::ring(item)), value: phase_words(started.verb, item), installed: None },
        Run::Failed => Live { ring: None, value: item.summary.clone().unwrap_or_else(|| "Failed".into()), installed: None },
        Run::Succeeded | Run::Done => {
            let installed = started.verb == Verb::Install;
            Live { ring: None, value: if installed { "Installed" } else { "Removed" }.into(), installed: Some(installed) }
        }
    })
}

/// The trailing value a row reads, live for a package the shell is
/// changing.
pub fn row_value(store: &Store, row: &Node) -> String {
    match p::is_package(row).then(|| live(store, &row.path)).flatten() {
        Some(l) => l.value,
        None => fs_menu::actions::accessory_for(Some(row)),
    }
}

/// Whether a row is ticked: a package the shell installed or removed reads
/// how that ended, everything else `checked` as the row resolved it.
pub fn row_checked(store: &Store, row: &Node, checked: bool) -> bool {
    match p::is_package(row).then(|| live(store, &row.path)).flatten().and_then(|l| l.installed) {
        Some(installed) => installed,
        None => checked,
    }
}

/// Starts an install of `id`, named off the search that listed it, the
/// launcher's Enter and Activity's Retry alike.
pub fn install(store: &Store, id: &str) {
    let name = match &store.packages.search {
        Some((_, Ok(found))) => found.iter().find(|f| f.id == id).map(|f| f.name.clone()),
        _ => None,
    };
    let name = name.or_else(|| store.packages.started(id).map(|s| s.name.clone())).unwrap_or_default();
    packages::send(Cmd::Install(id.to_owned(), name, desktop_ids(store)));
}

/// The desktop ids the apps scan knows now, before an install.
pub fn desktop_ids(store: &Store) -> HashSet<String> {
    store.menu.exec.keys().filter(|k| !k.contains('\u{1}')).cloned().collect()
}

/// fspec operations that change what is installed.
fn changes_apps(kind: &str) -> bool {
    matches!(kind, "install" | "uninstall" | "undo")
}

/// What the shell does once an fspec install, uninstall or undo succeeds,
/// whoever started it: the apps scanned again and every owner answer
/// dropped. An install the launcher started also records a pick for the
/// app it brought, once the scan has it, so the app heads `Recent` once.
/// True when the launcher's rows may read differently.
pub fn land(store: &mut Store) -> bool {
    let done: Vec<String> = store
        .activity
        .activity
        .live()
        .iter()
        .chain(store.activity.activity.history())
        .filter(|i| i.origin == fs_activity::Origin::Fspec && changes_apps(&i.kind) && matches!(i.state, Run::Succeeded | Run::Done))
        .map(|i| i.id.clone())
        .filter(|id| !store.packages.landed.contains(id))
        .collect();
    let mut moved = false;
    if !done.is_empty() {
        store.packages.landed.extend(done);
        store.packages.owners.clear();
        packages::send(Cmd::Forget);
        index::ask(Ask::Rescan);
        moved = true;
    }
    let now = desktop_ids(store);
    let labels: std::collections::HashMap<String, String> =
        now.iter().filter_map(|id| store.menu.nodes().get(&format!("apps.{id}")).map(|n| (id.clone(), n.label.clone()))).collect();
    let mut claimed: HashSet<String> = store.packages.started.iter().filter_map(|s| s.app.clone()).collect();
    let mut picks = Vec::new();
    for s in store.packages.started.iter_mut().filter(|s| s.verb == Verb::Install && s.app.is_none()) {
        let Ok(Some(op)) = &s.op else { continue };
        if !store.packages.landed.contains(&fs_activity::fspec::item_id(op)) {
            continue;
        }
        let mut fresh: Vec<&String> = now
            .iter()
            .filter(|id| !s.before.contains(*id) && !claimed.contains(*id))
            .filter(|id| brought_by(id, labels.get(*id).map_or("", String::as_str), &s.id, &s.name))
            .collect();
        fresh.sort();
        if let Some(app) = fresh.first() {
            claimed.insert((*app).clone());
            s.app = Some((*app).clone());
            picks.push(format!("app:{app}"));
            moved = true;
        }
    }
    if !picks.is_empty() {
        let mut held = crate::surfaces::launcher::picks(store);
        let at = crate::surfaces::launcher::now_ms();
        for id in &picks {
            held = fs_menu::frecency::record(&held, id, at, None);
        }
        state::set(vec![state::Field::Picks(json!(held))]);
    }
    moved
}

/// Whether desktop entry `desktop` (shown as `label`) is the app package
/// `id` (`<source>:<ref>`, catalog name `name`) brought: a Flathub app's
/// entry is named after its ref, anything else matches by name.
fn brought_by(desktop: &str, label: &str, id: &str, name: &str) -> bool {
    let reference = id.split_once(':').map_or(id, |(_, r)| r);
    let tail = |s: &str| s.rsplit(['.', '/']).next().unwrap_or(s).to_owned();
    desktop.eq_ignore_ascii_case(reference)
        || tail(desktop).eq_ignore_ascii_case(&tail(reference))
        || (!name.is_empty() && (label.eq_ignore_ascii_case(name) || desktop.eq_ignore_ascii_case(name)))
}

/// The app an Activity install row's Open launches, once it landed.
pub fn landed_app<'a>(store: &'a Store, source: &str) -> Option<&'a str> {
    let id = source.strip_prefix("fspec:install:")?;
    store.packages.started(id).filter(|s| s.verb == Verb::Install).and_then(|s| s.app.as_deref())
}

#[cfg(test)]
mod tests {
    use super::brought_by;

    #[test]
    fn an_install_claims_only_its_own_app() {
        assert!(brought_by("org.mozilla.firefox", "Firefox", "flatpak:org.mozilla.firefox", "Firefox"));
        assert!(brought_by("firefox", "Firefox", "zypper:firefox", "firefox"));
        assert!(brought_by("org.gnome.Btop", "btop", "zypper:btop", "btop"));
        assert!(!brought_by("org.gnome.Calculator", "Calculator", "zypper:btop", "btop"));
        assert!(!brought_by("org.example.Other", "Other", "aqua:ripgrep", "ripgrep"));
    }
}
