//! Synced lyrics for the active track, looked up a second
//! after the track changes whether or not the media panel is open, so the
//! panel opens on its final width.
//!
//! The chain: a sibling `.lrc` beside the player's `xesam:url` (never
//! cached, the file can change under it), the disk cache (`<key>.json` kept
//! forever, `<key>.miss` fresh for seven days), then paxsenix Apple Music,
//! paxsenix YouTube and lrclib raced. The first answer with real word
//! timing wins at once; otherwise every provider is waited for and the best
//! quality taken, ties to that order. A result reaches disk only when no
//! provider erred, so a weaker answer from an outage is kept for the session
//! alone. Every URL, parser and the line model are fs-media's.
//!
//! The output latency is read off `pw-dump` while a synced track has the
//! automatic hold on: what the player's stream (or the sink) declares
//! between the decoder and the ear.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, SystemTime};

use fs_media::lyrics::{self as model, Candidate, LatencyVia, Line, OutputLatency};
use fs_media::media::PlayerRow;
use futures_lite::future;
use serde_json::{Value, json};

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
}

#[derive(Clone, Debug)]
pub struct State {
    pub state: &'static str,
    pub source: String,
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
            state: "off",
            source: String::new(),
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
    Resolved { key: String, state: &'static str, source: String, raw: Vec<Line> },
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
                if state != "synced" {
                    self.latency = OutputLatency { ms: 0, via: LatencyVia::None };
                }
                true
            }
            Diff::Follow(on) => std::mem::replace(&mut self.follow, on) != on,
            Diff::Latency(l) => std::mem::replace(&mut self.latency, l) != l,
        }
    }

    pub fn synced(&self) -> bool {
        self.state == "synced"
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
        let int = |path: &str, fallback: f64, lo: f64, hi: f64| {
            let n = config.get(path).and_then(Value::as_f64).unwrap_or(fallback);
            if n.is_finite() { n.round().clamp(lo, hi) } else { fallback }
        };
        Self {
            auto: config.get("media.lyricsOffsetAuto") != Some(&Value::Bool(false)),
            offset_ms: int("media.lyricsOffsetMs", 0.0, -5000.0, 5000.0),
            blur: config.bool("media.lyricsBlur").unwrap_or(true),
            strength: int("media.lyricsBlurStrength", 100.0, 0.0, 200.0),
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
    let enabled = config.bool("media.lyrics").unwrap_or(true);
    let a = media.active();
    let ask = match &a {
        Some(a) if enabled && (a.kind == "mpris" || (a.kind == "iphone" && a.length > 0.0)) && !a.title.is_empty() && !a.artist.is_empty() => Ask {
            enabled,
            key: model::cache_key(&a.artist, &a.title, &a.album, a.length),
            artist: a.artist.clone(),
            title: a.title.clone(),
            album: a.album.clone(),
            length: a.length,
            url: media.url_of(&a.id),
        },
        _ => Ask { enabled, ..Ask::default() },
    };
    if let Ok(mut last) = LAST.lock()
        && last.as_ref().is_none_or(|l| l.key != ask.key || l.enabled != ask.enabled)
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
    let base = match std::env::var("XDG_CACHE_HOME") {
        Ok(d) if !d.is_empty() => PathBuf::from(d),
        _ => PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".cache"),
    };
    base.join("formalshell").join("lyrics")
}

enum Outcome {
    Hit(Vec<Line>),
    Miss,
    Error,
}

/// curl's exit code and stdout; 22 is the server's own miss (`--fail`).
async fn curl(url: &str, max_time: u32) -> (i32, String) {
    let out = crate::services::proc::command("curl")
        .args(["-sS", "--fail", "--max-time", &max_time.to_string(), "-H", USER_AGENT, url])
        .stdin(async_process::Stdio::null())
        .stderr(async_process::Stdio::null())
        .kill_on_drop(true)
        .output()
        .await;
    match out {
        Ok(o) => (o.status.code().unwrap_or(-1), String::from_utf8_lossy(&o.stdout).into_owned()),
        Err(_) => (-1, String::new()),
    }
}

