//! Synced lyrics for the active track, looked up
//! when the track changes whether or not the media panel is open, so the
//! panel opens on its final width.
//!
//! The chain: a sibling `.lrc` beside the player's `xesam:url` (never
//! cached, the file can change under it), kopuzd's own lyrics when the
//! player is Kopuz and its row is this track (cached with source `kopuz`;
//! it may hold server or embedded lyrics no web provider has), the disk cache (`<key>.json` kept
//! forever, `<key>.miss` fresh for seven days), then paxsenix Apple Music,
//! paxsenix YouTube and lrclib raced. The first answer with real word
//! timing wins at once; otherwise every provider is waited for and the best
//! quality taken, ties to that order. A result reaches disk only when no
//! provider erred, so a weaker answer from an outage is kept for the session
//! alone. Every URL, parser and the line model are fs-media's.
//!
//! The output latency is read off the audio service's PipeWire graph while a
//! synced track has the automatic hold on: what the player's stream (or the
//! default sink) declares between the decoder and the ear.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, SystemTime};

use fs_media::lyrics::{self as model, Candidate, LatencyVia, Line, OutputLatency};
use fs_media::media::PlayerRow;
use futures_lite::future;
use serde_json::{Value, json};

use super::devices::audio;
use super::http;
use crate::runtime::Ctx;
use crate::store;

const USER_AGENT: &str = "User-Agent: FormalShell (https://github.com/FormalStudio/FormalShell)";

/// What a lookup is for.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Ask {
    pub enabled: bool,
    pub key: String,
    pub artist: String,
    pub title: String,
    pub album: String,
    pub length: f64,
    pub url: String,
    /// kopuzd's key for this track, when the player is Kopuz and its row
    /// matches. It can land after the track does, so it is part of what
    /// makes a new ask.
    pub kopuz: String,
}

/// Where a lookup stands, as `media lyrics` names it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Phase {
    #[default]
    Off,
    Idle,
    Loading,
    Synced,
    None,
    Error,
}

impl Phase {
    pub fn as_str(self) -> &'static str {
        match self {
            Phase::Off => "off",
            Phase::Idle => "idle",
            Phase::Loading => "loading",
            Phase::Synced => "synced",
            Phase::None => "none",
            Phase::Error => "error",
        }
    }
}

/// Which link of the chain answered.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Source {
    #[default]
    Unknown,
    Local,
    Kopuz,
    Cache,
    Apple,
    Youtube,
    Lrclib,
}

impl Source {
    pub fn as_str(self) -> &'static str {
        match self {
            Source::Unknown => "",
            Source::Local => "local",
            Source::Kopuz => "kopuz",
            Source::Cache => "cache",
            Source::Apple => "apple",
            Source::Youtube => "youtube",
            Source::Lrclib => "lrclib",
        }
    }

    fn provider(name: &str) -> Self {
        match name {
            "apple" => Source::Apple,
            "youtube" => Source::Youtube,
            "lrclib" => Source::Lrclib,
            _ => Source::Unknown,
        }
    }
}

#[derive(Clone, Debug)]
pub struct State {
    pub state: Phase,
    pub source: Source,
    pub key: String,
    /// What the panel draws: interludes spliced in, chunks synthesised.
    pub lines: Arc<Vec<Line>>,
    pub quality: u8,
    pub has_words: bool,
    /// The pane still follows the song; a wheel takes it off.
    pub follow: bool,
    pub latency: OutputLatency,
}

impl Default for State {
    fn default() -> Self {
        Self {
            state: Phase::Off,
            source: Source::Unknown,
            key: String::new(),
            lines: Arc::new(Vec::new()),
            quality: 0,
            has_words: false,
            follow: true,
            latency: OutputLatency { ms: 0, via: LatencyVia::None },
        }
    }
}

pub enum Diff {
    Resolved { key: String, state: Phase, source: Source, raw: Vec<Line> },
    Follow(bool),
    Latency(OutputLatency),
}

