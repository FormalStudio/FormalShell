// Loads the real vendored dataset (shell/Menu/emoji.json), not a fixture: the
// point is proving the generated file parses and carries the mappings the
// emoji route searches.

mod common;

use std::sync::LazyLock;
use std::time::{SystemTime, UNIX_EPOCH};

use common::*;
use fs_menu::frecency::Record;
use fs_menu::model::parse_headered_json;
use fs_menu::providers::{EmojiEntry, EmojiIndex};

static DATASET: LazyLock<Vec<EmojiEntry>> = LazyLock::new(|| {
    let value = parse_headered_json(&read_repo("crates/fs-menu/data/emoji.json")).expect("emoji.json parses");
    serde_json::from_value(value).expect("emoji entries")
});

static REAL: LazyLock<EmojiIndex> = LazyLock::new(|| EmojiIndex::new(DATASET.clone()));

fn fixture(items: &[(&str, &str, Option<&str>)]) -> EmojiIndex {
    EmojiIndex::new(
        items
            .iter()
            .map(|(ch, name, kw)| EmojiEntry {
                ch: (*ch).into(),
                name: (*name).into(),
                group: "g".into(),
                kw: kw.map(str::to_string),
            })
            .collect(),
    )
}

fn icons(index: &EmojiIndex, query: &str) -> Vec<String> {
    index.rows(query, true, &[], None).into_iter().map(|r| r.icon).collect()
}

fn now() -> f64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis() as f64
}

#[test]
fn dataset_loads() {
    assert!(DATASET.len() > 3000);
    let e = &DATASET[0];
    assert!(!e.ch.is_empty());
    assert!(!e.name.is_empty());
    assert!(!e.group.is_empty());
}

// `kw` is optional per entry (CLDR has nothing extra for some, and the
// generator drops keywords the name already carries), but most of the set has
// one, and every one it writes is lowercase and pipe separated: the search
// matches against it raw, with no lowercasing of its own.
#[test]
fn keywords_are_vendored_lowercase() {
    let mut with_kw = 0;
    for e in DATASET.iter() {
        let Some(kw) = &e.kw else { continue };
        with_kw += 1;
        assert_eq!(kw, &kw.to_lowercase());
        assert!(!kw.contains("||"));
    }
    assert!(with_kw > 2000, "{with_kw} entries carry keywords");
}

#[test]
fn known_mapping() {
    let rows = REAL.rows("thumbs up", true, &[], None);
    assert!(!rows.is_empty());
    assert_eq!(rows[0].icon, "\u{1F44D}");
    assert_eq!(rows[0].label, "THUMBS UP");
    assert_eq!(rows[0].kind, fs_menu::node::Kind::Action);
    assert_eq!(rows[0].action.as_deref(), Some("wl-copy -- '\u{1F44D}'"));
    // paste_after marks the row for the post-close paste hook, the same field
    // and config key a clipboard-history row uses.
    assert!(rows[0].paste_after);
    assert_eq!(rows[0].verb.as_deref(), Some("Paste"));
}

// clipboard.paste off: the row still copies, it just stops touching the
// window focus returns to, and says Copy instead of Paste.
#[test]
fn paste_off() {
    let rows = REAL.rows("thumbs up", false, &[], None);
    assert!(!rows[0].paste_after);
    assert_eq!(rows[0].verb.as_deref(), Some("Copy"));
    assert_eq!(rows[0].action.as_deref(), Some("wl-copy -- '\u{1F44D}'"));
}

#[test]
fn exact_beats_earlier_substring() {
    // "grinning cat" (Smileys & Emotion) precedes "cat" (Animals & Nature) in
    // file order; the exact-name tier must still win.
    let rows = REAL.rows("cat", true, &[], None);
    assert_eq!(rows[0].label, "CAT");
    assert_eq!(rows[0].icon, "\u{1F408}");
}

// All four name tiers in one query, in order. The ranking is a bucket pass
// rather than a comparator sort (a one-letter query matches most of the
// dataset), so the tier boundaries and the file order inside a tier are what a
// regression would break.
#[test]
fn the_four_tiers_rank_in_order() {
    let index = fixture(&[
        ("1", "red apple", None),      // no match at all
        ("2", "cat face", None),       // prefix
        ("3", "cat", None),            // exact
        ("4", "black cat", None),      // word start
        ("5", "cat with tears", None), // prefix, later in file order
    ]);
    // exact, then the two prefixes in file order, then word start. "red apple"
    // has no "cat" in it at all and must not appear.
    assert_eq!(icons(&index, "cat"), ["3", "2", "5", "4"]);
}

// Uncapped: the route is a scrolling grid, and the old 40-result ceiling put
// all but five rows of the set out of reach.
#[test]
fn browse_shows_the_whole_set() {
    let all = REAL.rows("", true, &[], None);
    assert_eq!(all.len(), DATASET.len());
    assert_eq!(all[0].label, "GRINNING FACE");
    assert_eq!(REAL.rows("zzzznotanemoji", true, &[], None).len(), 0);
}

