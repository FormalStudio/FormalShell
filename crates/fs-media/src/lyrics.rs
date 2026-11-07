//! Pure lyrics glue: provider URL builders, LRC and paxsenix Apple Music
//! parsing, match scoring, the normalised line model and kopuz's own lit-set
//! and depth-of-field math, ported from kopuz's
//! crates/utils/src/lyrics/{lrc,paxsenix}.rs and
//! crates/components/src/playback/lyrics.rs. No network, disk or clock here:
//! the service owns every side effect.
//!
//! Every line, from whichever provider, normalises to one `Line`. `end` is
//! None when the provider gave none, `parent` is the index of the main line a
//! background line belongs to, and `estimated` is true when `words` were
//! synthesised (`synthesise_words`) rather than timed by a provider. `Line`
//! serialises to the shape the lyrics cache file holds (camelCase keys).
//!
//! LRC (`[mm:ss]`, `[mm:ss.xx]`, `[mm:ss.xxx]`) and enhanced LRC's inline
//! `<mm:ss.xx>` word stamps parse the same way: a line can carry several
//! leading `[..]` stamps, one entry per stamp, all sharing the line's text and
//! words; a stamp whose contents are not a bare number and a colon (`[ar:..]`,
//! `[offset:..]`) is skipped, and a line with no valid stamp contributes
//! nothing. Two entries landing on the same time merge: the first line's text
//! and word timing are kept unless it has no words and the second does, and
//! the second's text is folded onto a new line below the first's, wrapped in
//! parentheses unless it already is one (kopuz's append_translation).
//! plainLyrics and paxsenix's `plain` field are never read.
//!
//! `words` is a list of chunks, not pre-grouped words: for LRC, the raw text
//! between one `<..>` stamp and the next, trimmed; for paxsenix Apple rows, one
//! chunk per timed `text`/`backgroundText` part, also trimmed. `chunk_words`
//! groups a run of chunks joined by no whitespace (`joins_next`) into the word
//! the panel draws as one row; `chunk_end` and `chunk_progress` are the wipe's
//! span and its 0..1 fraction at a given position, capped at
//! `WIPE_MAX_SECONDS` for a provider's own stamps and uncapped for
//! synthesised ones.
//!
//! The lit-set functions take the playback position `t` as a plain argument;
//! every caller passes what `led_position` made of the player's clock.

use std::collections::HashMap;
use std::sync::LazyLock;

use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use unicode_normalization::UnicodeNormalization;

use fs_js::{
    encode_uri_component, is_space, round, parse_number, to_str, trim, utf16_len,
};

pub(crate) const INTERLUDE_MIN_SECONDS: f64 = 5.0;
pub const MISS_TTL_DAYS: f64 = 7.0;
pub(crate) const LINE_ASSUMED_SECONDS: f64 = 7.0;
pub(crate) const WORD_FALLBACK_SECONDS: f64 = 0.35;
/// A chunk runs until the next one starts, which over a pause or a line's own
/// tail can be far longer than the syllable itself. The wipe caps there so it
/// lands on the beat and holds instead of creeping through the silence.
pub(crate) const WIPE_MAX_SECONDS: f64 = 1.2;
/// How long the glow on a chunk takes to fade once the chunk's own span is
/// over, and the step it is reported in, so a decaying glow rewrites the
/// effect's properties twenty times rather than once a frame.
pub(crate) const GLOW_DECAY_SECONDS: f64 = 0.6;
pub(crate) const GLOW_QUANTUM: f64 = 0.05;
/// How far ahead of the player's own clock every lyrics reader works. An MPRIS
/// position is a single sample stamped when the player's D-Bus reply arrived
/// and extrapolated from there, and the frame drawn off it reaches the display
/// a refresh or two later.
pub(crate) const POSITION_LEAD_SECONDS: f64 = 0.1;

/// A main line ending and the next one starting within this long reads as one
/// continuous phrase rather than a gap; the earlier line (or its background)
/// stays lit across it.
pub(crate) const SEAMLESS_GAP_SECONDS: f64 = 3.0;

/// The rightbar ramp from kopuz: per-line-of-distance blur step and cap, in px,
/// sized for the panel's own (smaller) type.
pub(crate) const BLUR_STEP_PX: f64 = 1.1;
pub(crate) const BLUR_MAX_PX: f64 = 6.0;
pub const BLUR_QUANTUM_PX: f64 = 0.5;
/// The lit line rests this far down the viewport rather than at its centre.
pub(crate) const COMFORT_OFFSET_FRACTION: f64 = 0.42;

/// `lyrics_match_score`'s floor and the duration window: a candidate under the
/// floor is dropped outright, one further than this many seconds from the
/// track's own length loses regardless of how well its text matches.
pub(crate) const MATCH_SCORE_FLOOR: f64 = 55.0;
pub(crate) const DURATION_WINDOW_SECONDS: f64 = 12.0;

/// A latency past this is a misreport rather than a sink.
pub(crate) const OUTPUT_LATENCY_MAX_MS: f64 = 2000.0;

const DEPTH_OPACITY: [f64; 4] = [1.0, 0.7, 0.45, 0.25];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Word {
    pub time: f64,
    pub text: String,
    #[serde(default)]
    pub joins_next: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Line {
    pub time: f64,
    #[serde(default)]
    pub end: Option<f64>,
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub words: Vec<Word>,
    #[serde(default)]
    pub parent: Option<usize>,
    #[serde(default)]
    pub background: bool,
    #[serde(default)]
    pub opposite_turn: bool,
    #[serde(default)]
    pub estimated: bool,
    /// A synthetic note over an instrumental gap, spliced in by
    /// `display_lines`.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub interlude: bool,
}

impl Line {
    pub fn new(time: f64, end: Option<f64>, text: &str) -> Line {
        Line {
            time,
            end,
            text: text.to_string(),
            words: Vec::new(),
            parent: None,
            background: false,
            opposite_turn: false,
            estimated: false,
            interlude: false,
        }
    }
}

/// `x > 0`, false for NaN.
fn positive(x: f64) -> bool {
    x > 0.0
}

fn finite(v: Option<f64>) -> Option<f64> {
    v.filter(|x| x.is_finite())
}

/// Lowercased, with Latin accents dropped (NFD, then the Combining
/// Diacritical Marks block), so "Rosalía" and "ROSALIA" agree. Marks outside
/// that block stay: stripping Japanese dakuten would turn が into か.
fn fold(value: &str) -> String {
    value
        .nfd()
        .filter(|c| !('\u{300}'..='\u{36f}').contains(c))
        .nfc()
        .collect::<String>()
        .to_lowercase()
}

/// Folded and slugged on any non-alphanumeric run, Unicode letters kept: an
/// ASCII-only slug left every all-Japanese track of one length on one key,
/// sharing its lyrics and its miss. Capped so `<key>.json` stays a legal
/// file name.
pub fn cache_key(artist: &str, title: &str, album: &str, duration_seconds: f64) -> String {
    let raw = fold(&format!("{artist} {title} {album}"));
    let mut slug = String::new();
    for word in raw.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()) {
        if slug.len() + word.len() > CACHE_SLUG_MAX_BYTES {
            break;
        }
        if !slug.is_empty() {
            slug.push('-');
        }
        slug.push_str(word);
    }
    let slug = slug.as_str();
    let seconds = round(duration_seconds);
    let seconds = if seconds.is_finite() {
        seconds as i64
    } else {
        0
    };
    format!("{slug}-{seconds}")
}

