use std::io::{Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::thread;
use std::time::Duration;

use bytes::Bytes;
use fs_kopuz::pb::Message;
use fs_kopuz::*;
use futures_lite::future;
use h2::server::SendResponse;
use h2::{Reason, RecvStream};
use http::{HeaderMap, HeaderValue, Request, Response};

const STREAMED: u64 = 150;

fn socket(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("fs-kopuz-{}-{name}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("kopuzd.sock");
    let _ = std::fs::remove_file(&path);
    path
}

fn run<T>(f: impl Future<Output = T>) -> T {
    future::block_on(future::or(f, async {
        async_io::Timer::after(Duration::from_secs(20)).await;
        panic!("timed out");
    }))
}

fn grpc(message: &[u8]) -> Bytes {
    let mut out = vec![0];
    out.extend_from_slice(&(message.len() as u32).to_be_bytes());
    out.extend_from_slice(message);
    out.into()
}

fn h2_server(name: &str) -> Client {
    let path = socket(name);
    let listener = UnixListener::bind(&path).unwrap();
    listener.set_nonblocking(true).unwrap();
    thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        rt.block_on(async move {
            let listener = tokio::net::UnixListener::from_std(listener).unwrap();
            loop {
                let (sock, _) = listener.accept().await.unwrap();
                tokio::spawn(async move {
                    let mut conn = h2::server::handshake(sock).await.unwrap();
                    let mut held = Vec::new();
                    while let Some(Ok((req, respond))) = conn.accept().await {
                        if req.uri().path() == "/kopuz.v1.Kopuz/GetFavorites" {
                            conn.abrupt_shutdown(Reason::ENHANCE_YOUR_CALM);
                            held.push(respond);
                            continue;
                        }
                        tokio::spawn(answer(req, respond));
                    }
                });
            }
        });
    });
    Client::new(path)
}

async fn answer(req: Request<RecvStream>, mut respond: SendResponse<Bytes>) {
    assert_eq!(req.method(), "POST");
    assert_eq!(req.headers()["content-type"], "application/grpc");
    assert_eq!(req.headers()["te"], "trailers");
    let path = req.uri().path().to_owned();
    let mut body = req.into_body();
    let mut bytes = Vec::new();
    while let Some(chunk) = body.data().await {
        let chunk = chunk.unwrap();
        body.flow_control().release_capacity(chunk.len()).unwrap();
        bytes.extend_from_slice(&chunk);
    }
    assert_eq!(bytes[0], 0);
    assert_eq!(u32::from_be_bytes(bytes[1..5].try_into().unwrap()) as usize, bytes.len() - 5);
    let request = &bytes[5..];

    let head = Response::builder().status(200).header("content-type", "application/grpc").body(()).unwrap();
    let mut send = respond.send_response(head, false).unwrap();
    let mut status = "0";
    match path.as_str() {
        "/kopuz.v1.Kopuz/GetPlayerState" => {
            Empty::decode(request).unwrap();
            let state = PlayerState {
                rev: 4,
                row: Some(TrackInfo { key: "k".into(), title: "T".into(), ..Default::default() }),
            };
            send.send_data(grpc(&state.to_bytes()), false).unwrap();
        }
        "/kopuz.v1.Kopuz/GetLyrics" => {
            let key = Key::decode(request).unwrap().key;
            send.send_data(grpc(&Lyrics { plain: Some(key), synced: vec![] }.to_bytes()), false).unwrap();
        }
        "/kopuz.v1.Kopuz/SetFavorite" => {
            assert_eq!(
                FavoriteRequest::decode(request).unwrap(),
                FavoriteRequest { key: "k".into(), favorite: true }
            );
            send.send_data(grpc(&[]), false).unwrap();
        }
        "/kopuz.v1.Kopuz/Subscribe" => {
            for rev in 0..STREAMED {
                let row = TrackInfo { title: "x".repeat(1000), ..Default::default() };
                let event = EventEnvelope { event: Event::PlayerState(PlayerState { rev, row: Some(row) }) };
                send.send_data(grpc(&event.to_bytes()), false).unwrap();
            }
        }
        _ => status = "12",
    }
    let mut trailers = HeaderMap::new();
    trailers.insert("grpc-status", HeaderValue::from_static(status));
    send.send_trailers(trailers).unwrap();
}

