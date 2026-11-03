//! The messages the shell sends and reads, by `kopuz.proto`'s field numbers,
//! and the calls that carry them.

use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};

use async_io::Async;

use crate::http2::{Call, Error};
use crate::pb::{DecodeError, Message, Reader, Writer};

/// Every empty message: the `Get*Request`s, `SubscribeRequest`, `Unit` and
/// `SetFavoriteResponse`, and the `MutationResult` `Next` answers, unread.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Empty;

impl Message for Empty {
    fn encode(&self, _: &mut Writer) {}

    fn decode(buf: &[u8]) -> Result<Self, DecodeError> {
        let mut r = Reader::new(buf);
        while r.field()?.is_some() {}
        Ok(Empty)
    }
}

/// The fields of `TrackInfo` the shell reads.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TrackInfo {
    pub key: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub duration_ms: Option<u64>,
}

impl Message for TrackInfo {
    fn encode(&self, w: &mut Writer) {
        w.string(1, &self.key);
        w.string(3, &self.title);
        w.string(4, &self.artist);
        w.string(5, &self.album);
        w.optional_uint64(7, self.duration_ms);
    }

    fn decode(buf: &[u8]) -> Result<Self, DecodeError> {
        let mut m = Self::default();
        let mut r = Reader::new(buf);
        while let Some((field, v)) = r.field()? {
            match field {
                1 => m.key = v.string()?,
                3 => m.title = v.string()?,
                4 => m.artist = v.string()?,
                5 => m.album = v.string()?,
                7 => m.duration_ms = Some(v.uint64()?),
                _ => {}
            }
        }
        Ok(m)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PlayerState {
    pub rev: u64,
    pub row: Option<TrackInfo>,
}

impl Message for PlayerState {
    fn encode(&self, w: &mut Writer) {
        w.uint64(1, self.rev);
        if let Some(row) = &self.row {
            w.message(14, row);
        }
    }

    fn decode(buf: &[u8]) -> Result<Self, DecodeError> {
        let mut m = Self::default();
        let mut r = Reader::new(buf);
        while let Some((field, v)) = r.field()? {
            match field {
                1 => m.rev = v.uint64()?,
                14 => m.row = Some(v.message()?),
                _ => {}
            }
        }
        Ok(m)
    }
}

/// `LibraryInvalidated.table`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Table {
    Favorites,
    /// The configured sources, which kopuzd invalidates when the active one moves.
    Servers,
    Other(i32),
}

impl Table {
    const FAVORITES: i32 = 4;
    const SERVERS: i32 = 6;

    fn from_i32(v: i32) -> Self {
        match v {
            Self::FAVORITES => Table::Favorites,
            Self::SERVERS => Table::Servers,
            _ => Table::Other(v),
        }
    }

    fn to_i32(self) -> i32 {
        match self {
            Table::Favorites => Self::FAVORITES,
            Table::Servers => Self::SERVERS,
            Table::Other(v) => v,
        }
    }
}

/// `Event`'s oneof, with every kind the shell does not follow as `Other`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    PlayerState(PlayerState),
    LibraryInvalidated(Table),
    Other,
}

const EVENT_PLAYER_STATE: u32 = 1;
const EVENT_LIBRARY_INVALIDATED: u32 = 6;

impl Message for Event {
    fn encode(&self, w: &mut Writer) {
        match self {
            Event::PlayerState(s) => w.message(EVENT_PLAYER_STATE, s),
            Event::LibraryInvalidated(t) => {
                let mut inner = Writer::default();
                inner.enumeration(1, t.to_i32());
                w.bytes(EVENT_LIBRARY_INVALIDATED, &inner.into_bytes());
            }
            Event::Other => {}
        }
    }

    fn decode(buf: &[u8]) -> Result<Self, DecodeError> {
        let mut m = Event::Other;
        let mut r = Reader::new(buf);
        while let Some((field, v)) = r.field()? {
            match field {
                EVENT_PLAYER_STATE => m = Event::PlayerState(v.message()?),
                EVENT_LIBRARY_INVALIDATED => {
                    let mut table = 0;
                    let mut inner = Reader::new(v.bytes()?);
                    while let Some((field, v)) = inner.field()? {
                        if field == 1 {
                            table = v.int32()?;
                        }
                    }
                    m = Event::LibraryInvalidated(Table::from_i32(table));
                }
                // A later member of a oneof replaces an earlier one.
                2..=13 => m = Event::Other,
                _ => {}
            }
        }
        Ok(m)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EventEnvelope {
    pub event: Event,
}

impl Message for EventEnvelope {
    fn encode(&self, w: &mut Writer) {
        w.message(2, &self.event);
    }

    fn decode(buf: &[u8]) -> Result<Self, DecodeError> {
        let mut event = Event::Other;
        let mut r = Reader::new(buf);
        while let Some((field, v)) = r.field()? {
            if field == 2 {
                event = v.message()?;
            }
        }
        Ok(EventEnvelope { event })
    }
}

/// `TrackRef`, `DontRecommendRequest`: a key alone in field 1.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Key {
    pub key: String,
}

impl Message for Key {
    fn encode(&self, w: &mut Writer) {
        w.string(1, &self.key);
    }