impl State {
    pub fn apply(&mut self, diff: Diff) -> bool {
        match diff {
            Diff::Resolved { key, state, source, raw } => {
                if key != self.key {
                    self.follow = true;
                }
                self.key = key;
                self.state = state;
                self.source = source;
                self.quality = model::quality(&raw);
                self.has_words = raw.iter().any(|l| !l.words.is_empty());
                self.lines = Arc::new(model::synthesise_words(&model::display_lines(&raw)));
                if state != Phase::Synced {
                    self.latency = OutputLatency { ms: 0, via: LatencyVia::None };
                }
                true
            }
            Diff::Follow(on) => std::mem::replace(&mut self.follow, on) != on,
            Diff::Latency(l) => std::mem::replace(&mut self.latency, l) != l,
        }
    }

    pub fn synced(&self) -> bool {
        self.state == Phase::Synced
    }
}

/// `media.lyricsOffsetAuto`, `media.lyricsOffsetMs`, `media.lyricsBlur` and
/// its strength, read the way LyricsService clamps them.
pub struct Settings {
    pub auto: bool,
    pub offset_ms: f64,
    pub blur: bool,
    pub strength: f64,
}

impl Settings {
    pub fn read(config: &super::config::State) -> Self {
        use crate::settings::{LYRICS_BLUR, LYRICS_OFFSET_AUTO, lyrics_blur_strength, lyrics_offset_ms};
        Self {
            auto: LYRICS_OFFSET_AUTO.get(config),
            offset_ms: lyrics_offset_ms(config),
            blur: LYRICS_BLUR.get(config),
            strength: lyrics_blur_strength(config),
        }
    }

    pub fn hold(&self, latency: OutputLatency) -> f64 {
        model::hold_seconds(self.auto, latency.ms as f64, self.offset_ms)
    }
}

static ASK: OnceLock<async_channel::Sender<Ask>> = OnceLock::new();
static LAST: Mutex<Option<Ask>> = Mutex::new(None);
/// What the latency read needs from the UI thread: the active player's row,
/// and whether a synced track with the automatic hold wants it at all.
static PLAYER: Mutex<Option<PlayerRow>> = Mutex::new(None);
static LATENCY_WANTED: AtomicBool = AtomicBool::new(false);
static LATENCY_POKE: OnceLock<async_channel::Sender<()>> = OnceLock::new();

/// The UI thread's side, after every media or config change: a lookup when
/// the track (or the switch) changed, and the latency read's inputs.
pub fn sync(media: &super::media::State, config: &super::config::State, lyrics: &State) {
    let enabled = crate::settings::LYRICS.get(config);
    let a = media.active();
    let ask = match &a {
        Some(a) if enabled && a.kind.has_timeline(!a.length.is_zero()) && !a.title.is_empty() && !a.artist.is_empty() => Ask {
            enabled,
            key: model::cache_key(&a.artist, &a.title, &a.album, a.length.as_secs_f64()),
            artist: a.artist.clone(),
            title: a.title.clone(),
            album: a.album.clone(),
            length: a.length.as_secs_f64(),
            url: media.url_of(&a.id),
            kopuz: media.kopuz().map(|k| k.key.clone()).unwrap_or_default(),
        },
        _ => Ask { enabled, ..Ask::default() },
    };
    if let Ok(mut last) = LAST.lock()
        && last.as_ref().is_none_or(|l| l.key != ask.key || l.enabled != ask.enabled || l.kopuz != ask.kopuz)
    {
        *last = Some(ask.clone());
        if let Some(tx) = ASK.get() {
            let _ = tx.try_send(ask);
        }
    }
    let row = a.as_ref().and_then(|a| media.players_rows().into_iter().find(|r| r.id == a.id));
    let auto = Settings::read(config).auto;
    let wanted = auto && lyrics.synced();
    let was = LATENCY_WANTED.swap(wanted, Ordering::Relaxed);
    let moved = PLAYER.lock().map(|mut p| std::mem::replace(&mut *p, row.clone()) != row).unwrap_or(false);
    if wanted && (!was || moved)
        && let Some(tx) = LATENCY_POKE.get()
    {
        let _ = tx.try_send(());
    }
}

fn cache_dir() -> PathBuf {
    crate::paths::cache().join("formalshell").join("lyrics")
}

enum Outcome {
    Hit(Vec<Line>),
    Miss,
    Error,
}