#[test]
fn unary_calls() {
    let client = h2_server("unary");
    run(async {
        let state = client.player_state().await.unwrap();
        assert_eq!(state.rev, 4);
        assert_eq!(state.row.unwrap().title, "T");
        assert_eq!(client.lyrics("abc").await.unwrap().plain.as_deref(), Some("abc"));
        client.set_favorite("k", true).await.unwrap();
        assert!(matches!(client.dont_recommend("k").await, Err(Error::NoMessage)));
    });
}

#[test]
fn stream_runs_past_the_initial_window() {
    let client = h2_server("stream");
    run(async {
        let mut sub = client.subscribe().await.unwrap();
        let mut total = 0;
        for rev in 0..STREAMED {
            match sub.next().await.unwrap() {
                Some(Event::PlayerState(s)) => {
                    assert_eq!(s.rev, rev);
                    total += s.row.unwrap().title.len();
                }
                other => panic!("{other:?}"),
            }
        }
        assert!(total > 2 * 65_535);
        assert_eq!(sub.next().await.unwrap(), None);
    });
}

#[test]
fn goaway_fails_the_call() {
    let client = h2_server("goaway");
    run(async {
        let err = client.favorites().await.unwrap_err();
        assert!(matches!(err, Error::GoAway(code) if code == u32::from(Reason::ENHANCE_YOUR_CALM)), "{err:?}");
    });
}

fn read_frame(s: &mut UnixStream) -> Option<(u8, u8, u32, Vec<u8>)> {
    let mut head = [0u8; 9];
    s.read_exact(&mut head).ok()?;
    let len = u32::from_be_bytes([0, head[0], head[1], head[2]]) as usize;
    let mut payload = vec![0; len];
    s.read_exact(&mut payload).ok()?;
    Some((head[3], head[4], u32::from_be_bytes(head[5..9].try_into().unwrap()), payload))
}

fn frame(kind: u8, flags: u8, stream: u32, payload: &[u8]) -> Vec<u8> {
    let mut out = (payload.len() as u32).to_be_bytes()[1..].to_vec();
    out.extend_from_slice(&[kind, flags]);
    out.extend_from_slice(&stream.to_be_bytes());
    out.extend_from_slice(payload);
    out
}

/// A hand-driven peer: PING answered, SETTINGS acked, a padded DATA frame
/// read and its whole length handed back on the connection window.
#[test]
fn ping_settings_and_padding() {
    let path = socket("raw");
    let listener = UnixListener::bind(&path).unwrap();
    let favorites = Favorites { refs: vec!["a".into()], generation: 2 };
    let reply = favorites.to_bytes();
    let peer = thread::spawn(move || {
        let (mut s, _) = listener.accept().unwrap();
        let mut preface = [0u8; 24];
        s.read_exact(&mut preface).unwrap();
        assert_eq!(&preface, b"PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n");
        s.write_all(&frame(0x4, 0, 0, &[])).unwrap();
        s.write_all(&frame(0x6, 0, 0, b"pingpong")).unwrap();
        let mut seen = Vec::new();
        while !seen.iter().any(|(k, f, _, _)| *k == 0x6 && *f == 0x1) || !seen.iter().any(|(k, f, _, _)| *k == 0x4 && *f == 0x1) {
            seen.push(read_frame(&mut s).unwrap());
        }
        let ping = seen.iter().find(|(k, f, _, _)| *k == 0x6 && *f == 0x1).unwrap();
        assert_eq!(ping.3, b"pingpong");
        s.write_all(&frame(0x1, 0x4, 1, &[0x88])).unwrap();
        let mut data = vec![3];
        data.extend_from_slice(&grpc(&reply));
        data.extend_from_slice(&[0, 0, 0]);
        s.write_all(&frame(0x0, 0x1 | 0x8, 1, &data)).unwrap();
        let mut updates = Vec::new();
        while let Some(f) = read_frame(&mut s) {
            if f.0 == 0x8 {
                updates.push((f.2, u32::from_be_bytes(f.3[..4].try_into().unwrap())));
            }
        }
        assert_eq!(updates, [(0, data.len() as u32)]);
    });
    let client = Client::new(path);
    assert_eq!(run(client.favorites()).unwrap(), favorites);
    drop(client);
    peer.join().unwrap();
}