const CACHE_SLUG_MAX_BYTES: usize = 200;

pub fn get_url(artist: &str, title: &str, album: &str, duration_seconds: f64) -> String {
    let mut url = format!(
        "https://lrclib.net/api/get?track_name={}&artist_name={}",
        encode_uri_component(title),
        encode_uri_component(artist)
    );
    if !album.is_empty() {
        url.push_str(&format!("&album_name={}", encode_uri_component(album)));
    }
    if duration_seconds.is_finite() && duration_seconds > 0.0 {
        url.push_str(&format!(
            "&duration={}",
            round(duration_seconds) as i64
        ));
    }
    url
}

pub fn search_url(artist: &str, title: &str) -> String {
    format!(
        "https://lrclib.net/api/search?track_name={}&artist_name={}",
        encode_uri_component(title),
        encode_uri_component(artist)
    )
}

/// iTunes' own search, the first paxsenix Apple Music step: "title artist" is
/// kopuz's own query order, entity/limit/country pinned to its values.
pub fn itunes_search_url(artist: &str, title: &str) -> String {
    let query = trim(&format!("{title} {artist}")).to_string();
    format!(
        "https://itunes.apple.com/search?term={}&entity=song&limit=8&country=US",
        encode_uri_component(&query)
    )
}

pub fn paxsenix_apple_lyrics_url(track_id: impl std::fmt::Display) -> String {
    format!(
        "https://lyrics.paxsenix.org/apple-music/lyrics?id={}",
        encode_uri_component(&track_id.to_string())
    )
}

pub fn paxsenix_youtube_search_url(artist: &str, title: &str) -> String {
    let query = trim(&format!("{title} {artist}")).to_string();
    format!(
        "https://lyrics.paxsenix.org/youtube/search?q={}",
        encode_uri_component(&query)
    )
}

pub fn paxsenix_youtube_lyrics_url(video_id: &str) -> String {
    format!(
        "https://lyrics.paxsenix.org/youtube/lyrics?id={}",
        encode_uri_component(video_id)
    )
}

fn usable_synced_from(entry: &Value) -> String {
    match entry.get("syncedLyrics") {
        Some(Value::String(s)) if !s.is_empty() && has_usable_timing(&parse_lrc(s)) => s.clone(),
        _ => String::new(),
    }
}

/// Handles /api/get's single object and /api/search's array of candidates the
/// same way: the first entry (the only one, for /api/get) whose syncedLyrics
/// parses with usable timing. A non-JSON body or a body with nothing usable is
/// "".
pub fn pick_synced(body_text: &str) -> String {
    let Ok(data) = serde_json::from_str::<Value>(body_text) else {
        return String::new();
    };
    match &data {
        Value::Array(entries) => entries
            .iter()
            .map(usable_synced_from)
            .find(|s| !s.is_empty())
            .unwrap_or_default(),
        other => usable_synced_from(other),
    }
}

static TIME_TAG: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^([0-9]+):([0-9]+(?:\.[0-9]+)?)$").unwrap());
static WORD_STAMP: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"<([0-9]+):([0-9]+(?:\.[0-9]+)?)>").unwrap());

fn stamp_seconds(minutes: &str, seconds: &str) -> f64 {
    minutes.parse::<f64>().unwrap_or(f64::NAN) * 60.0 + seconds.parse::<f64>().unwrap_or(f64::NAN)
}

/// A tag's raw contents (bracket stripped) as a time in seconds, or None for
/// anything that is not a bare "digits:digits[.digits]" pair, which is how a
/// metadata tag's letters (ar, ti, offset, ...) fail out.
fn parse_time_tag(tag: &str) -> Option<f64> {
    let m = TIME_TAG.captures(tag)?;
    Some(stamp_seconds(&m[1], &m[2]))
}

/// Every leading `[..]` group off a line, in order, plus whatever text follows
/// the last one. A metadata tag rides along in the tags too, since only the
/// caller knows which tags parsed as a time.
fn leading_tags(line: &str) -> (Vec<&str>, &str) {
    let mut tags = Vec::new();
    let mut rest = line;
    while let Some(inner) = rest.strip_prefix('[') {
        let Some(close) = inner.find(']') else {
            break;
        };
        tags.push(&inner[..close]);
        rest = &inner[close + 1..];
    }
    (tags, rest)
}

/// Enhanced LRC's inline `<mm:ss.xx>` stamps, kept as raw chunks rather than
/// pre-grouped into words: each tag owns the raw text up to the next tag (or
/// the end of the line), untrimmed, so a syllable split mid-word
/// ("<0:01.0>Hel<0:01.2>lo") is two chunks rather than two words. `joins_next`
/// is true only when this chunk's own raw text carries no trailing whitespace
/// and the very next raw chunk carries no leading whitespace and trims to
/// something: a chunk before a stray stamp with no text is never joined across
/// it even though that stamp is dropped from the returned list. Returns the
/// words plus the whole span's text (every raw chunk concatenated, then
/// trimmed once), since a mid-word split must not gain a space it never had.
fn parse_words(rest: &str) -> (Vec<Word>, String) {
    let matches: Vec<(f64, usize, usize)> = WORD_STAMP
        .captures_iter(rest)
        .map(|c| {
            let whole = c.get(0).expect("group 0 always present");
            (stamp_seconds(&c[1], &c[2]), whole.start(), whole.end())
        })
        .collect();
    if matches.is_empty() {
        return (Vec::new(), String::new());
    }

    let raw: Vec<(f64, &str)> = matches
        .iter()
        .enumerate()
        .map(|(i, &(time, _, end))| {
            let text_end = matches.get(i + 1).map_or(rest.len(), |m| m.1);
            (time, &rest[end..text_end])
        })
        .collect();

    let joined: String = raw.iter().map(|&(_, text)| text).collect();
    (raw_chunk_words(&raw), trim(&joined).to_string())
}

/// Raw timed chunks (seconds, untrimmed text) to words: a chunk trimming to
/// nothing is dropped, and `joins_next` holds only when this chunk's raw text
/// carries no trailing whitespace and the very next raw chunk carries no
/// leading whitespace and trims to something. Enhanced LRC's stamps and
/// kopuzd's `LyricChunk`s both arrive this way.
pub fn raw_chunk_words(raw: &[(f64, &str)]) -> Vec<Word> {
    let mut words = Vec::new();
    for (j, &(time, raw_text)) in raw.iter().enumerate() {
        let text = trim(raw_text);
        if text.is_empty() {
            continue;
        }
        let joins_next = raw.get(j + 1).is_some_and(|&(_, next)| {
            !trim(next).is_empty()
                && !raw_text.chars().next_back().is_some_and(is_space)
                && !next.chars().next().is_some_and(is_space)
        });
        words.push(Word {
            time,
            text: text.to_string(),
            joins_next,
        });
    }
    words
}

