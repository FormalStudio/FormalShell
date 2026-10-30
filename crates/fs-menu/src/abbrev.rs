//! Abbreviations: a short query that names one subject. Three sources, in
//! the order they win: configured in `settings.json` (`menu.abbreviations`,
//! which the launcher only reads), pinned from the launcher, and learned
//! from what gets picked while the field holds a query. The last two live
//! in `state.json` under `abbreviations`.
//!
//! A learned subject leads only when it dominates its query: two picks or
//! more, a decayed score of at least 1, and at least 60% of all the decayed
//! score the query has taught. Below that its score is a bonus inside its
//! tier, so a query used for two things ranks both higher without either
//! jumping.

use std::collections::{BTreeMap, HashMap};

use serde::{Deserialize, Serialize};

use crate::frecency::{self, Record};

pub const MAX_LEN: usize = 8;
/// The learned map keeps this many `(query, subject)` pairs, best first.
pub const MAX_PAIRS: usize = 500;
/// A learned record under this decayed score is dropped on the next write,
/// about 47 days after a single pick.
pub const DROP_BELOW: f64 = 0.1;
const DOMINANT_COUNT: f64 = 2.0;
const DOMINANT_SCORE: f64 = 1.0;
const DOMINANT_SHARE: f64 = 0.6;
/// The bonus a non-dominant learned subject gets: `min(BOOST_MAX, BOOST_PER * score)`.
const BOOST_PER: f64 = 50.0;
const BOOST_MAX: f64 = 150.0;

/// `state.json`'s `abbreviations`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Book {
    pub pinned: BTreeMap<String, String>,
    pub learned: BTreeMap<String, Vec<Record>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    Configured,
    Pinned,
    Learned,
}

impl Source {
    pub fn as_str(self) -> &'static str {
        match self {
            Source::Configured => "configured",
            Source::Pinned => "pinned",
            Source::Learned => "learned",
        }
    }
}

/// What one query's abbreviations say about the ranking.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Lookup {
    /// The subject the query names outright, and which source names it.
    pub lead: Option<(String, Source)>,
    /// Learned subjects that do not dominate, by subject id.
    pub boost: HashMap<String, i32>,
}

impl Lookup {
    /// Which source puts `subject` where it is, if any.
    pub fn source_of(&self, subject: &str) -> Option<Source> {
        match &self.lead {
            Some((id, source)) if id == subject => Some(*source),
            _ => self.boost.contains_key(subject).then_some(Source::Learned),
        }
    }
}

/// The query as an abbreviation: lowercased, 1 to 8 characters, not a
/// `:` prefix.
pub fn key(query: &str) -> Option<String> {
    let q = query.trim().to_lowercase();
    let n = q.chars().count();
    (n >= 1 && n <= MAX_LEN && !q.starts_with(':')).then_some(q)
}

/// A subject id as abbreviations name it: `app:<desktop id>` takes its
/// `.desktop` suffix or leaves it.
fn same_subject(a: &str, b: &str) -> bool {
    let trim = |s: &'_ str| -> String {
        match s.strip_prefix("app:") {
            Some(id) => format!("app:{}", id.strip_suffix(".desktop").unwrap_or(id)),
            None => s.to_owned(),
        }
    };
    trim(a) == trim(b)
}

fn canonical(id: &str) -> String {
    match id.strip_prefix("app:") {
        Some(rest) => format!("app:{}", rest.strip_suffix(".desktop").unwrap_or(rest)),
        None => id.to_owned(),
    }
}

