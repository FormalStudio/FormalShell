use std::ffi::OsString;
use std::path::PathBuf;

use fs_kopuz::pb::Message;
use fs_kopuz::*;

fn track() -> TrackInfo {
    TrackInfo {
        key: "t1".into(),
        title: "Song".into(),
        artist: "Me".into(),
        album: "LP".into(),
        duration_ms: Some(200_000),
    }
}

fn round_trip<M: Message + PartialEq + std::fmt::Debug>(m: M) {
    assert_eq!(M::decode(&m.to_bytes()).unwrap(), m);
}

#[test]
fn requests_encode() {
    assert_eq!(Empty.to_bytes(), b"");
    assert_eq!(Key { key: "abc".into() }.to_bytes(), b"\x0a\x03abc");
    assert_eq!(FavoriteRequest { key: "k1".into(), favorite: true }.to_bytes(), b"\x0a\x02k1\x10\x01");
    assert_eq!(FavoriteRequest { key: "k1".into(), favorite: false }.to_bytes(), b"\x0a\x02k1");
    assert_eq!(RateRequest { item_ref: "r".into(), rating: Rating::Dislike }.to_bytes(), b"\x0a\x01r\x10\x03");
    assert_eq!(RateRequest { item_ref: "r".into(), rating: Rating::Like }.to_bytes(), b"\x0a\x01r\x10\x02");
}

#[test]
fn player_state_decodes_past_unknown_fields() {
    let row: &[u8] = b"\x0a\x02t1\x12\x01u\x1a\x04Song\x22\x02Me\x2a\x02LP\x38\xc0\x9a\x0c\xd2\x01\x00";
    assert_eq!(row.len(), 0x1c);
    let mut bytes = b"\x08\x07\x18\x02\x45\x00\x00\x80\x3f\x72\x1c".to_vec();
    bytes.extend_from_slice(row);
    bytes.extend_from_slice(b"\x78\x01");
    assert_eq!(PlayerState::decode(&bytes).unwrap(), PlayerState { rev: 7, row: Some(track()) });
    assert_eq!(PlayerState::decode(b"").unwrap(), PlayerState::default());
}

#[test]
fn player_state_encodes() {
    let state = PlayerState { rev: 7, row: Some(track()) };
    assert_eq!(
        state.to_bytes(),
        b"\x08\x07\x72\x16\x0a\x02t1\x1a\x04Song\x22\x02Me\x2a\x02LP\x38\xc0\x9a\x0c".to_vec()
    );
    round_trip(state);
    round_trip(PlayerState { rev: 0, row: Some(TrackInfo::default()) });
}

#[test]
fn events_decode() {
    let decode = |b: &[u8]| EventEnvelope::decode(b).unwrap().event;
    assert_eq!(
        decode(b"\x08\x05\x12\x04\x0a\x02\x08\x01"),
        Event::PlayerState(PlayerState { rev: 1, row: None })
    );
    assert_eq!(decode(b"\x12\x04\x32\x02\x08\x04"), Event::LibraryInvalidated(Table::Favorites));
    assert_eq!(decode(b"\x12\x04\x32\x02\x08\x01"), Event::LibraryInvalidated(Table::Other(1)));
    assert_eq!(decode(b"\x12\x02\x12\x00"), Event::Other);
    assert_eq!(decode(b"\x12\x02\x62\x00"), Event::Other);
    assert_eq!(decode(b"\x12\x03\xf8\x01\x00"), Event::Other);
    assert_eq!(decode(b"\x12\x06\x0a\x02\x08\x01\x12\x00"), Event::Other);
    assert_eq!(decode(b""), Event::Other);
}

#[test]
fn events_encode() {
    let e = EventEnvelope { event: Event::LibraryInvalidated(Table::Favorites) };
    assert_eq!(e.to_bytes(), b"\x12\x04\x32\x02\x08\x04");
    let e = EventEnvelope { event: Event::PlayerState(PlayerState { rev: 1, row: None }) };
    assert_eq!(e.to_bytes(), b"\x12\x04\x0a\x02\x08\x01");
    round_trip(e);
    round_trip(EventEnvelope { event: Event::PlayerState(PlayerState { rev: 3, row: Some(track()) }) });
    round_trip(EventEnvelope { event: Event::LibraryInvalidated(Table::Other(9)) });
    round_trip(EventEnvelope { event: Event::Other });
}