/// A merged-in translation line (an equal stamp, or a second raw line with no
/// stamp of its own) wraps in parentheses unless it already is one; an empty
/// translation contributes nothing.
fn append_translation(existing_text: &str, text: &str) -> String {
    let trimmed = trim(text);
    if trimmed.is_empty() {
        return existing_text.to_string();
    }
    let wrapped = if trimmed.starts_with('(') && trimmed.ends_with(')') {
        trimmed.to_string()
    } else {
        format!("({trimmed})")
    };
    if existing_text.is_empty() {
        wrapped
    } else {
        format!("{existing_text}\n{wrapped}")
    }
}

/// Lines sorted by time, equal times merged. A line with several leading
/// stamps yields one entry per stamp; a line with no valid stamp yields
/// nothing.
pub fn parse_lrc(text: &str) -> Vec<Line> {
    struct Entry {
        time: f64,
        text: String,
        words: Vec<Word>,
    }
    let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
    let mut entries: Vec<Entry> = Vec::new();
    for raw_line in normalized.split('\n') {
        let (tags, rest) = leading_tags(raw_line);
        let times: Vec<f64> = tags.iter().filter_map(|t| parse_time_tag(t)).collect();
        if times.is_empty() {
            continue;
        }
        let (words, words_text) = parse_words(rest);
        let line_text = if words.is_empty() {
            trim(rest).to_string()
        } else {
            words_text
        };
        for &time in &times {
            entries.push(Entry {
                time,
                text: line_text.clone(),
                words: words.clone(),
            });
        }
    }
    entries.sort_by(|a, b| {
        a.time
            .partial_cmp(&b.time)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let mut merged: Vec<Line> = Vec::new();
    for entry in entries {
        if let Some(last) = merged.last_mut().filter(|l| l.time == entry.time) {
            if last.words.is_empty() && !entry.words.is_empty() {
                last.words = entry.words;
            }
            last.text = append_translation(&last.text, &entry.text);
        } else {
            merged.push(Line {
                words: entry.words,
                ..Line::new(entry.time, None, &entry.text)
            });
        }
    }
    merged
}

/// One line is usable on its own (there is still a time to trigger on); several
/// lines need a genuine increase somewhere, not just a run of entries stuck on
/// the same time (a malformed or wholly-merged file). `parse_lrc`'s own output
/// is already sorted with equal times merged away, so any increase at all shows
/// up between neighbours.
pub fn has_usable_timing(lines: &[Line]) -> bool {
    match lines.len() {
        0 => false,
        1 => true,
        _ => lines.windows(2).any(|w| w[1].time > w[0].time),
    }
}

// paxsenix's punctuation rule (should_insert_apple_space): no space before the
// very first part, none after a part flagged `part: true` (it joins the next
// one with nothing between), and none before a leading punctuation character
// on the next part.
const APPLE_NO_SPACE_BEFORE: [char; 10] = [',', '.', '?', '!', ':', ';', ')', ']', '}', '\''];

fn paxsenix_needs_space(
    current_text: &str,
    previous_part_continues: bool,
    next_text: &str,
) -> bool {
    if current_text.is_empty() || previous_part_continues {
        return false;
    }
    match next_text.chars().next() {
        None => false,
        Some(first) => first != '\u{2019}' && !APPLE_NO_SPACE_BEFORE.contains(&first),
    }
}

fn number_field(v: &Value, key: &str) -> Option<f64> {
    v.get(key).and_then(Value::as_f64)
}

fn parts_of(v: Option<&Value>) -> &[Value] {
    match v {
        Some(Value::Array(a)) => a,
        _ => &[],
    }
}

/// One paxsenix `text`/`backgroundText` array to a line: the line's `text`
/// keeps every inserted space (trimmed once at the end), each timed part
/// becomes a chunk with its own text trimmed instead, and `joins_next` mirrors
/// the part's own `part` flag: a part marked `part: true` means the next part
/// joins it with no space. None for a row with nothing but blank parts.
fn paxsenix_parts_to_line(
    parts: &[Value],
    start_time: f64,
    end_time: Option<f64>,
    parent: Option<usize>,
    background: bool,
    opposite_turn: bool,
) -> Option<Line> {
    let mut text = String::new();
    let mut words = Vec::new();
    let mut previous_continues = false;
    for part in parts {
        let Some(part_text) = part.get("text").and_then(Value::as_str) else {
            continue;
        };
        if trim(part_text).is_empty() {
            continue;
        }
        let prefix = if paxsenix_needs_space(&text, previous_continues, part_text) {
            " "
        } else {
            ""
        };
        let display_text = format!("{prefix}{part_text}");
        text.push_str(&display_text);
        let continues = part.get("part").and_then(Value::as_bool) == Some(true);
        if let Some(timestamp) = number_field(part, "timestamp") {
            words.push(Word {
                time: timestamp / 1000.0,
                text: trim(&display_text).to_string(),
                joins_next: continues,
            });
        }
        previous_continues = continues;
    }
    let text = trim(&text).to_string();
    if text.is_empty() {
        return None;
    }
    Some(Line {
        words,
        parent,
        background,
        opposite_turn,
        ..Line::new(start_time, finite(end_time), &text)
    })
}

fn paxsenix_has_timing(rows: &[Value]) -> bool {
    let timed = |v: &Value| number_field(v, "timestamp").is_some_and(|t| t > 0.0);
    rows.iter().any(|row| {
        timed(row)
            || number_field(row, "endtime").is_some_and(|e| e > 0.0)
            || parts_of(row.get("text")).iter().any(timed)
            || parts_of(row.get("backgroundText")).iter().any(timed)
    })
}

/// paxsenix's `content` rows to display lines (paxsenix_apple_to_lines): a
/// row's `text` becomes a foreground line unless the row is `background` with
/// no `backgroundText` of its own, in which case it is the background line; a
/// row's `backgroundText`, when present, becomes its own `background` line
/// right after its parent with `parent` pointing back at it, starting at its
/// first timed part (or the row's own stamp with none). None when nothing in
/// `rows` carries real timing.
pub(crate) fn paxsenix_apple_to_lines(rows: &[Value]) -> Option<Vec<Line>> {
    if !paxsenix_has_timing(rows) {
        return None;
    }
    let mut lines: Vec<Line> = Vec::new();
    for row in rows {
        let row_start = number_field(row, "timestamp").map_or(f64::NAN, |t| t / 1000.0);
        let row_end = number_field(row, "endtime").map(|e| e / 1000.0);
        let opposite_turn = row.get("oppositeTurn").and_then(Value::as_bool) == Some(true);
        let background_parts = parts_of(row.get("backgroundText"));
        let has_background_text = !background_parts.is_empty();
        let main_is_background =
            row.get("background").and_then(Value::as_bool) == Some(true) && !has_background_text;
        let mut parent_index = None;

        if let Some(main) = paxsenix_parts_to_line(
            parts_of(row.get("text")),
            row_start,
            row_end,
            None,
            main_is_background,
            opposite_turn,
        ) {
            lines.push(main);
            if !main_is_background {
                parent_index = Some(lines.len() - 1);
            }
        }

        let background_start = background_parts
            .iter()
            .find_map(|p| number_field(p, "timestamp"))
            .map_or(row_start, |t| t / 1000.0);
        if let Some(background) = paxsenix_parts_to_line(
            background_parts,
            background_start,
            row_end,
            parent_index,
            true,
            opposite_turn,
        ) {
            lines.push(background);
        }
    }
    if lines.is_empty() { None } else { Some(lines) }
}

/// The paxsenix Apple Music lyrics endpoint's whole body
/// (paxsenix_apple_to_lyrics): `content` wins when it carries usable timing,
/// else the fallback is `lrc` parsed the same way lrclib's own text is.
/// `plain` is never read; a bad JSON body or a response with nothing usable is
/// empty.
pub fn from_paxsenix_apple(body_text: &str) -> Vec<Line> {
    let Ok(data) = serde_json::from_str::<Value>(body_text) else {
        return Vec::new();
    };
    if let Some(lines) =
        paxsenix_apple_to_lines(parts_of(data.get("content"))).filter(|l| has_usable_timing(l))
    {
        return lines;
    }
    let lrc = data.get("lrc").and_then(Value::as_str).unwrap_or("");
    if trim(lrc).is_empty() {
        return Vec::new();
    }
    let parsed = parse_lrc(lrc);
    if has_usable_timing(&parsed) {
        parsed
    } else {
        Vec::new()
    }
}

/// `lyrics_match_score`'s own tokeniser: folded (`fold`), "(feat."/"(ft."/
/// "(featuring" stripped (a featured artist shouldn't cost a match its score),
/// split on anything that isn't alphanumeric in Unicode's sense, as kopuz
/// does (an ASCII-only split scored every non-Latin title 0), duplicates
/// collapsed (the score is a token-set overlap, not a bag-of-words one).
fn match_tokens(value: &str) -> Vec<String> {
    let normalized = fold(value)
        .replace("(feat.", " ")
        .replace("(ft.", " ")
        .replace("(featuring", " ");
    let mut tokens: Vec<String> = Vec::new();
    for token in normalized.split(|c: char| !c.is_alphanumeric()) {
        if token.is_empty() || tokens.iter().any(|t| t == token) {
            continue;
        }
        tokens.push(token.to_string());
    }
    tokens
}

/// Twice the shared token count over the combined token count, as a
/// percentage: 0 when either side tokenises to nothing.
pub(crate) fn match_score(candidate: &str, query: &str) -> f64 {
    let candidate_tokens = match_tokens(candidate);
    let query_tokens = match_tokens(query);
    if candidate_tokens.is_empty() || query_tokens.is_empty() {
        return 0.0;
    }
    let shared = candidate_tokens
        .iter()
        .filter(|t| query_tokens.contains(t))
        .count();
    (2 * shared) as f64 * 100.0 / (candidate_tokens.len() + query_tokens.len()) as f64
}

/// A candidate's combined score, or None when it should be dropped outright:
/// below the match floor, or both a duration and a candidate length are known
/// and they differ by more than the window. A `duration_seconds` of 0 or NaN
/// reads as unknown.
fn rank_candidate(
    text_score: f64,
    duration_seconds: f64,
    candidate_seconds: Option<f64>,
) -> Option<f64> {
    if text_score < MATCH_SCORE_FLOOR {
        return None;
    }
    let mut duration_score = 0.0;
    if duration_seconds != 0.0
        && !duration_seconds.is_nan()
        && let Some(candidate) = finite(candidate_seconds)
    {
        let delta = (candidate - duration_seconds).abs();
        if delta > DURATION_WINDOW_SECONDS {
            return None;
        }
        duration_score = DURATION_WINDOW_SECONDS - delta;
    }
    Some(text_score + duration_score)
}

fn str_field<'a>(v: &'a Value, key: &str) -> &'a str {
    v.get(key).and_then(Value::as_str).unwrap_or("")
}