enum Step {
    Ok(String),
    Miss,
    Error,
}

async fn step(url: &str, max_time: u32) -> Step {
    match http::send(&http::Request::get(url, max_time).header(USER_AGENT)).await {
        Ok(r) if r.ok() => Step::Ok(r.text()),
        Ok(_) => Step::Miss,
        Err(_) => Step::Error,
    }
}

fn timed(lines: Vec<Line>) -> Outcome {
    if lines.is_empty() { Outcome::Miss } else { Outcome::Hit(lines) }
}

async fn apple(ask: Ask) -> Outcome {
    let query = format!("{} {}", ask.title, ask.artist).trim().to_owned();
    let songs = match step(&model::itunes_search_url(&ask.artist, &ask.title), 5).await {
        Step::Error => return Outcome::Error,
        Step::Miss => Vec::new(),
        Step::Ok(body) => serde_json::from_str::<Value>(&body).ok().and_then(|v| v.get("results").and_then(Value::as_array).cloned()).unwrap_or_default(),
    };
    let Some(id) = model::best_itunes_song(&songs, &query, ask.length).and_then(|s| s.get("trackId")).cloned() else { return Outcome::Miss };
    let id = match id {
        Value::Number(n) => n.to_string(),
        Value::String(s) => s,
        _ => return Outcome::Miss,
    };
    match step(&model::paxsenix_apple_lyrics_url(id), 10).await {
        Step::Error => Outcome::Error,
        Step::Miss => Outcome::Miss,
        Step::Ok(body) => timed(model::from_paxsenix_apple(&body)),
    }
}

fn lrc_lines(text: &str) -> Vec<Line> {
    let lines = model::parse_lrc(text);
    if model::has_usable_timing(&lines) { lines } else { Vec::new() }
}

async fn youtube(ask: Ask) -> Outcome {
    let query = format!("{} {}", ask.title, ask.artist).trim().to_owned();
    let results = match step(&model::paxsenix_youtube_search_url(&ask.artist, &ask.title), 5).await {
        Step::Error => return Outcome::Error,
        Step::Miss => Vec::new(),
        Step::Ok(body) => serde_json::from_str::<Value>(&body).ok().and_then(|v| v.as_array().cloned()).unwrap_or_default(),
    };
    let Some(id) = model::best_youtube_result(&results, &query, ask.length).and_then(|r| r.get("videoId")).and_then(Value::as_str).map(str::to_owned) else {
        return Outcome::Miss;
    };
    match step(&model::paxsenix_youtube_lyrics_url(&id), 3).await {
        Step::Error => Outcome::Error,
        Step::Miss => Outcome::Miss,
        Step::Ok(body) => timed(lrc_lines(&body)),
    }
}

fn lrclib_lines(body: &str) -> Vec<Line> {
    let text = model::pick_synced(body);
    if text.is_empty() { Vec::new() } else { model::parse_lrc(&text) }
}

async fn lrclib(ask: Ask) -> Outcome {
    match step(&model::get_url(&ask.artist, &ask.title, &ask.album, ask.length), 5).await {
        Step::Error => return Outcome::Error,
        Step::Ok(body) => {
            let lines = lrclib_lines(&body);
            if !lines.is_empty() {
                return Outcome::Hit(lines);
            }
        }
        Step::Miss => {}
    }
    match step(&model::search_url(&ask.artist, &ask.title), 5).await {
        Step::Error => Outcome::Error,
        Step::Miss => Outcome::Miss,
        Step::Ok(body) => timed(lrclib_lines(&body)),
    }
}

fn local(url: &str) -> Option<Vec<Line>> {
    let path = url.strip_prefix("file://")?;
    let path = super::cover::percent_decode(path);
    let path = PathBuf::from(path).with_extension("lrc");
    let lines = lrc_lines(&std::fs::read_to_string(path).ok()?);
    (!lines.is_empty()).then_some(lines)
}

fn seconds(ms: u64) -> f64 {
    ms as f64 / 1000.0
}

