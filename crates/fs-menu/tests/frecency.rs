mod common;

use std::collections::HashMap;

use common::*;
use fs_menu::frecency::{self, Record};
use fs_menu::node::{DesktopEntry, Kind, Node, Provider, Tree};
use fs_menu::providers::{ProviderFn, apply_providers, apps_provider};
use fs_menu::search;

const DAY: f64 = 24.0 * 60.0 * 60.0 * 1000.0;
// 2026-08-06T12:00:00Z
const NOW: f64 = 1_786_017_600_000.0;

fn entry(id: &str, name: &str) -> DesktopEntry {
    desktop_entry(id, name)
}

fn rec(id: &str, count: f64, last_ms: f64) -> Record {
    Record::new(id, count, last_ms)
}

/// What the launcher's tree binding does: the apps provider's rows, parented
/// under a provider node by the real `apply_providers`, which is where their
/// declaration order (and so the search's decl index) comes from.
fn apps_tree(entries: Vec<DesktopEntry>, launches: Vec<Record>) -> Tree {
    let mut tree = Tree::default();
    tree.nodes.insert(
        "apps".into(),
        Node {
            provider: Some(Provider::Apps),
            ..Node::new("apps", "Apps", Kind::Provider)
        },
    );
    let mut fns: HashMap<Provider, ProviderFn> = HashMap::new();
    fns.insert(Provider::Apps, Box::new(move || apps_provider(&entries, None, &launches, Some(NOW))));
    apply_providers(&mut tree, &fns);
    tree
}

fn ids_of(items: &[Item]) -> Vec<&str> {
    items.iter().map(|i| i.id.as_str()).collect()
}

#[derive(Clone)]
struct Item {
    id: String,
}

fn item(id: &str) -> Item {
    Item { id: id.into() }
}

// score

#[test]
fn never_launched_entry_scores_zero() {
    assert_eq!(frecency::score(&[], "firefox", NOW), 0.0);
    assert_eq!(frecency::score(&[rec("mpv", 9.0, NOW)], "firefox", NOW), 0.0);
}

#[test]
fn more_launches_scores_higher() {
    let store = [rec("often", 12.0, NOW), rec("rarely", 2.0, NOW)];
    assert!(frecency::score(&store, "often", NOW) > frecency::score(&store, "rarely", NOW));
}

#[test]
fn recent_launch_outranks_older_one_of_the_same_count() {
    let store = [rec("today", 3.0, NOW), rec("lastmonth", 3.0, NOW - 30.0 * DAY)];
    assert!(frecency::score(&store, "today", NOW) > frecency::score(&store, "lastmonth", NOW));
}

#[test]
fn old_pile_of_launches_loses_to_one_recent_launch() {
    let store = [rec("stale", 4.0, NOW - 60.0 * DAY), rec("fresh", 1.0, NOW)];
    assert!(frecency::score(&store, "fresh", NOW) > frecency::score(&store, "stale", NOW));
}

#[test]
fn zero_count_and_absent_record_both_score_zero() {
    assert_eq!(frecency::score(&[rec("never", 0.0, NOW)], "never", NOW), 0.0);
}

// record

#[test]
fn record_appends_a_first_launch() {
    let store = frecency::record(&[], "firefox", NOW, None);
    assert_eq!(store.len(), 1);
    assert_eq!(store[0], rec("firefox", 1.0, NOW));
}

#[test]
fn record_increments_and_restamps_an_existing_entry() {
    let store = frecency::record(&[rec("firefox", 4.0, NOW - 10.0 * DAY)], "firefox", NOW, None);
    assert_eq!(store.len(), 1);
    assert_eq!(store[0].count, 5.0);
    assert_eq!(store[0].last_ms, NOW);
}

#[test]
fn record_never_mutates_the_store_it_was_given() {
    let store = vec![rec("firefox", 1.0, 1000.0)];
    let next = frecency::record(&store, "firefox", 2000.0, None);
    assert_eq!(store[0].count, 1.0);
    assert_eq!(store[0].last_ms, 1000.0);
    assert_eq!(next[0].count, 2.0);
}

#[test]
fn record_caps_the_store_by_score() {
    let store = [rec("hot", 5.0, NOW), rec("ancient", 1.0, NOW - 200.0 * DAY)];
    let next = frecency::record(&store, "new", NOW, Some(2));
    assert_eq!(next.len(), 2);
    assert_eq!(next[0].id, "hot");
    assert_eq!(next[1].id, "new");
}