/// The best iTunes search hit for "title artist" against a track's own
/// duration (best_itunes_song): `trackTimeMillis` beats a pure text match, but
/// only within the window. None when nothing clears the floor.
pub fn best_itunes_song<'a>(
    songs: &'a [Value],
    query: &str,
    duration_seconds: f64,
) -> Option<&'a Value> {
    let mut best = None;
    let mut best_score = f64::NEG_INFINITY;
    for song in songs {
        let candidate = format!(
            "{} {}",
            str_field(song, "trackName"),
            str_field(song, "artistName")
        );
        let candidate_seconds = number_field(song, "trackTimeMillis").map(|ms| ms / 1000.0);
        let score = rank_candidate(
            match_score(&candidate, query),
            duration_seconds,
            candidate_seconds,
        );
        if let Some(score) = score.filter(|s| *s > best_score) {
            best_score = score;
            best = Some(song);
        }
    }
    best
}

/// "m:ss" or "h:mm:ss" to whole seconds (parse_colon_duration); None for
/// anything that isn't a run of colon-separated non-negative integers.
pub(crate) fn parse_colon_duration(duration: &str) -> Option<f64> {
    if duration.is_empty() {
        return None;
    }
    let mut total = 0.0;
    for part in duration.split(':') {
        if part.is_empty() || !part.chars().all(|c| c.is_ascii_digit()) {
            return None;
        }
        total = total * 60.0 + part.parse::<f64>().unwrap_or(f64::NAN);
    }
    Some(total)
}

/// best_youtube_result's own scoring: title+author text match, duration parsed
/// out of paxsenix's "m:ss" string.
pub fn best_youtube_result<'a>(
    results: &'a [Value],
    query: &str,
    duration_seconds: f64,
) -> Option<&'a Value> {
    let mut best = None;
    let mut best_score = f64::NEG_INFINITY;
    for result in results {
        let candidate = format!(
            "{} {}",
            str_field(result, "title"),
            str_field(result, "author")
        );
        let candidate_seconds = result
            .get("duration")
            .and_then(Value::as_str)
            .and_then(parse_colon_duration);
        let score = rank_candidate(
            match_score(&candidate, query),
            duration_seconds,
            candidate_seconds,
        );
        if let Some(score) = score.filter(|s| *s > best_score) {
            best_score = score;
            best = Some(result);
        }
    }
    best
}

/// lyrics_quality: 2 when any line carries more than one word chunk (syllable
/// or word timing), 1 for line timing alone, 0 for nothing usable.
pub fn quality(lines: &[Line]) -> u8 {
    if lines.is_empty() {
        0
    } else if lines.iter().any(|l| l.words.len() > 1) {
        2
    } else {
        1
    }
}

/// True for a result the chain can stop on immediately: the first quality-2
/// answer wins outright.
pub fn is_definitive(lines: &[Line]) -> bool {
    quality(lines) == 2
}

#[derive(Debug, Clone, PartialEq)]
pub struct Candidate {
    pub source: String,
    pub lines: Vec<Line>,
}

