// Abbreviations: the three sources and their order, dominance, decay, the
// pair cap, forgetting, and what each does to the ranking.

mod common;

use std::collections::HashMap;

use common::{no_conds, tree_of};
use fs_menu::abbrev::{self, Book, Source, forget, learn, lookup, pin};
use fs_menu::search::{self, Boost};
use serde_json::json;

const DAY: f64 = 24.0 * 60.0 * 60.0 * 1000.0;
const NOW: f64 = 1_000.0 * DAY;

fn all(_: &str) -> bool {
    true
}

fn none() -> HashMap<String, String> {
    HashMap::new()
}

fn taught(pairs: &[(&str, &str, f64)]) -> Book {
    pairs.iter().fold(Book::default(), |b, (q, id, at)| learn(&b, q, id, *at))
}

#[test]
fn two_picks_dominate_one_does_not() {
    let once = taught(&[("fx", "app:foxtrot", NOW)]);
    let l = lookup(&none(), &once, "fx", NOW, all);
    assert_eq!(l.lead, None);
    assert_eq!(l.boost.get("app:foxtrot"), Some(&50));
    let twice = taught(&[("fx", "app:foxtrot", NOW), ("fx", "app:foxtrot", NOW)]);
    assert_eq!(lookup(&none(), &twice, "FX", NOW, all).lead, Some(("app:foxtrot".into(), Source::Learned)));
}

#[test]
fn a_stale_record_does_not_lead() {
    let old = taught(&[("fx", "app:foxtrot", NOW - 60.0 * DAY), ("fx", "app:foxtrot", NOW - 60.0 * DAY)]);
    assert_eq!(lookup(&none(), &old, "fx", NOW, all).lead, None);
    // Fourteen days on, two picks are exactly one half-life and still score 1.
    let fortnight = taught(&[("fx", "app:foxtrot", NOW - 14.0 * DAY), ("fx", "app:foxtrot", NOW - 14.0 * DAY)]);
    assert!(lookup(&none(), &fortnight, "fx", NOW, all).lead.is_some());
}

#[test]
fn two_subjects_sharing_a_query_boost_both() {
    let b = taught(&[("te", "app:terminal", NOW), ("te", "app:terminal", NOW), ("te", "app:telegram", NOW), ("te", "app:telegram", NOW)]);
    let l = lookup(&none(), &b, "te", NOW, all);
    assert_eq!(l.lead, None);
    assert_eq!(l.boost.len(), 2);
}

#[test]
fn configured_beats_pinned_beats_learned() {
    let b = pin(&taught(&[("ff", "app:files", NOW), ("ff", "app:files", NOW)]), "ff", "app:fish");
    assert_eq!(lookup(&none(), &b, "ff", NOW, all).lead, Some(("app:fish".into(), Source::Pinned)));
    let configured = HashMap::from([("ff".to_string(), "app:firefox.desktop".to_string())]);
    let l = lookup(&configured, &b, "ff", NOW, all);
    assert_eq!(l.lead, Some(("app:firefox".into(), Source::Configured)));
    assert_eq!(l.source_of("app:files"), Some(Source::Learned));
    // A configured subject that is gone does not hide the rest; the pin
    // stays ignored while the configured one exists.
    let l = lookup(&configured, &b, "ff", NOW, |id| id != "app:firefox");
    assert_eq!(l.lead, Some(("app:files".into(), Source::Learned)));
}

#[test]
fn keys_are_short_lowercase_and_never_a_prefix() {
    assert_eq!(abbrev::key(" FF "), Some("ff".into()));
    assert_eq!(abbrev::key(""), None);
    assert_eq!(abbrev::key("abcdefghi"), None);
    assert_eq!(abbrev::key(":e"), None);
    assert_eq!(learn(&Book::default(), ":e x", "a", NOW), Book::default());
}

#[test]
fn decayed_records_drop_and_pairs_cap() {
    let b = taught(&[("old", "a", NOW - 50.0 * DAY), ("new", "b", NOW)]);
    assert!(!b.learned.contains_key("old"));
    let mut many = Book::default();
    for i in 0..abbrev::MAX_PAIRS + 20 {
        many = learn(&many, &format!("q{}", i % 100), &format!("s{i}"), NOW - i as f64);
    }
    assert_eq!(many.learned.values().map(Vec::len).sum::<usize>(), abbrev::MAX_PAIRS);
}

#[test]
fn forget_starts_a_pair_again_from_one() {
    let b = taught(&[("fx", "app:foxtrot", NOW), ("fx", "app:foxtrot", NOW)]);
    let gone = forget(&pin(&b, "fx", "app:foxtrot"), "fx", "app:foxtrot");
    assert!(gone.learned.is_empty() && gone.pinned.is_empty());
    let again = learn(&gone, "fx", "app:foxtrot", NOW);
    assert_eq!(lookup(&none(), &again, "fx", NOW, all).lead, None);
}

#[test]
fn a_learned_lead_ranks_above_an_exact_label() {
    let tree = tree_of(json!({
        "apps": { "provider": "apps" },
        "apps.fx": { "label": "fx", "kind": "app" },
        "apps.foxtrot": { "label": "Foxtrot", "kind": "app" }
    }));
    let ids = |l: &abbrev::Lookup| -> Vec<String> {
        let boost = Boost { abbrev: Some(l), picks: &[], now: NOW };
        search::rank_with(&tree.nodes, "fx", &no_conds(), None, boost).iter().map(|n| n.id.clone()).collect()
    };
    let none_learned = lookup(&none(), &Book::default(), "fx", NOW, all);
    assert_eq!(ids(&none_learned), ["apps.fx", "apps.foxtrot"]);
    // The tree's nodes here carry no desktop entry, so their subject id is
    // the node id.
    let b = taught(&[("fx", "apps.foxtrot", NOW), ("fx", "apps.foxtrot", NOW)]);
    assert_eq!(ids(&lookup(&none(), &b, "fx", NOW, all)), ["apps.foxtrot", "apps.fx"]);
}

// A record decayed past keeping is dropped before the pick, so one pick 60
// days ago and one now is one pick, which does not lead.
#[test]
fn a_pick_after_a_stale_one_starts_again_from_one() {
    let book = taught(&[("fx", "app:foxtrot", NOW - 60.0 * DAY), ("fx", "app:foxtrot", NOW)]);
    assert_eq!(book.learned["fx"][0].count, 1.0);
    assert_eq!(lookup(&none(), &book, "fx", NOW, all).lead, None);
}