fn lyrics() -> (Lyrics, Vec<u8>) {
    let lyrics = Lyrics {
        plain: Some("la".into()),
        synced: vec![LyricLine {
            start_ms: 1500,
            end_ms: Some(3000),
            text: "hi".into(),
            chunks: vec![
                LyricChunk { start_ms: 1500, text: "h".into() },
                LyricChunk { start_ms: 2000, text: "i".into() },
            ],
            parent_line_index: Some(0),
            background: true,
            opposite_turn: false,
        }],
    };
    let line: &[u8] =
        b"\x08\xdc\x0b\x10\xb8\x17\x1a\x02hi\x22\x06\x08\xdc\x0b\x12\x01h\x22\x06\x08\xd0\x0f\x12\x01i\x28\x00\x30\x01";
    assert_eq!(line.len(), 0x1e);
    let mut bytes = b"\x0a\x02la\x12\x1e".to_vec();
    bytes.extend_from_slice(line);
    (lyrics, bytes)
}

#[test]
fn lyrics_both_ways() {
    let (lyrics, bytes) = lyrics();
    assert_eq!(lyrics.to_bytes(), bytes);
    assert_eq!(Lyrics::decode(&bytes).unwrap(), lyrics);
    assert_eq!(Lyrics::decode(b"").unwrap(), Lyrics::default());
    let mut opposite = lyrics.clone();
    opposite.synced[0].opposite_turn = true;
    opposite.synced[0].parent_line_index = None;
    round_trip(opposite);
}

#[test]
fn favorites_both_ways() {
    let favorites = Favorites { refs: vec!["a".into(), "bc".into()], generation: 9 };
    let bytes = b"\x0a\x01a\x0a\x02bc\x10\x09";
    assert_eq!(favorites.to_bytes(), bytes);
    assert_eq!(Favorites::decode(bytes).unwrap(), favorites);
}

#[test]
fn requests_round_trip() {
    round_trip(Key { key: "abc".into() });
    round_trip(FavoriteRequest { key: "k".into(), favorite: true });
    round_trip(RateRequest { item_ref: "yt:1".into(), rating: Rating::None });
    assert_eq!(Empty::decode(b"\x08\x01\x12\x01x").unwrap(), Empty);
}

#[test]
fn unknown_rating_reads_unspecified() {
    let r = RateRequest::decode(b"\x10\x09").unwrap();
    assert_eq!(r.rating, Rating::Unspecified);
}

#[test]
fn malformed_input_is_an_error() {
    assert!(PlayerState::decode(b"\x08").is_err());
    assert!(PlayerState::decode(b"\x72\x05\x0a").is_err());
    assert!(Key::decode(b"\x0a\x01\xff").is_err());
    assert!(Key::decode(b"\x0b").is_err());
    assert!(Key::decode(b"\x0a\x01").is_err());
    assert!(Key::decode(b"\x08\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\x01").is_err());
    assert!(Key::decode(b"\x0d\x00\x00").is_err());
    assert!(Key::decode(b"\x08\x01").is_err());
    assert!(Key::decode(b"\x10\x01").is_ok());
    assert!(Key::decode(b"\x11\x01\x02\x03\x04\x05\x06\x07\x08").is_ok());
}

#[test]
fn socket_path_follows_kopuz() {
    let env = |pairs: &'static [(&'static str, &'static str)]| {
        move |name: &str| pairs.iter().find(|(k, _)| *k == name).map(|(_, v)| OsString::from(v))
    };
    let path = |p: &str| Some(PathBuf::from(p));
    assert_eq!(
        socket_path_from(env(&[("HOME", "/h"), ("XDG_RUNTIME_DIR", "/run/user/1000")])),
        path("/run/user/1000/kopuz/kopuzd.sock")
    );
    assert_eq!(
        socket_path_from(env(&[("HOME", "/h"), ("XDG_RUNTIME_DIR", "run"), ("XDG_CACHE_HOME", "/c")])),
        path("/c/kopuz/kopuzd.sock")
    );
    assert_eq!(socket_path_from(env(&[("HOME", "/h"), ("XDG_CACHE_HOME", "c")])), path("/h/.cache/kopuz/kopuzd.sock"));
    assert_eq!(socket_path_from(env(&[("XDG_RUNTIME_DIR", "/run/user/1000")])), None);
}
