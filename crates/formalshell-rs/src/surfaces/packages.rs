//! Install mode's rows (launcher spec §8) off the packages service's
//! answers, and a package row's ring while its install runs. What the rows
//! need asked is `services::packages::search_for`.

use fs_activity::{Item, State as Run};
use fs_menu::node::{Kind as NodeKind, Node};
use fs_menu::providers as p;

use crate::services::packages::NO_FSPEC;
use fs_types::fspec::AppVerb;
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
    let found = match &store.packages.search {
        Some((answered, found)) if answered == q => found,
        _ => return vec![p::install_searching_row()],
    };
    let found = match found {
        Err(why) if why == NO_FSPEC => return vec![p::install_no_fspec_row()],
        Err(why) => return vec![p::install_failed_row(why)],
        Ok(found) => found,
    };
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
fn phase_words(verb: AppVerb, item: &Item) -> String {
    let phase = match item.phase.as_deref().unwrap_or("") {
        "queued" => "Queued",
        "resolving" => "Resolving",
        "evaluating" | "planning" => "Preparing",
        "preparing" => "Downloading",
        "committing" if verb == AppVerb::Uninstall => "Removing",
        "committing" => "Installing",
        "pushing" => "Saving",
        _ if verb == AppVerb::Uninstall => "Removing",
        _ => "Installing",
    };
    match item.progress.fraction() {
        Some(f) => format!("{phase} {}%", f.to_percent().rounded()),
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
            let installed = started.verb == AppVerb::Install;
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
