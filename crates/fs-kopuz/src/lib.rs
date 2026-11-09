//! kopuzd's gRPC (`kopuz.v1.Kopuz`, `kopuz/crates/proto/proto/kopuz.proto`)
//! over its Unix socket, with the protobuf codec and the HTTP/2 client
//! written by hand so the shell links no gRPC stack.

pub mod api;
pub mod http2;
pub mod pb;

use std::ffi::OsString;
use std::path::PathBuf;
use std::time::Duration;

use async_io::Timer;
use futures_lite::future;

pub use api::{
    Capabilities, Client, Empty, Event, EventEnvelope, FavoriteRequest, Favorites, Key, LyricChunk, LyricLine, Lyrics,
    PlayerState, SourceInfo, SourceList, Subscription, Table, TrackInfo,
};
pub use http2::Error;

/// Where kopuzd listens, as kopuz's `client::default_socket_path` finds it.
pub fn socket_path() -> Option<PathBuf> {
    socket_path_from(|name| std::env::var_os(name))
}

/// `socket_path` over any environment. The `directories` crate kopuz uses
/// takes only absolute XDG values and needs a home directory either way
/// (it falls back to the passwd entry where `HOME` is unset; this does not).
pub fn socket_path_from(env: impl Fn(&str) -> Option<OsString>) -> Option<PathBuf> {
    let absolute = |name| env(name).map(PathBuf::from).filter(|p| p.is_absolute());
    let home = env("HOME").filter(|h| !h.is_empty()).map(PathBuf::from)?;
    let dir = absolute("XDG_RUNTIME_DIR")
        .or_else(|| absolute("XDG_CACHE_HOME"))
        .unwrap_or_else(|| home.join(".cache"));
    Some(dir.join("kopuz").join("kopuzd.sock"))
}

/// A client for kopuzd's socket, when it is there.
pub fn client() -> Option<Client> {
    socket_path().filter(|p| p.exists()).map(Client::new)
}

/// A unary call to a local socket answers in milliseconds; past this the
/// daemon is wedged and the call is abandoned.
pub const CALL_TIMEOUT: Duration = Duration::from_secs(3);

/// `fut`, or None past [`CALL_TIMEOUT`].
pub async fn timed<T>(fut: impl Future<Output = T>) -> Option<T> {
    future::or(async { Some(fut.await) }, async {
        Timer::after(CALL_TIMEOUT).await;
        None
    })
    .await
}

/// An MPRIS bus name Kopuz registers: `org.mpris.MediaPlayer2.kopuz`, or
/// `.kopuz.instance<pid>` for a second instance.
pub fn is_player_bus(bus: &str) -> bool {
    match bus.strip_prefix("org.mpris.MediaPlayer2.kopuz") {
        Some("") => true,
        Some(rest) => rest.strip_prefix(".instance").is_some_and(|pid| !pid.is_empty() && pid.bytes().all(|b| b.is_ascii_digit())),
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kopuz_bus_names() {
        assert!(is_player_bus("org.mpris.MediaPlayer2.kopuz"));
        assert!(is_player_bus("org.mpris.MediaPlayer2.kopuz.instance4242"));
        assert!(!is_player_bus("org.mpris.MediaPlayer2.kopuzz"));
        assert!(!is_player_bus("org.mpris.MediaPlayer2.kopuz.instance"));
        assert!(!is_player_bus("org.mpris.MediaPlayer2.mpv"));
    }
}