/// The ranking's view of `query`. `resolves` says whether a subject id is
/// in the tree right now: a configured or pinned subject that is not falls
/// through to the next source, and its abbreviation stays.
pub fn lookup(configured: &HashMap<String, String>, book: &Book, query: &str, now: f64, resolves: impl Fn(&str) -> bool) -> Lookup {
    let Some(q) = key(query) else { return Lookup::default() };
    let mut out = Lookup::default();
    let set = match configured.get(&q) {
        Some(id) => Some((id, Source::Configured)),
        None => book.pinned.get(&q).map(|id| (id, Source::Pinned)),
    };
    if let Some((id, source)) = set {
        let id = canonical(id);
        if resolves(&id) {
            out.lead = Some((id, source));
        }
    }
    let records = book.learned.get(&q).map(Vec::as_slice).unwrap_or_default();
    let scored: Vec<(&Record, f64)> = records.iter().map(|r| (r, frecency::score_of(Some(r), now))).collect();
    let total: f64 = scored.iter().map(|(_, s)| s).sum();
    for (r, score) in scored {
        let id = canonical(&r.id);
        if out.lead.as_ref().is_some_and(|(lead, _)| *lead == id) || !resolves(&id) {
            continue;
        }
        let dominant = r.count >= DOMINANT_COUNT && score >= DOMINANT_SCORE && score >= DOMINANT_SHARE * total;
        if dominant && out.lead.is_none() {
            out.lead = Some((id, Source::Learned));
        } else {
            out.boost.insert(id, (BOOST_PER * score).min(BOOST_MAX).round() as i32);
        }
    }
    out
}

/// The book with `(query, subject)` taught once more, decayed records
/// dropped and the pairs capped. A record decayed past keeping goes before
/// the pick, so the pick starts it again from one.
pub fn learn(book: &Book, query: &str, subject: &str, now: f64) -> Book {
    let mut out = book.clone();
    prune(&mut out, now);
    if let Some(q) = key(query) {
        let records = out.learned.remove(&q).unwrap_or_default();
        let existing = records.iter().find(|r| same_subject(&r.id, subject)).map_or_else(|| canonical(subject), |r| r.id.clone());
        out.learned.insert(q, frecency::record(&records, &existing, now, Some(usize::MAX)));
    }
    prune(&mut out, now);
    out
}

/// `query` pinned to `subject`.
pub fn pin(book: &Book, query: &str, subject: &str) -> Book {
    let mut out = book.clone();
    if let Some(q) = key(query) {
        out.pinned.insert(q, canonical(subject));
    }
    out
}

/// `(query, subject)` gone from both the pinned and the learned map.
pub fn forget(book: &Book, query: &str, subject: &str) -> Book {
    let mut out = book.clone();
    let Some(q) = key(query) else { return out };
    if out.pinned.get(&q).is_some_and(|id| same_subject(id, subject)) {
        out.pinned.remove(&q);
    }
    if let Some(records) = out.learned.get_mut(&q) {
        records.retain(|r| !same_subject(&r.id, subject));
        if records.is_empty() {
            out.learned.remove(&q);
        }
    }
    out
}

fn prune(book: &mut Book, now: f64) {
    let score = |r: &Record| frecency::score_of(Some(r), now);
    for records in book.learned.values_mut() {
        records.retain(|r| score(r) >= DROP_BELOW);
    }
    book.learned.retain(|_, records| !records.is_empty());
    let pairs: usize = book.learned.values().map(Vec::len).sum();
    if pairs <= MAX_PAIRS {
        return;
    }
    let mut all: Vec<(String, String, f64)> =
        book.learned.iter().flat_map(|(q, rs)| rs.iter().map(|r| (q.clone(), r.id.clone(), score(r)))).collect();
    all.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal));
    let keep: std::collections::HashSet<(String, String)> = all.into_iter().take(MAX_PAIRS).map(|(q, id, _)| (q, id)).collect();
    for (q, records) in book.learned.iter_mut() {
        records.retain(|r| keep.contains(&(q.clone(), r.id.clone())));
    }
    book.learned.retain(|_, records| !records.is_empty());
}

/// `abbreviations()`'s answer: every source, the learned records with their
/// decayed scores.
pub fn report(configured: &HashMap<String, String>, book: &Book, now: f64) -> serde_json::Value {
    let configured: BTreeMap<&String, &String> = configured.iter().collect();
    let learned: BTreeMap<&String, Vec<serde_json::Value>> = book
        .learned
        .iter()
        .map(|(q, rs)| {
            let rows = rs
                .iter()
                .map(|r| {
                    let score = frecency::score_of(Some(r), now);
                    serde_json::json!({"id": r.id, "count": r.count, "lastMs": r.last_ms, "score": (score * 1000.0).round() / 1000.0})
                })
                .collect();
            (q, rows)
        })
        .collect();
    serde_json::json!({"configured": configured, "pinned": book.pinned, "learned": learned})
}
