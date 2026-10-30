//! Launch-frequency ranking for the app rows and the emoji grid: how often an
//! id gets used, weighted by how recently.
//!
//! The score is `count` times an exponential recency decay of half-life
//! `HALF_LIFE_MS`. A never-used id scores exactly 0, and four launches a
//! month ago lose to one this morning. Frecency picks which of two
//! equally-good matches leads and nothing else: a stronger match tier still
//! wins outright.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

pub const HALF_LIFE_MS: f64 = 14.0 * 24.0 * 60.0 * 60.0 * 1000.0;

/// The ledger grows by one id per distinct thing ever used and never shrinks
/// on its own, so capping by current score on write is the whole of the
/// hygiene.
pub const MAX_ENTRIES: usize = 200;

/// One ledger record, as `state.json` stores it.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Record {
    pub id: String,
    #[serde(serialize_with = "js_number")]
    pub count: f64,
    #[serde(rename = "lastMs", serialize_with = "js_number")]
    pub last_ms: f64,
}

/// A whole number written the way JSON.stringify writes it, `1` not `1.0`.
fn js_number<S: serde::Serializer>(x: &f64, s: S) -> Result<S::Ok, S::Error> {
    if x.fract() == 0.0 && x.abs() < 9.0e15 { s.serialize_i64(*x as i64) } else { s.serialize_f64(*x) }
}

impl Record {
    pub fn new(id: &str, count: f64, last_ms: f64) -> Record {
        Record { id: id.to_string(), count, last_ms }
    }
}

/// One record's decayed score, 0 for none.
pub fn score_of(record: Option<&Record>, now_ms: f64) -> f64 {
    let Some(record) = record else { return 0.0 };
    let count = record.count;
    if !count.is_finite() || count <= 0.0 {
        return 0.0;
    }
    // A record with no usable timestamp decays not at all, rather than
    // decaying to nothing: state.json is hand-editable, and a missing
    // `lastMs` is missing information, not a launch in 1970.
    let last = record.last_ms;
    let age = if last.is_finite() && last > 0.0 { (now_ms - last).max(0.0) } else { 0.0 };
    count * 0.5_f64.powf(age / HALF_LIFE_MS)
}

fn index(store: &[Record]) -> HashMap<&str, &Record> {
    let mut out = HashMap::new();
    for record in store {
        if !record.id.is_empty() {
            out.insert(record.id.as_str(), record);
        }
    }
    out
}

/// 0 for anything this store has never seen.
pub fn score(store: &[Record], id: &str, now_ms: f64) -> f64 {
    score_of(index(store).get(id).copied(), now_ms)
}

fn num_or_zero(n: f64) -> f64 {
    if n.is_nan() { 0.0 } else { n }
}

/// A new store with `id` bumped; the argument is never mutated. `max_entries`
/// defaults to `MAX_ENTRIES`.
pub fn record(store: &[Record], id: &str, now_ms: f64, max_entries: Option<usize>) -> Vec<Record> {
    let mut out: Vec<Record> = Vec::new();
    let mut bumped = false;
    for entry in store {
        if entry.id.is_empty() {
            continue;
        }
        if entry.id == id {
            bumped = true;
            out.push(Record { id: id.to_string(), count: num_or_zero(entry.count) + 1.0, last_ms: now_ms });
        } else {
            out.push(Record {
                id: entry.id.clone(),
                count: num_or_zero(entry.count),
                last_ms: num_or_zero(entry.last_ms),
            });
        }
    }
    if id.is_empty() {
        return out;
    }
    if !bumped {
        out.push(Record { id: id.to_string(), count: 1.0, last_ms: now_ms });
    }
    cap(out, now_ms, max_entries.unwrap_or(MAX_ENTRIES))
}

fn cap(mut list: Vec<Record>, now_ms: f64, max_entries: usize) -> Vec<Record> {
    if list.len() <= max_entries {
        return list;
    }
    list.sort_by(|a, b| {
        score_of(Some(b), now_ms).partial_cmp(&score_of(Some(a), now_ms)).unwrap_or(std::cmp::Ordering::Equal)
    });
    list.truncate(max_entries);
    list
}

/// Stable reorder of `items` by descending score, ties keeping the caller's
/// own order. An empty store leaves the input order untouched. `id_of` reads
/// the ledger key off an item; the emoji route keys on a precomputed row id.
pub fn order<T>(items: Vec<T>, store: &[Record], now_ms: f64, id_of: impl Fn(&T) -> &str) -> Vec<T> {
    let index = index(store);
    let mut decorated: Vec<(f64, T)> = items
        .into_iter()
        .map(|item| (score_of(index.get(id_of(&item)).copied(), now_ms), item))
        .collect();
    // sort_by is stable: equal scores keep their original position.
    decorated.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    decorated.into_iter().map(|(_, item)| item).collect()
}

/// The same reorder as `order`, but sorting only the items the ledger scores
/// above zero: everything else keeps its original relative order untouched.
/// Equivalent to `order` for any input, but a route with thousands of
/// never-used entries behind one recorded id (the emoji grid) pays for a sort
/// over the handful that moved, not the whole list.
pub fn pull_recorded<T>(items: Vec<T>, store: &[Record], now_ms: f64, id_of: impl Fn(&T) -> &str) -> Vec<T> {
    let index = index(store);
    let mut recorded: Vec<(f64, T)> = Vec::new();
    let mut rest: Vec<T> = Vec::new();
    for item in items {
        let s = score_of(index.get(id_of(&item)).copied(), now_ms);
        if s > 0.0 {
            recorded.push((s, item));
        } else {
            rest.push(item);
        }
    }
    recorded.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    recorded.into_iter().map(|(_, item)| item).chain(rest).collect()
}