/// kopuzd's synced lines in the shell's model, one for one, so a line's
/// `parent_line_index` still points at its main line. Chunks arrive raw,
/// spaces and all, the way enhanced LRC's do.
pub fn kopuz_lines(lyrics: &fs_kopuz::Lyrics) -> Vec<Line> {
    lyrics
        .synced
        .iter()
        .map(|l| {
            let chunks: Vec<(f64, &str)> = l.chunks.iter().map(|c| (seconds(c.start_ms), c.text.as_str())).collect();
            Line {
                words: model::raw_chunk_words(&chunks),
                parent: l.parent_line_index.map(|i| i as usize),
                background: l.background,
                opposite_turn: l.opposite_turn,
                ..Line::new(seconds(l.start_ms), l.end_ms.map(seconds), fs_js::trim(&l.text))
            }
        })
        .collect()
}

async fn kopuz(key: &str) -> Option<Vec<Line>> {
    let client = fs_kopuz::client()?;
    let lyrics = fs_kopuz::timed(client.lyrics(key)).await?.ok()?;
    let lines = kopuz_lines(&lyrics);
    model::has_usable_timing(&lines).then_some(lines)
}

fn cached(key: &str) -> Option<Result<Vec<Line>, ()>> {
    let dir = cache_dir();
    if let Ok(text) = std::fs::read_to_string(dir.join(format!("{key}.json")))
        && let Ok(v) = serde_json::from_str::<Value>(&text)
        && let Some(lines) = v.get("lines").and_then(|l| serde_json::from_value::<Vec<Line>>(l.clone()).ok())
        && !lines.is_empty()
    {
        return Some(Ok(lines));
    }
    let fresh = std::fs::metadata(dir.join(format!("{key}.miss")))
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| SystemTime::now().duration_since(t).ok())
        .is_some_and(|age| age.as_secs_f64() < model::MISS_TTL_DAYS * 86400.0);
    fresh.then_some(Err(()))
}

fn write_atomic(path: PathBuf, body: &[u8]) {
    let tmp = path.with_extension("tmp");
    if std::fs::write(&tmp, body).and_then(|_| std::fs::rename(&tmp, &path)).is_err() {
        eprintln!("lyrics: could not write {}", path.display());
    }
}

type Session = HashMap<String, (Phase, Source, Vec<Line>)>;

fn publish(ctx: &Ctx, key: &str, state: Phase, source: Source, raw: Vec<Line>) {
    ctx.publish(store::Diff::Lyrics(Diff::Resolved { key: key.to_owned(), state, source, raw }));
}

fn remember(ctx: &Ctx, session: &mut Session, key: &str, state: Phase, source: Source, raw: Vec<Line>) {
    session.insert(key.to_owned(), (state, source, raw.clone()));
    publish(ctx, key, state, source, raw);
}