/// The best of several provider results: highest quality wins, a tie goes to
/// the earlier entry (the caller's own priority order), and quality 0 never
/// wins even when it's all there is.
pub fn pick_best(results: &[Candidate]) -> Option<&Candidate> {
    let mut best = None;
    let mut best_quality = 0;
    for candidate in results {
        let q = quality(&candidate.lines);
        if q == 0 {
            continue;
        }
        if best.is_none() || q > best_quality {
            best_quality = q;
            best = Some(candidate);
        }
    }
    best
}

/// Runs of chunks joined by `joins_next` grouped into one word each, so a
/// syllable-stamped file reads as several chunks per word and a word-stamped
/// one as one chunk per word.
pub fn chunk_words(words: &[Word]) -> Vec<&[Word]> {
    let mut out = Vec::new();
    let mut start = 0;
    for (i, word) in words.iter().enumerate() {
        if !word.joins_next {
            out.push(&words[start..=i]);
            start = i + 1;
        }
    }
    if start < words.len() {
        out.push(&words[start..]);
    }
    out
}

/// A chunk's own span ends where the next one starts; the last chunk of a line
/// has no next, so it runs to the line's end, and a line with no end of its
/// own (the last display entry) falls back to a chunk-sized fudge past its own
/// stamp (kopuz's chunk_end_time).
pub(crate) fn chunk_end(words: &[Word], index: usize, line_end: Option<f64>) -> f64 {
    if let Some(next) = words.get(index + 1) {
        return next.time;
    }
    if let Some(end) = finite(line_end) {
        return end;
    }
    words[index].time + WORD_FALLBACK_SECONDS
}

/// The 0..1 fraction of chunk `index`'s own wipe elapsed at `t`: 0 before its
/// stamp, 1 once its (capped) span has passed. The cap runs off the span's own
/// start rather than off `chunk_end`'s raw answer, so a chunk whose next stamp
/// (or line end) is far in the future still finishes its wipe at
/// `WIPE_MAX_SECONDS` instead of creeping toward it.
///
/// `estimated` is the line's own flag, and it turns the cap off. The cap is
/// there for a provider's own stamp held over a pause; synthesised chunks have
/// no such silence in them, since `synthesise_words` laid them across the
/// line's whole sung stretch by character count.
pub fn chunk_progress(
    words: &[Word],
    index: usize,
    line_end: Option<f64>,
    t: f64,
    estimated: bool,
) -> f64 {
    let Some(chunk) = words.get(index) else {
        return 0.0;
    };
    let start = chunk.time;
    let mut end = chunk_end(words, index, line_end);
    if !estimated {
        end = end.min(start + WIPE_MAX_SECONDS);
    }
    if end <= start {
        return if t >= start { 1.0 } else { 0.0 };
    }
    ((t - start) / (end - start)).clamp(0.0, 1.0)
}

/// The 0..1 glow on chunk `index` at `t`: full while the chunk is the one
/// being sung, then linear to 0 over `GLOW_DECAY_SECONDS`. Off its raw span
/// rather than the capped wipe: a chunk held over a pause keeps its glow for as
/// long as it is the chunk being sung.
pub fn chunk_glow(words: &[Word], index: usize, line_end: Option<f64>, t: f64) -> f64 {
    let Some(chunk) = words.get(index) else {
        return 0.0;
    };
    if t < chunk.time {
        return 0.0;
    }
    let end = chunk_end(words, index, line_end);
    let glow = if t <= end {
        1.0
    } else {
        1.0 - (t - end) / GLOW_DECAY_SECONDS
    };
    round(glow.clamp(0.0, 1.0) / GLOW_QUANTUM) * GLOW_QUANTUM
}

/// A line's own words if it has any (not synthesised), the whole main-line
/// run's span otherwise: from `line.time` to `line.end` when the provider gave
/// one, else to the next main line's own start.
///
/// Run this over the display set, never over the raw lines: the next main line
/// there is the next thing that takes the row over, which is the note over an
/// instrumental where one was marked and the next line of words where none
/// was, so the span is exactly how long this line stays lit and the wipe lands
/// as it goes dark. `LINE_ASSUMED_SECONDS` is the answer only for the last
/// line of a file, which has nothing after it to end it.
///
/// Skips an interlude and a merged translation line (its text carries a "\n",
/// and synthesising only the first physical line would desync the second's own
/// wipe).
pub fn synthesise_words(lines: &[Line]) -> Vec<Line> {
    let mut out = lines.to_vec();
    if lines.is_empty() {
        return out;
    }
    let main = main_line_indices(lines);
    for (idx, line) in lines.iter().enumerate() {
        if line.interlude || !line.words.is_empty() {
            continue;
        }
        if line.text.is_empty() || line.text.contains('\n') {
            continue;
        }
        let words: Vec<&str> = line
            .text
            .split(is_space)
            .filter(|w| !w.is_empty())
            .collect();
        if words.is_empty() {
            continue;
        }
        let span_end = match finite(line.end) {
            Some(end) => end,
            None => {
                next_main_line_start(lines, &main, idx).unwrap_or(line.time + LINE_ASSUMED_SECONDS)
            }
        };
        let span = span_end - line.time;
        let total_chars: usize = words.iter().map(|w| utf16_len(w)).sum();

        let mut words_out = Vec::with_capacity(words.len());
        let mut chars_so_far = 0;
        for w in &words {
            let time = if span > 0.0 {
                line.time + span * (chars_so_far as f64 / total_chars as f64)
            } else {
                line.time
            };
            words_out.push(Word {
                time,
                text: w.to_string(),
                joins_next: false,
            });
            chars_so_far += utf16_len(w);
        }
        out[idx] = Line {
            words: words_out,
            estimated: true,
            ..line.clone()
        };
    }
    out
}

/// The one position the lit set, the wipe and the interlude ramp are all read
/// against: the player's clock led by `POSITION_LEAD_SECONDS`, then held back
/// by `offset_seconds` (see `hold_seconds`), positive holding the lyrics back.
/// A NaN offset reads as 0. Every caller goes through here, so the pane and
/// `media lyrics` can never disagree about which line is lit.
pub fn led_position(position: f64, offset_seconds: f64) -> f64 {
    position + POSITION_LEAD_SECONDS
        - if offset_seconds.is_finite() {
            offset_seconds
        } else {
            0.0
        }
}

/// The hold `led_position` takes: the output's own latency while
/// `media.lyricsOffsetAuto` is on, with `media.lyricsOffsetMs` added on top
/// either way. A NaN latency or nudge reads as 0.
pub fn hold_seconds(auto: bool, latency_ms: f64, manual_ms: f64) -> f64 {
    let latency = if auto && latency_ms.is_finite() {
        latency_ms
    } else {
        0.0
    };
    let manual = if manual_ms.is_finite() {
        manual_ms
    } else {
        0.0
    };
    (latency + manual) / 1000.0
}

#[derive(Debug, Clone, PartialEq)]
pub struct Stream {
    pub id: i64,
    /// application.name, application.process.binary, application.id and
    /// node.name, the names `media::stream_owner` matches a player on.
    pub keys: [Option<String>; 4],
}

#[derive(Debug, Clone, PartialEq)]
pub struct LatencyGraph {
    pub streams: Vec<Stream>,
    /// Sink node ids by node.name.
    pub sinks: HashMap<String, i64>,
    /// Downstream latency in ms by node id.
    pub latency: HashMap<i64, f64>,
    pub default_sink: String,
}