// A one-letter query matches most of the dataset. Every match has to survive
// to the grid: the cell the owner is looking for is exactly as likely to be
// the 400th as the 4th. Asserted as containment rather than a count, since
// keyword hits legitimately add rows no name carries.
#[test]
fn a_broad_query_is_not_truncated() {
    let broad = REAL.rows("a", true, &[], None);
    let seen: std::collections::HashSet<&str> = broad.iter().map(|r| r.icon.as_str()).collect();
    let mut missing = 0;
    let mut expected = 0;
    for e in DATASET.iter() {
        if !e.name.to_lowercase().contains('a') {
            continue;
        }
        expected += 1;
        if !seen.contains(e.ch.as_str()) {
            missing += 1;
        }
    }
    assert_eq!(missing, 0);
    assert!(broad.len() >= expected);
    assert!(broad.len() > 40);
}

// CLDR's keywords, the thing that makes the route searchable the way macOS's
// picker is: Unicode's name for the crying face is "loudly crying face", and
// nothing in it says "sob".
#[test]
fn keywords_find_what_the_name_does_not() {
    assert_eq!(REAL.rows("sob", true, &[], None)[0].icon, "\u{1F62D}");
    assert_eq!(REAL.rows("lmao", true, &[], None)[0].icon, "\u{1F923}");
    assert_eq!(REAL.rows("+1", true, &[], None)[0].icon, "\u{1F44D}");
}

// The keyword tiers sit below every name tier: a row whose visible name
// matches always leads one that only matched on data the user cannot see.
#[test]
fn a_name_match_still_beats_a_keyword_match() {
    let index = fixture(&[
        ("1", "unrelated face", Some("cat")),
        ("2", "cat", None),
        ("3", "wildcat", None),
        ("4", "black cat", None),
        ("5", "another face", Some("catnip")),
    ]);
    // name exact, name word start, whole keyword, name substring, keyword word
    // start.
    assert_eq!(icons(&index, "cat"), ["2", "4", "1", "3", "5"]);
}

// Most-copied first inside a tier, never across one.
#[test]
fn usage_reorders_within_a_tier_only() {
    let index = fixture(&[
        ("1", "cat", None),
        ("2", "cat face", None),
        ("3", "cool cat", None),
        ("4", "cat with a hat", None),
    ]);
    let t = now();
    let uses = [Record::new("emoji.4", 9.0, t), Record::new("emoji.1", 1.0, t)];
    let order: Vec<String> = index.rows("cat", true, &uses, Some(t)).into_iter().map(|r| r.icon).collect();
    // "cat with a hat" leads the prefix tier over "cat face" on nine copies,
    // and still does not overtake the exact-name row above it.
    assert_eq!(order, ["1", "4", "2", "3"]);
}

// The browse grid is the surface the ranking is actually for: an empty query
// opens on what the user reaches for, not on Unicode file order.
#[test]
fn usage_leads_the_browse_grid() {
    let t = now();
    let uses = [Record::new("emoji.\u{1F62D}", 4.0, t)];
    assert_eq!(REAL.rows("", true, &uses, Some(t))[0].icon, "\u{1F62D}");
    // No ledger, no reordering: a fresh profile browses file order.
    assert_eq!(REAL.rows("", true, &[], Some(t))[0].label, "GRINNING FACE");
}

// A stale ledger entry decays rather than ruling forever: the same half-life
// the app rows use.
#[test]
fn a_stale_favourite_loses_to_a_recent_one() {
    let index = fixture(&[("1", "cat one", None), ("2", "cat two", None)]);
    let t = now();
    let year = 365.0 * 24.0 * 60.0 * 60.0 * 1000.0;
    let uses = [Record::new("emoji.1", 20.0, t - year), Record::new("emoji.2", 2.0, t)];
    let order: Vec<String> = index.rows("cat", true, &uses, Some(t)).into_iter().map(|r| r.icon).collect();
    assert_eq!(order, ["2", "1"]);
}

// The emoji grid browses the whole 3944-entry dataset since the cap came off.
// What these guard is that showing all of it stays cheap. The budget is
// deliberately loose: it asserts the work is linear and small, not a specific
// machine's speed. A regression that made it quadratic would blow past it by
// orders of magnitude rather than by a factor.
#[test]
fn building_every_row_stays_under_the_frame_budget() {
    let t0 = std::time::Instant::now();
    let rows = REAL.rows("", true, &[], None);
    let elapsed = t0.elapsed().as_millis();
    assert_eq!(rows.len(), DATASET.len());
    assert!(elapsed < 200, "building {} rows took {elapsed}ms", rows.len());
}

#[test]
fn repeat_search_is_not_slower_than_the_first() {
    REAL.rows("face", true, &[], None);
    let t0 = std::time::Instant::now();
    for _ in 0..5 {
        REAL.rows("face", true, &[], None);
    }
    let elapsed = t0.elapsed().as_millis();
    assert!(elapsed < 500, "five repeat searches took {elapsed}ms");
}

// A grid draws no headings, so the per-row section pass has nothing to
// produce and must not walk the list to say so.
#[test]
fn a_grid_skips_the_section_pass() {
    use fs_menu::model::{Mode, SectionCtx, sections_for};
    let rows = REAL.rows("", true, &[], None);
    let ctx = SectionCtx { grid: true, mode: Some(Mode::Menu), ..SectionCtx::default() };
    assert_eq!(sections_for(&rows, &ctx).len(), 0);
}