async fn resolve(ctx: &Ctx, ask: &Ask, session: &mut Session) {
    if !ask.enabled {
        return publish(ctx, "", Phase::Off, Source::Unknown, Vec::new());
    }
    if ask.key.is_empty() {
        return publish(ctx, "", Phase::Idle, Source::Unknown, Vec::new());
    }
    // A track resolved before kopuzd's key arrived asks kopuzd once more,
    // keeping what it shows meanwhile.
    let known = session.get(&ask.key).cloned();
    if let Some((state, source, lines)) = &known
        && (ask.kopuz.is_empty() || *source == Source::Kopuz || *source == Source::Local)
    {
        return publish(ctx, &ask.key, *state, *source, lines.clone());
    }
    let dir = cache_dir();
    if known.is_none() {
        publish(ctx, &ask.key, Phase::Loading, Source::Unknown, Vec::new());
        let url = ask.url.clone();
        if let Some(Some(lines)) = ctx.pool().run(move || local(&url)).await {
            return remember(ctx, session, &ask.key, Phase::Synced, Source::Local, lines);
        }
    }
    if !ask.kopuz.is_empty()
        && let Some(lines) = kopuz(&ask.kopuz).await
    {
        let path = dir.join(format!("{}.json", ask.key));
        let body = json!({ "source": "kopuz", "lines": lines }).to_string();
        ctx.pool().run(move || write_atomic(path, body.as_bytes())).await;
        return remember(ctx, session, &ask.key, Phase::Synced, Source::Kopuz, lines);
    }
    if let Some((state, source, lines)) = known {
        return publish(ctx, &ask.key, state, source, lines);
    }

    let key = ask.key.clone();
    match ctx.pool().run(move || cached(&key)).await {
        Some(Some(Ok(lines))) => return remember(ctx, session, &ask.key, Phase::Synced, Source::Cache, lines),
        Some(Some(Err(()))) => return remember(ctx, session, &ask.key, Phase::None, Source::Unknown, Vec::new()),
        _ => {}
    }

    let (tx, rx) = async_channel::unbounded();
    for (name, task) in [
        ("apple", Box::pin(apple(ask.clone())) as std::pin::Pin<Box<dyn std::future::Future<Output = Outcome>>>),
        ("youtube", Box::pin(youtube(ask.clone()))),
        ("lrclib", Box::pin(lrclib(ask.clone()))),
    ] {
        let tx = tx.clone();
        ctx.spawn(async move {
            let _ = tx.send((name, task.await)).await;
        });
    }
    drop(tx);
    let mut outcomes: HashMap<&str, Outcome> = HashMap::new();
    let mut decided: Option<Candidate> = None;
    while let Ok((name, outcome)) = rx.recv().await {
        if decided.is_none()
            && let Outcome::Hit(lines) = &outcome
            && model::is_definitive(lines)
        {
            decided = Some(Candidate { source: name.to_owned(), lines: lines.clone() });
            publish(ctx, &ask.key, Phase::Synced, Source::provider(name), lines.clone());
        }
        outcomes.insert(name, outcome);
    }
    let erred = outcomes.values().any(|o| matches!(o, Outcome::Error));
    let candidates: Vec<Candidate> = ["apple", "youtube", "lrclib"]
        .iter()
        .map(|n| Candidate {
            source: (*n).to_owned(),
            lines: match outcomes.get(n) {
                Some(Outcome::Hit(l)) => l.clone(),
                _ => Vec::new(),
            },
        })
        .collect();
    let winner = decided.or_else(|| model::pick_best(&candidates).cloned());
    match winner {
        None if erred => publish(ctx, &ask.key, Phase::Error, Source::Unknown, Vec::new()),
        None => {
            let path = dir.join(format!("{}.miss", ask.key));
            ctx.pool().run(move || write_atomic(path, b"")).await;
            remember(ctx, session, &ask.key, Phase::None, Source::Unknown, Vec::new());
        }
        Some(w) if erred => remember(ctx, session, &ask.key, Phase::Synced, Source::provider(&w.source), w.lines),
        Some(w) => {
            let path = dir.join(format!("{}.json", ask.key));
            let body = json!({ "source": w.source, "lines": w.lines }).to_string();
            ctx.pool().run(move || write_atomic(path, body.as_bytes())).await;
            remember(ctx, session, &ask.key, Phase::Synced, Source::provider(&w.source), w.lines);
        }
    }
}

fn read_latency(ctx: &Ctx) {
    let row = PLAYER.lock().ok().and_then(|p| p.clone());
    let latency = audio::with_graph(|g| {
        let streams = g
            .nodes
            .values()
            .filter(|n| n.is_stream() && n.is_sink())
            .filter(|n| row.as_ref().is_some_and(|r| !fs_media::media::stream_owner(&audio::stream_keys(n), std::slice::from_ref(r)).is_empty()))
            .filter_map(|n| g.latency_ms(n.id));
        let sink = g.nodes.values().find(|n| n.media_class == "Audio/Sink" && !n.name.is_empty() && n.name == g.defaults.sink).and_then(|n| g.latency_ms(n.id));
        model::output_latency(streams, sink)
    });
    ctx.publish(store::Diff::Lyrics(Diff::Latency(latency.unwrap_or_else(|| model::output_latency([], None)))));
}