enum Step {
    Ok(String),
    Miss,
    Error,
}

async fn step(url: &str, max_time: u32) -> Step {
    match curl(url, max_time).await {
        (0, body) => Step::Ok(body),
        (22, _) => Step::Miss,
        _ => Step::Error,
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

type Session = HashMap<String, (&'static str, String, Vec<Line>)>;

fn publish(ctx: &Ctx, key: &str, state: &'static str, source: &str, raw: Vec<Line>) {
    ctx.publish(store::Diff::Lyrics(Diff::Resolved { key: key.to_owned(), state, source: source.to_owned(), raw }));
}

fn remember(ctx: &Ctx, session: &mut Session, key: &str, state: &'static str, source: &str, raw: Vec<Line>) {
    session.insert(key.to_owned(), (state, source.to_owned(), raw.clone()));
    publish(ctx, key, state, source, raw);
}

async fn resolve(ctx: &Ctx, ask: &Ask, session: &mut Session) {
    if !ask.enabled {
        return publish(ctx, "", "off", "", Vec::new());
    }
    if ask.key.is_empty() {
        return publish(ctx, "", "idle", "", Vec::new());
    }
    if let Some((state, source, lines)) = session.get(&ask.key).cloned() {
        return publish(ctx, &ask.key, state, &source, lines);
    }
    publish(ctx, &ask.key, "loading", "", Vec::new());
    async_io::Timer::after(Duration::from_secs(1)).await;

    let (url, key) = (ask.url.clone(), ask.key.clone());
    let disk = ctx.pool().run(move || (local(&url), cached(&key))).await;
    match disk {
        Some((Some(lines), _)) => return remember(ctx, session, &ask.key, "synced", "local", lines),
        Some((None, Some(Ok(lines)))) => return remember(ctx, session, &ask.key, "synced", "cache", lines),
        Some((None, Some(Err(())))) => return remember(ctx, session, &ask.key, "none", "", Vec::new()),
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
            publish(ctx, &ask.key, "synced", name, lines.clone());
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
    let dir = cache_dir();
    match winner {
        None if erred => publish(ctx, &ask.key, "error", "", Vec::new()),
        None => {
            let path = dir.join(format!("{}.miss", ask.key));
            ctx.pool().run(move || write_atomic(path, b"")).await;
            remember(ctx, session, &ask.key, "none", "", Vec::new());
        }
        Some(w) if erred => remember(ctx, session, &ask.key, "synced", &w.source, w.lines),
        Some(w) => {
            let path = dir.join(format!("{}.json", ask.key));
            let body = json!({ "source": w.source, "lines": w.lines }).to_string();
            ctx.pool().run(move || write_atomic(path, body.as_bytes())).await;
            remember(ctx, session, &ask.key, "synced", &w.source, w.lines);
        }
    }
}

async fn read_latency(ctx: &Ctx) {
    let out = crate::services::proc::command("pw-dump").stdin(async_process::Stdio::null()).stderr(async_process::Stdio::null()).kill_on_drop(true).output().await;
    let text = match out {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).into_owned(),
        _ => String::new(),
    };
    let graph = if text.is_empty() { None } else { model::latency_graph(&text) };
    let row = PLAYER.lock().ok().and_then(|p| p.clone());
    let ids: Vec<i64> = match (&graph, &row) {
        (Some(g), Some(r)) => g
            .streams
            .iter()
            .filter(|s| {
                let keys: Vec<Option<&str>> = s.keys.iter().map(|k| k.as_deref()).collect();
                !fs_media::media::stream_owner(&keys, std::slice::from_ref(r)).is_empty()
            })
            .map(|s| s.id)
            .collect(),
        _ => Vec::new(),
    };
    ctx.publish(store::Diff::Lyrics(Diff::Latency(model::output_latency(graph.as_ref(), &ids, ""))));
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
            read_latency(&ctx).await;
        }
    }
}

pub async fn run(ctx: Ctx) {
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
        "state": lyrics.state,
        "source": lyrics.source,
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
