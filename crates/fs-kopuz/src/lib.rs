//! kopuzd's gRPC (`kopuz.v1.Kopuz`, `kopuz/crates/proto/proto/kopuz.proto`)
//! over its Unix socket, with the protobuf codec and the HTTP/2 client
//! written by hand so the shell links no gRPC stack.

pub mod api;
pub mod http2;
pub mod pb;

use std::ffi::OsString;
use std::path::PathBuf;

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