async fn latency(ctx: Ctx) {
    let (tx, rx) = async_channel::unbounded();
    let _ = LATENCY_POKE.set(tx);
    loop {
        // A synced track re-reads every ten seconds, since a Bluetooth sink
        // revises its own transport delay mid-stream; a poke (a newly synced
        // track, another player) reads once things settle. With nothing
        // synced there is no timer at all, only the poke that starts one.
        let poked = if LATENCY_WANTED.load(Ordering::Relaxed) {
            future::or(async { rx.recv().await.is_ok() }, async {
                async_io::Timer::after(Duration::from_secs(10)).await;
                false
            })
            .await
        } else {
            rx.recv().await.is_ok()
        };
        if poked {
            async_io::Timer::after(Duration::from_millis(300)).await;
            while rx.try_recv().is_ok() {}
        }
        if LATENCY_WANTED.load(Ordering::Relaxed) {
            read_latency(&ctx);
        }
    }
}

pub fn start(ctx: &Ctx) {
    ctx.spawn(run(ctx.clone()));
}

async fn run(ctx: Ctx) {
    let (tx, rx) = async_channel::unbounded::<Ask>();
    let _ = ASK.set(tx);
    ctx.spawn(latency(ctx.clone()));
    let dir = cache_dir();
    ctx.pool()
        .run(move || {
            let _ = std::fs::create_dir_all(&dir);
            for entry in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
                let p = entry.path();
                if matches!(p.extension().and_then(|e| e.to_str()), Some("lrc" | "none")) {
                    let _ = std::fs::remove_file(p);
                }
            }
        })
        .await;
    let mut session = Session::new();
    let mut next = rx.recv().await.ok();
    while let Some(mut ask) = next.take() {
        while let Ok(newer) = rx.try_recv() {
            ask = newer;
        }
        // A newer track abandons the lookup in flight; its providers'
        // answers land on a closed channel.
        next = future::or(
            async {
                resolve(&ctx, &ask, &mut session).await;
                rx.recv().await.ok()
            },
            async { rx.recv().await.ok() },
        )
        .await;
    }
}

/// `media lyrics`.
pub fn status(lyrics: &State, position: f64, settings: &Settings) -> Value {
    let lines = &lyrics.lines;
    let main = model::main_line_indices(lines);
    let hold = settings.hold(lyrics.latency);
    let t = model::led_position(position, hold);
    let active = model::active_main_line_index(lines, &main, t);
    let secondary = model::active_secondary_lines(lines, &main, t, active);
    json!({
        "state": lyrics.state.as_str(),
        "source": lyrics.source.as_str(),
        "quality": lyrics.quality,
        "words": lyrics.has_words,
        "blur": settings.blur,
        "follow": lyrics.follow,
        "lines": &**lines,
        "active": active.map_or(json!(-1), |i| json!(i)),
        "secondary": secondary,
        "position": position,
        "latency": { "ms": lyrics.latency.ms, "via": lyrics.latency.via.as_str() },
        "hold": hold,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use fs_kopuz::{LyricChunk, LyricLine, Lyrics};
    use fs_media::lyrics::Word;

    fn chunk(start_ms: u64, text: &str) -> LyricChunk {
        LyricChunk { start_ms, text: text.into() }
    }

    #[test]
    fn kopuz_lines_keep_timing_parents_and_raw_chunk_joins() {
        let lyrics = Lyrics {
            plain: None,
            synced: vec![
                LyricLine {
                    start_ms: 1500,
                    end_ms: Some(3000),
                    text: " Hello world ".into(),
                    chunks: vec![chunk(1500, "Hel"), chunk(1700, "lo"), chunk(2000, " world")],
                    parent_line_index: None,
                    background: false,
                    opposite_turn: true,
                },
                LyricLine { start_ms: 2000, end_ms: None, text: "(ooh)".into(), chunks: vec![], parent_line_index: Some(0), background: true, opposite_turn: false },
            ],
        };
        let lines = kopuz_lines(&lyrics);
        assert_eq!(lines.len(), 2);
        assert_eq!((lines[0].time, lines[0].end, lines[0].text.as_str(), lines[0].opposite_turn), (1.5, Some(3.0), "Hello world", true));
        let word = |time, text: &str, joins_next| Word { time, text: text.into(), joins_next };
        assert_eq!(lines[0].words, vec![word(1.5, "Hel", true), word(1.7, "lo", false), word(2.0, "world", false)]);
        assert_eq!((lines[1].parent, lines[1].background, lines[1].end), (Some(0), true, None));
        assert!(model::has_usable_timing(&lines));
    }
}