// order

#[test]
fn order_puts_the_most_frecent_first() {
    let items = vec![item("a"), item("b"), item("c")];
    let store = [rec("c", 6.0, NOW), rec("b", 1.0, NOW)];
    let ordered = frecency::order(items, &store, NOW, |i| i.id.as_str());
    assert_eq!(ids_of(&ordered), ["c", "b", "a"]);
}

#[test]
fn order_is_stable_for_entries_that_tie() {
    let items = vec![item("a"), item("b"), item("c")];
    let ordered = frecency::order(items, &[], NOW, |i| i.id.as_str());
    assert_eq!(ids_of(&ordered), ["a", "b", "c"]);
}

// pull_recorded: the emoji route's per-keystroke reorder. Same contract as
// order (recorded ids lead by score, everything else keeps its relative
// position), reached by decorating only the recorded subset.

#[test]
fn pull_recorded_moves_only_ledger_entries_to_the_front() {
    let items = vec![item("a"), item("b"), item("c"), item("d")];
    let store = [rec("c", 6.0, NOW)];
    let ordered = frecency::pull_recorded(items, &store, NOW, |i| i.id.as_str());
    assert_eq!(ids_of(&ordered), ["c", "a", "b", "d"]);
}

#[test]
fn pull_recorded_leaves_input_order_alone_with_no_ledger() {
    let items = vec![item("a"), item("b"), item("c")];
    let ordered = frecency::pull_recorded(items, &[], NOW, |i| i.id.as_str());
    assert_eq!(ids_of(&ordered), ["a", "b", "c"]);
}

// Multiple recorded ids still sort by score among themselves, and a stale one
// that has decayed to zero stays with the untouched rest rather than jumping
// the queue on presence in the store alone.
#[test]
fn pull_recorded_sorts_multiple_hits_and_drops_decayed_ones() {
    let items = vec![item("a"), item("b"), item("c")];
    let year = 365.0 * 24.0 * 60.0 * 60.0 * 1000.0;
    let store = [rec("a", 1.0, NOW), rec("c", 9.0, NOW), rec("b", 50.0, NOW - 50.0 * year)];
    let ordered = frecency::pull_recorded(items, &store, NOW, |i| i.id.as_str());
    assert_eq!(ids_of(&ordered), ["c", "a", "b"]);
}

// apps_provider integration

#[test]
fn apps_provider_orders_rows_by_launch_frecency() {
    let rows = apps_provider(
        &[entry("fish", "Fish Shell"), entry("files", "Files")],
        None,
        &[rec("files", 3.0, NOW)],
        Some(NOW),
    );
    assert_eq!(rows[0].id, "apps.files");
    assert_eq!(rows[1].id, "apps.fish");
}

#[test]
fn apps_provider_without_a_store_keeps_desktop_entry_order() {
    let rows = apps_provider(&[entry("fish", "Fish Shell"), entry("files", "Files")], None, &[], Some(NOW));
    assert_eq!(rows[0].id, "apps.fish");
    assert_eq!(rows[1].id, "apps.files");
}

// Search integration: frecency only ever breaks a tie.

#[test]
fn launch_frecency_breaks_a_tie_between_equally_good_matches() {
    let entries = vec![entry("fish", "Fish Shell"), entry("files", "Files")];
    let cold = apps_tree(entries.clone(), vec![]);
    assert_eq!(search::rank(&cold.nodes, "fi", &no_conds(), None)[0].id, "apps.fish");
    let warm = apps_tree(entries, vec![rec("files", 3.0, NOW)]);
    assert_eq!(search::rank(&warm.nodes, "fi", &no_conds(), None)[0].id, "apps.files");
}

#[test]
fn fuzzy_score_still_dominates_for_a_specific_query() {
    let entries = vec![entry("recent-files", "Recent Files"), entry("files", "Files")];
    let launches = vec![rec("recent-files", 40.0, NOW)];
    let tree = apps_tree(entries, launches);
    let ranked = search::rank(&tree.nodes, "files", &no_conds(), None);
    // Exact label match (tier 1000) over a contains match (tier 600): forty
    // launches cannot buy a tier.
    assert_eq!(ranked[0].id, "apps.files");
    assert_eq!(ranked[1].id, "apps.recent-files");
}