    fn decode(buf: &[u8]) -> Result<Self, DecodeError> {
        let mut m = Self::default();
        let mut r = Reader::new(buf);
        while let Some((field, v)) = r.field()? {
            if field == 1 {
                m.key = v.string()?;
            }
        }
        Ok(m)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LyricChunk {
    pub start_ms: u64,
    pub text: String,
}

impl Message for LyricChunk {
    fn encode(&self, w: &mut Writer) {
        w.uint64(1, self.start_ms);
        w.string(2, &self.text);
    }

    fn decode(buf: &[u8]) -> Result<Self, DecodeError> {
        let mut m = Self::default();
        let mut r = Reader::new(buf);
        while let Some((field, v)) = r.field()? {
            match field {
                1 => m.start_ms = v.uint64()?,
                2 => m.text = v.string()?,
                _ => {}
            }
        }
        Ok(m)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LyricLine {
    pub start_ms: u64,
    pub end_ms: Option<u64>,
    pub text: String,
    pub chunks: Vec<LyricChunk>,
    pub parent_line_index: Option<u32>,
    pub background: bool,
    pub opposite_turn: bool,
}

impl Message for LyricLine {
    fn encode(&self, w: &mut Writer) {
        w.uint64(1, self.start_ms);
        w.optional_uint64(2, self.end_ms);
        w.string(3, &self.text);
        for c in &self.chunks {
            w.message(4, c);
        }
        w.optional_uint64(5, self.parent_line_index.map(u64::from));
        w.bool(6, self.background);
        w.bool(7, self.opposite_turn);
    }

    fn decode(buf: &[u8]) -> Result<Self, DecodeError> {
        let mut m = Self::default();
        let mut r = Reader::new(buf);
        while let Some((field, v)) = r.field()? {
            match field {
                1 => m.start_ms = v.uint64()?,
                2 => m.end_ms = Some(v.uint64()?),
                3 => m.text = v.string()?,
                4 => m.chunks.push(v.message()?),
                5 => m.parent_line_index = Some(v.uint32()?),
                6 => m.background = v.bool()?,
                7 => m.opposite_turn = v.bool()?,
                _ => {}
            }
        }
        Ok(m)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Lyrics {
    pub plain: Option<String>,
    pub synced: Vec<LyricLine>,
}

impl Message for Lyrics {
    fn encode(&self, w: &mut Writer) {
        w.optional_string(1, self.plain.as_deref());
        for l in &self.synced {
            w.message(2, l);
        }
    }

    fn decode(buf: &[u8]) -> Result<Self, DecodeError> {
        let mut m = Self::default();
        let mut r = Reader::new(buf);
        while let Some((field, v)) = r.field()? {
            match field {
                1 => m.plain = Some(v.string()?),
                2 => m.synced.push(v.message()?),
                _ => {}
            }
        }
        Ok(m)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Favorites {
    pub refs: Vec<String>,
    pub generation: u64,
}

impl Message for Favorites {
    fn encode(&self, w: &mut Writer) {
        for r in &self.refs {
            w.bytes(1, r.as_bytes());
        }
        w.uint64(2, self.generation);
    }

    fn decode(buf: &[u8]) -> Result<Self, DecodeError> {
        let mut m = Self::default();
        let mut r = Reader::new(buf);
        while let Some((field, v)) = r.field()? {
            match field {
                1 => m.refs.push(v.string()?),
                2 => m.generation = v.uint64()?,
                _ => {}
            }
        }
        Ok(m)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FavoriteRequest {
    pub key: String,
    pub favorite: bool,
}

impl Message for FavoriteRequest {
    fn encode(&self, w: &mut Writer) {
        w.string(1, &self.key);
        w.bool(2, self.favorite);
    }

    fn decode(buf: &[u8]) -> Result<Self, DecodeError> {
        let mut m = Self::default();
        let mut r = Reader::new(buf);
        while let Some((field, v)) = r.field()? {
            match field {
                1 => m.key = v.string()?,
                2 => m.favorite = v.bool()?,
                _ => {}
            }
        }
        Ok(m)
    }
}

/// The field of `SourceCapabilities` the shell reads.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Capabilities {
    /// It takes a "stop recommending this" signal for a track.
    pub dont_recommend: bool,
}

impl Message for Capabilities {
    fn encode(&self, w: &mut Writer) {
        w.bool(17, self.dont_recommend);
    }

    fn decode(buf: &[u8]) -> Result<Self, DecodeError> {
        let mut m = Self::default();
        let mut r = Reader::new(buf);
        while let Some((field, v)) = r.field()? {
            if field == 17 {
                m.dont_recommend = v.bool()?;
            }
        }
        Ok(m)
    }
}

/// The fields of `SourceInfo` the shell reads.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SourceInfo {
    pub id: String,
    pub active: bool,
    pub capabilities: Capabilities,
}

impl Message for SourceInfo {
    fn encode(&self, w: &mut Writer) {
        w.string(1, &self.id);
        w.bool(5, self.active);
        w.message(7, &self.capabilities);
    }

    fn decode(buf: &[u8]) -> Result<Self, DecodeError> {
        let mut m = Self::default();
        let mut r = Reader::new(buf);
        while let Some((field, v)) = r.field()? {
            match field {
                1 => m.id = v.string()?,
                5 => m.active = v.bool()?,
                7 => m.capabilities = v.message()?,
                _ => {}
            }
        }
        Ok(m)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SourceList {
    pub sources: Vec<SourceInfo>,
}

impl SourceList {
    /// The source playing from, whose capabilities Kopuz's buttons follow.
    pub fn active(&self) -> Option<&SourceInfo> {
        self.sources.iter().find(|s| s.active)
    }
}

impl Message for SourceList {
    fn encode(&self, w: &mut Writer) {
        for s in &self.sources {
            w.message(1, s);
        }
    }

    fn decode(buf: &[u8]) -> Result<Self, DecodeError> {
        let mut m = Self::default();
        let mut r = Reader::new(buf);
        while let Some((field, v)) = r.field()? {
            if field == 1 {
                m.sources.push(v.message()?);
            }
        }
        Ok(m)
    }
}

/// kopuzd at one socket path. Every call opens its own connection, so calls
/// run side by side and a subscription never blocks one. None carries a
/// timeout: race it against a timer.
#[derive(Debug, Clone)]
pub struct Client {
    socket: PathBuf,
}

impl Client {
    pub fn new(socket: impl Into<PathBuf>) -> Self {
        Self { socket: socket.into() }
    }

    pub fn socket(&self) -> &Path {
        &self.socket
    }

    async fn unary<R: Message>(&self, method: &str, request: &impl Message) -> Result<R, Error> {
        let io = Async::<UnixStream>::connect(&self.socket).await?;
        let reply = Call::unary(io, method, &request.to_bytes()).await?;
        Ok(R::decode(&reply)?)
    }

    pub async fn player_state(&self) -> Result<PlayerState, Error> {
        self.unary("GetPlayerState", &Empty).await
    }

    pub async fn subscribe(&self) -> Result<Subscription, Error> {
        let io = Async::<UnixStream>::connect(&self.socket).await?;
        let call = Call::start(io, "Subscribe", &[]).await?;
        Ok(Subscription { call })
    }

    pub async fn lyrics(&self, key: &str) -> Result<Lyrics, Error> {
        self.unary("GetLyrics", &Key { key: key.to_owned() }).await
    }

    pub async fn favorites(&self) -> Result<Favorites, Error> {
        self.unary("GetFavorites", &Empty).await
    }

    pub async fn set_favorite(&self, key: &str, favorite: bool) -> Result<(), Error> {
        let request = FavoriteRequest { key: key.to_owned(), favorite };
        self.unary::<Empty>("SetFavorite", &request).await.map(drop)
    }

    pub async fn sources(&self) -> Result<SourceList, Error> {
        self.unary("GetSources", &Empty).await
    }

    pub async fn dont_recommend(&self, key: &str) -> Result<(), Error> {
        self.unary::<Empty>("DontRecommend", &Key { key: key.to_owned() }).await.map(drop)
    }

    /// The next track; the `MutationResult` it answers is not read.
    pub async fn next(&self) -> Result<(), Error> {
        self.unary::<Empty>("Next", &Empty).await.map(drop)
    }
}

pub struct Subscription {
    call: Call<Async<UnixStream>>,
}

impl Subscription {
    /// The next event, `None` once the daemon ends the stream. Cancel-safe.
    pub async fn next(&mut self) -> Result<Option<Event>, Error> {
        match self.call.message().await? {
            Some(m) => Ok(Some(EventEnvelope::decode(&m)?.event)),
            None => Ok(None),
        }
    }
}