fn js_number(v: Option<&Value>) -> f64 {
    match v {
        Some(Value::Number(n)) => n.as_f64().unwrap_or(f64::NAN),
        Some(Value::String(s)) => parse_number(s),
        Some(Value::Null) => 0.0,
        Some(Value::Bool(b)) => f64::from(u8::from(*b)),
        _ => f64::NAN,
    }
}

/// JS truthiness of a parsed JSON value.
fn is_truthy(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().is_some_and(|f| f != 0.0),
        Value::String(s) => !s.is_empty(),
        Value::Array(_) | Value::Object(_) => true,
    }
}

/// `Number(x) || 0`.
fn number_or_zero(v: Option<&Value>) -> f64 {
    let n = js_number(v);
    if n.is_nan() { 0.0 } else { n }
}

/// `pw-dump`'s whole graph, cut down to what the output latency needs: every
/// playback stream with the names `media::stream_owner` matches a player on,
/// each node's downstream latency in ms, sinks by node.name, and the default
/// sink's name out of the "default" metadata.
///
/// A port's Latency param with direction "Input" is the latency from that port
/// to the ear, PipeWire's own sum over everything downstream of it: a bluez5
/// sink puts its packet, codec and transport delay there, and a link carries it
/// up to the stream feeding it, through any loopback or filter chain in
/// between. It is quantum + rate + ns, the first two in frames of the graph
/// clock, so the settings metadata's quantum and rate turn them into time; a
/// forced value wins, as it does in the graph itself.
///
/// A node whose `id` is not an integer is left out.
pub fn latency_graph(text: &str) -> Option<LatencyGraph> {
    let Ok(Value::Array(objects)) = serde_json::from_str::<Value>(text) else {
        return None;
    };
    let (mut rate, mut quantum, mut force_rate, mut force_quantum) = (48000.0, 1024.0, 0.0, 0.0);
    let mut default_sink = String::new();
    for meta in &objects {
        if meta.get("type").and_then(Value::as_str) != Some("PipeWire:Interface:Metadata") {
            continue;
        }
        let Some(Value::Array(entries)) = meta.get("metadata") else {
            continue;
        };
        let meta_name = meta
            .get("props")
            .and_then(|p| p.get("metadata.name"))
            .and_then(Value::as_str)
            .unwrap_or("");
        for entry in entries {
            let key = entry.get("key").and_then(Value::as_str).unwrap_or("");
            if meta_name == "settings" {
                let n = js_number(entry.get("value"));
                if n > 0.0 {
                    match key {
                        "clock.rate" => rate = n,
                        "clock.quantum" => quantum = n,
                        "clock.force-rate" => force_rate = n,
                        "clock.force-quantum" => force_quantum = n,
                        _ => {}
                    }
                }
            } else if meta_name == "default"
                && key == "default.audio.sink"
                && let Some(value) = entry.get("value").filter(|v| is_truthy(v))
            {
                default_sink = value
                    .get("name")
                    .filter(|n| is_truthy(n))
                    .map(to_str)
                    .unwrap_or_default();
            }
        }
    }
    let rate = if force_rate != 0.0 { force_rate } else { rate };
    let quantum = if force_quantum != 0.0 {
        force_quantum
    } else {
        quantum
    };

    let mut graph = LatencyGraph {
        streams: Vec::new(),
        sinks: HashMap::new(),
        latency: HashMap::new(),
        default_sink,
    };
    for o in &objects {
        let Some(info) = o.get("info").filter(|i| is_truthy(i)) else {
            continue;
        };
        let props = info.get("props");
        let prop = |k: &str| props.and_then(|p| p.get(k)).filter(|v| !v.is_null());
        match o.get("type").and_then(Value::as_str) {
            Some("PipeWire:Interface:Node") => {
                let Some(id) = o.get("id").and_then(Value::as_i64) else {
                    continue;
                };
                let class = prop("media.class").map(to_str).unwrap_or_default();
                if class == "Stream/Output/Audio" {
                    let key = |k: &str| prop(k).map(to_str);
                    graph.streams.push(Stream {
                        id,
                        keys: [
                            key("application.name"),
                            key("application.process.binary"),
                            key("application.id"),
                            key("node.name"),
                        ],
                    });
                } else if class == "Audio/Sink"
                    && let Some(name) = prop("node.name")
                        .map(to_str)
                        .filter(|n| !n.is_empty())
                {
                    graph.sinks.insert(name, id);
                }
            }
            Some("PipeWire:Interface:Port") => {
                let params = match info.get("params").and_then(|p| p.get("Latency")) {
                    Some(Value::Array(a)) => a.as_slice(),
                    _ => &[],
                };
                for l in params {
                    if l.get("direction").and_then(Value::as_str) != Some("Input") {
                        continue;
                    }
                    let ms = (number_or_zero(l.get("minQuantum")) * quantum / rate
                        + number_or_zero(l.get("minRate")) / rate)
                        * 1000.0
                        + number_or_zero(l.get("minNs")) / 1e6;
                    let Some(node) = prop("node.id").and_then(Value::as_i64) else {
                        continue;
                    };
                    if graph.latency.get(&node).is_none_or(|&cur| ms > cur) {
                        graph.latency.insert(node, ms);
                    }
                }
            }
            _ => {}
        }
    }
    Some(graph)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LatencyVia {
    Stream,
    Sink,
    None,
}

impl LatencyVia {
    pub fn as_str(self) -> &'static str {
        match self {
            LatencyVia::Stream => "stream",
            LatencyVia::Sink => "sink",
            LatencyVia::None => "none",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OutputLatency {
    pub ms: u32,
    pub via: LatencyVia,
}

/// What the ear is behind the player by, for the playback streams `stream_ids`
/// names (the lyric's player's own, off `latency_graph`'s `streams`), else for
/// the sink `sink_name`, else the graph's default sink. The streams are read
/// first because theirs is the whole path to the sink they are actually routed
/// to; the sink stands in while the player has no stream up (paused past its
/// own teardown, say).
pub fn output_latency(
    graph: Option<&LatencyGraph>,
    stream_ids: &[i64],
    sink_name: &str,
) -> OutputLatency {
    let none = OutputLatency {
        ms: 0,
        via: LatencyVia::None,
    };
    let Some(graph) = graph else {
        return none;
    };
    let clamp = |ms: f64| round(ms.clamp(0.0, OUTPUT_LATENCY_MAX_MS)) as u32;
    let best = stream_ids
        .iter()
        .filter_map(|id| graph.latency.get(id).copied())
        .fold(-1.0_f64, f64::max);
    if best >= 0.0 {
        return OutputLatency {
            ms: clamp(best),
            via: LatencyVia::Stream,
        };
    }
    for name in [sink_name, graph.default_sink.as_str()] {
        if name.is_empty() {
            continue;
        }
        if let Some(ms) = graph.sinks.get(name).and_then(|id| graph.latency.get(id)) {
            return OutputLatency {
                ms: clamp(*ms),
                via: LatencyVia::Sink,
            };
        }
    }
    none
}

/// The foreground lines (main_line_indices): every non-background line, or
/// every line at all when the whole set is background (no foreground to anchor
/// on).
pub fn main_line_indices(lines: &[Line]) -> Vec<usize> {
    let foreground: Vec<usize> = (0..lines.len()).filter(|&i| !lines[i].background).collect();
    if foreground.is_empty() {
        (0..lines.len()).collect()
    } else {
        foreground
    }
}

/// The start time of the main line right after `line_index` in
/// `main_indices`, or None past the last one.
pub(crate) fn next_main_line_start(
    lines: &[Line],
    main_indices: &[usize],
    line_index: usize,
) -> Option<f64> {
    let position = main_indices.iter().position(|&i| i == line_index)?;
    let next = *main_indices.get(position + 1)?;
    lines.get(next).map(|l| l.time)
}

/// Whether `line` is lit at `t`: not yet started is never active; no end time
/// runs until the next main line starts (or forever, with none); within its
/// own end is active; past its end it stays lit only through the seamless gap
/// into a main line that starts soon enough after it.
pub(crate) fn line_active_at(line: &Line, t: f64, next_main_start: Option<f64>) -> bool {
    if t < line.time {
        return false;
    }
    let Some(end) = finite(line.end) else {
        return next_main_start.is_none_or(|next| t < next);
    };
    if t <= end {
        return true;
    }
    match next_main_start {
        None => false,
        Some(next) => next > end && next - end <= SEAMLESS_GAP_SECONDS && t < next,
    }
}

/// The last main line at or before `t` that is still active there, None when
/// none is (before the first line, or in a gap with no seamless carry).
///
/// `main_indices` can be one property binding behind `lines` for a single
/// evaluation (the pane's main-index and active-index bindings settle
/// separately on a track switch), so an index past the current array is a real
/// transient, skipped rather than indexed.
pub fn active_main_line_index(lines: &[Line], main_indices: &[usize], t: f64) -> Option<usize> {
    let mut result = None;
    for &index in main_indices {
        if index >= lines.len() {
            continue;
        }
        if lines[index].time > t {
            break;
        }
        if line_active_at(
            &lines[index],
            t,
            next_main_line_start(lines, main_indices, index),
        ) {
            result = Some(index);
        }
    }
    result
}

/// A background line carries its own timing and often outlasts the line it was
/// attached to, or overlaps the next one (Apple starts the next main row while
/// the backing vocal is still going). Judged on that timing alone: with no end
/// time of its own it runs until the next main line starts after it, None when
/// there is none.
pub(crate) fn background_line_bound(lines: &[Line], main_indices: &[usize], line: &Line) -> Option<f64> {
    if finite(line.end).is_some() {
        return None;
    }
    main_indices
        .iter()
        .filter(|&&i| i < lines.len())
        .map(|&i| lines[i].time)
        .find(|&start| start > line.time)
}

/// Every line lit at `t` besides `main_line_index` itself: a background line
/// judged on `background_line_bound`, any other (a duet's opposite line) only
/// while a main line is active at all.
pub fn active_secondary_lines(
    lines: &[Line],
    main_indices: &[usize],
    t: f64,
    main_line_index: Option<usize>,
) -> Vec<usize> {
    let mut result = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        if Some(i) == main_line_index {
            continue;
        }
        let next_start = if line.background {
            background_line_bound(lines, main_indices, line)
        } else {
            next_main_line_start(lines, main_indices, i)
        };
        if !line_active_at(line, t, next_start) {
            continue;
        }
        if line.background || main_line_index.is_some() {
            result.push(i);
        }
    }
    result
}

/// A line's own estimated end when it has none: its last word plus the chunk
/// fallback, or the assumed line length off its own start when it has no words
/// either. Synthesised words are not an answer here: they are spread over this
/// very estimate, so reading them back would report a line as ending wherever
/// its own fabricated last word happened to land and close a real instrumental
/// gap to nothing.
pub(crate) fn line_end_estimate(line: &Line) -> f64 {
    if let Some(end) = finite(line.end) {
        return end;
    }
    match line.words.last() {
        Some(last) if !line.estimated => last.time + WORD_FALLBACK_SECONDS,
        _ => line.time + LINE_ASSUMED_SECONDS,
    }
}

/// `lines` with a synthetic interlude line spliced in wherever a gap of
/// `INTERLUDE_MIN_SECONDS` or more opens between one main line's run (itself
/// plus any background lines riding along after it) and the next: before the
/// first line when it starts that late, and between two main lines whose gap
/// from the whole run's own estimated end is that wide (kopuz's
/// build_display_lines). Every original line's `parent` is remapped onto its
/// new index; nothing is mutated in place.
pub fn display_lines(lines: &[Line]) -> Vec<Line> {
    if lines.is_empty() {
        return Vec::new();
    }
    let main = main_line_indices(lines);
    struct Gap {
        at: usize,
        start: f64,
        end: f64,
    }
    let mut gaps: Vec<Gap> = Vec::new();

    // Nothing is lit before the first line, so the run-in is judged on the dark
    // rule alone rather than on kopuz's wider one.
    if !main.is_empty() && lines[main[0]].time > SEAMLESS_GAP_SECONDS {
        gaps.push(Gap {
            at: main[0],
            start: 0.0,
            end: lines[main[0]].time,
        });
    }

    for w in 0..main.len().saturating_sub(1) {
        let current = main[w];
        let next = main[w + 1];
        let next_start = lines[next].time;
        let mut gap_start = f64::NEG_INFINITY;
        // Whether the whole run (the main line and any background line riding
        // after it) carries an end of its own, which is what decides whether it
        // goes dark at all: `line_active_at` runs a line with no end until the
        // next main line starts, so such a run is never dark and needs no note
        // over it however wide the gap reads.
        let mut run_ends = true;
        for line in &lines[current..next] {
            if finite(line.end).is_none() {
                run_ends = false;
            }
            gap_start = gap_start.max(line_end_estimate(line));
        }
        gap_start = lines[current].time.max(gap_start.min(next_start));
        // Two rules, not one. kopuz's own threshold marks an instrumental
        // stretch, and the second one closes the hole it leaves: a run that
        // ends 3 to 5 seconds before the next line goes dark (the seamless
        // carry only reaches SEAMLESS_GAP_SECONDS) and would draw nothing at
        // all. Past this, the row that goes dark and the note that lights
        // answer one rule: the note's own start is the instant `line_active_at`
        // stops holding the line before it.
        if next_start - gap_start >= INTERLUDE_MIN_SECONDS
            || (run_ends && next_start - gap_start > SEAMLESS_GAP_SECONDS)
        {
            gaps.push(Gap {
                at: next,
                start: gap_start,
                end: next_start,
            });
        }
    }

    if gaps.is_empty() {
        return lines.to_vec();
    }

    let mut display: Vec<Line> = Vec::with_capacity(lines.len() + gaps.len());
    let mut remap = vec![0; lines.len()];
    let mut gap_index = 0;
    for (i, line) in lines.iter().enumerate() {
        while gap_index < gaps.len() && gaps[gap_index].at == i {
            let gap = &gaps[gap_index];
            display.push(Line {
                interlude: true,
                ..Line::new(gap.start, Some(gap.end), "")
            });
            gap_index += 1;
        }
        remap[i] = display.len();
        display.push(line.clone());
    }

    for entry in &mut display {
        if let Some(parent) = entry.parent {
            entry.parent = remap.get(parent).copied();
        }
    }
    display
}

/// The depth ramp at `distance` rows from the anchor, spread over `row_span`
/// rows instead of the table's own four entries: `row_span` is how many rows
/// the pane actually has room for on that side of the anchor, so the ramp
/// reaches its floor at the viewport's edge rather than three rows in with the
/// rest of the pane left holding rows nobody can read. The table is sampled
/// rather than replaced, and an omitted span reproduces the four entries
/// exactly.
pub fn depth_opacity(distance: f64, row_span: Option<f64>) -> f64 {
    let last = DEPTH_OPACITY.len() - 1;
    let span = finite(row_span).filter(|s| *s > 0.0).unwrap_or(last as f64);
    // Multiplied before the divide, so an integer distance against the default
    // span lands on a table entry exactly rather than a float hair away from
    // one.
    let at = (distance.abs() * last as f64 / span).min(last as f64);
    let low = at.floor() as usize;
    if low >= last {
        return DEPTH_OPACITY[last];
    }
    DEPTH_OPACITY[low] + (DEPTH_OPACITY[low + 1] - DEPTH_OPACITY[low]) * (at - low as f64)
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RowSpans {
    pub above: f64,
    pub below: f64,
}

/// How many rows fit between the anchored row's own resting place (the comfort
/// offset) and each end of a `viewport_height`-tall viewport, at `row_pitch` px
/// a row. The two ramps above and `blur_for` below span these rather than a
/// fixed row count, so a taller pane reads more of the song. Never under 1: a
/// pane with room for less than one row either side still has to put its
/// neighbours somewhere on the ramp.
pub fn row_spans(viewport_height: f64, row_pitch: f64) -> RowSpans {
    if !positive(viewport_height) || !positive(row_pitch) {
        return RowSpans {
            above: 1.0,
            below: 1.0,
        };
    }
    let top = viewport_height * COMFORT_OFFSET_FRACTION;
    RowSpans {
        above: f64::max(1.0, top / row_pitch),
        below: f64::max(1.0, (viewport_height - top) / row_pitch),
    }
}

/// The 0..1 fade a `height`-tall row starting at `top` carries for its
/// clearance from the ends of a `viewport_height`-tall viewport: 1 while it
/// clears both by its own height, ramping to 0 as either end reaches it. The
/// ramp is spent before the viewport's clip rather than across it, so the pane
/// never ends on half a glyph. `top` is the item's position after the column's
/// own travel.
///
/// `ramp_height` is how much clearance buys a full fade in, one row's pitch
/// rather than the row's own height: a line wrapped over three rows would
/// otherwise sit at half opacity for resting where every other line rests.
pub fn edge_fraction(top: f64, height: f64, viewport_height: f64, ramp_height: Option<f64>) -> f64 {
    if !positive(height) {
        return 0.0;
    }
    let ramp = finite(ramp_height).filter(|r| *r > 0.0).unwrap_or(height);
    let clearance = top.min(viewport_height - (top + height));
    (clearance / ramp).clamp(0.0, 1.0)
}

/// A line's depth-of-field blur for its distance (in display rows, not
/// seconds) from the anchor: kopuz's rightbar ramp, capped before the strength
/// scale is applied, quantised to `BLUR_QUANTUM_PX` so the effect's own cache
/// doesn't rebuild every frame over a sub-pixel change. `row_span` is the row
/// count the pane has room for on that side (`row_spans`), so the cap lands at
/// the viewport's edge; omitting it keeps the ramp's own `BLUR_STEP_PX` slope.
pub fn blur_for(distance: f64, strength_percent: f64, row_span: Option<f64>) -> f64 {
    let span = finite(row_span)
        .filter(|s| *s > 0.0)
        .unwrap_or(BLUR_MAX_PX / BLUR_STEP_PX);
    let capped = (distance.abs() / span).min(1.0) * BLUR_MAX_PX;
    let scaled = capped * (strength_percent / 100.0);
    round(scaled / BLUR_QUANTUM_PX) * BLUR_QUANTUM_PX
}

/// One laid-out text row of a chunk: its `y`, `height` and ink `width`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TextRow {
    pub y: f64,
    pub height: f64,
    pub width: f64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RowBand {
    pub top: f64,
    pub height: f64,
    pub width: f64,
}

/// A chunk's own text rows as bands over its box. A chunk wider than the pane
/// wraps inside its own text item, and the wipe over it has to cross that
/// break the way reading does. The bands returned tile the whole box top to
/// bottom (a band runs to the next row's own top, and the last to the box's
/// floor) so the mask never leaves a sliver of a glyph uncovered, and each
/// carries the row's own ink width for `row_wipe`.
pub fn chunk_row_bands(rows: &[TextRow], box_height: f64, box_width: f64) -> Vec<RowBand> {
    if rows.is_empty() {
        return vec![RowBand {
            top: 0.0,
            height: box_height,
            width: box_width,
        }];
    }
    rows.iter()
        .enumerate()
        .map(|(i, row)| {
            let top = if i == 0 { 0.0 } else { row.y };
            let bottom = rows.get(i + 1).map_or(box_height, |next| next.y);
            RowBand {
                top,
                height: f64::max(0.0, bottom - top),
                width: if row.width > 0.0 {
                    row.width
                } else {
                    box_width
                },
            }
        })
        .collect()
}

/// The 0..1 wipe on one band of a wrapped chunk at the chunk's own `progress`:
/// band N finishes at its own right end before band N+1 starts at its left, so
/// the lit run reads in reading order rather than as one horizontal cut across
/// every row of the block at once. The travel is weighted by each band's own
/// ink width, so the edge crosses a full row and a short last one at one speed
/// instead of spending the same time on each.
pub fn row_wipe(bands: &[RowBand], index: usize, progress: f64) -> f64 {
    if index >= bands.len() {
        return 0.0;
    }
    if bands.len() == 1 {
        return progress;
    }
    let total: f64 = bands.iter().map(|b| b.width.max(0.0)).sum();
    let own = bands[index].width.max(0.0);
    if !positive(total) || !positive(own) {
        return if progress >= 1.0 { 1.0 } else { 0.0 };
    }
    let before: f64 = bands[..index].iter().map(|b| b.width.max(0.0)).sum();
    ((progress * total - before) / own).clamp(0.0, 1.0)
}

/// The column `y` that rests an item starting at `item_y` with its own top
/// `COMFORT_OFFSET_FRACTION` down a `viewport_height`-tall viewport (kopuz's
/// comfort scroll offset, translated from a scrollTop into a translated
/// column's own y since the pane never has real scroll content). Anchored on
/// the item's top rather than its centre.
pub fn comfort_y(viewport_height: f64, item_y: f64) -> f64 {
    viewport_height * COMFORT_OFFSET_FRACTION - item_y
}

#[cfg(test)]
mod tests;
