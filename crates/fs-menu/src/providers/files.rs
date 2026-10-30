//! The Files source (launcher spec §7), which keeps no index: GTK's
//! recent-files list and the XDG user directories as file and folder
//! subjects, a folder's own listing for →, and what the preview pane reads
//! off a file. The shell reads the disk on its service thread and hands
//! what it found in here.

use std::collections::HashSet;

use chrono::{DateTime, Local};

use crate::node::{Kind, Node};
use crate::thumbnails::path_key;

pub const FILES_ROUTE: &str = "files";
pub const FILE_KIND: &str = "file";
pub const FOLDER_KIND: &str = "folder";
/// How many of the recent list's newest files rank.
pub const RECENT_FILES: usize = 100;
/// A folder lists at most this many visible entries, and as many hidden.
pub const LISTING_MAX: usize = 500;
/// A text file's preview: its first lines, out of at most this much of it.
pub const PEEK_LINES: usize = 40;
pub const PEEK_BYTES: usize = 64 * 1024;
/// A folder's preview names this many of its entries.
pub const PEEK_NAMES: usize = 12;

/// The local path a `file://` URI names, percent-decoded; `None` for any
/// other scheme, a remote host or a path that is not UTF-8.
pub fn file_path(uri: &str) -> Option<String> {
    let rest = uri.strip_prefix("file://")?;
    let rest = match rest.find('/') {
        Some(0) => rest,
        Some(i) if &rest[..i] == "localhost" => &rest[i..],
        _ => return None,
    };
    let path = percent_encoding::percent_decode_str(rest).decode_utf8().ok()?;
    Some(path.into_owned())
}

/// A bookmark's last use: `modified`, else `visited`, else `added`, as
/// milliseconds; 0 when none parses.
fn stamp(node: roxmltree::Node) -> i64 {
    ["modified", "visited", "added"]
        .iter()
        .filter_map(|k| node.attribute(*k))
        .find_map(|v| DateTime::parse_from_rfc3339(v).ok())
        .map_or(0, |d| d.timestamp_millis())
}

/// The local paths `recently-used.xbel` lists, most recently used first,
/// each once.
pub fn recent_paths(xbel: &str) -> Vec<String> {
    let Ok(doc) = roxmltree::Document::parse(xbel) else { return Vec::new() };
    let mut found: Vec<(i64, String)> = doc
        .descendants()
        .filter(|n| n.has_tag_name("bookmark"))
        .filter_map(|n| Some((stamp(n), file_path(n.attribute("href")?)?)))
        .collect();
    found.sort_by_key(|a| std::cmp::Reverse(a.0));
    let mut seen = HashSet::new();
    found.into_iter().map(|(_, p)| p).filter(|p| seen.insert(p.clone())).collect()
}

/// The directories `user-dirs.dirs` names, in its order: `$HOME/...` or an
/// absolute path, quoted. One set to `$HOME` itself (how xdg-user-dirs
/// disables a directory) is left out.
pub fn user_dirs(text: &str, home: &str) -> Vec<String> {
    let home = home.trim_end_matches('/');
    let mut seen = HashSet::new();
    text.lines()
        .filter_map(|line| {
            let (key, value) = line.trim().split_once('=')?;
            if !(key.starts_with("XDG_") && key.ends_with("_DIR")) {
                return None;
            }
            let value = value.trim().strip_prefix('"')?.strip_suffix('"')?;
            let path = match value.strip_prefix("$HOME") {
                Some(rest) if rest.is_empty() || rest.starts_with('/') => format!("{home}{rest}"),
                Some(_) => return None,
                None if value.starts_with('/') => value.to_owned(),
                None => return None,
            };
            let path = path.trim_end_matches('/').to_owned();
            (!path.is_empty() && path != home).then_some(path)
        })
        .filter(|p| seen.insert(p.clone()))
        .collect()
}

fn basename(path: &str) -> &str {
    path.trim_end_matches('/').rsplit_once('/').map_or(path, |(_, b)| b)
}

fn parent(path: &str) -> &str {
    match path.trim_end_matches('/').rsplit_once('/') {
        Some(("", _)) => "/",
        Some((p, _)) => p,
        None => "",
    }
}

/// `path` with the home directory written `~`.
pub fn tilde(path: &str, home: &str) -> String {
    let home = home.trim_end_matches('/');
    match path.strip_prefix(home) {
        Some(rest) if !home.is_empty() && (rest.is_empty() || rest.starts_with('/')) => format!("~{rest}"),
        _ => path.to_owned(),
    }
}

/// A file or folder as a subject. Its id hashes the path, so the path's
/// words never match a query through the id.
pub fn subject(path: &str, dir: bool, meta: Option<String>) -> Node {
    let kind = if dir { FOLDER_KIND } else { FILE_KIND };
    Node {
        path: path.to_owned(),
        meta,
        verb: Some("Open".into()),
        ..Node::new(format!("{FILES_ROUTE}.{}", path_key(path)), basename(path), Kind::Other(kind.into()))
    }
}

/// A file or folder picked from anywhere, as `Recent` lists it: trailing
/// its parent, the home directory written `~`.
pub fn recent_subject(path: &str, dir: bool, home: &str) -> Node {
    subject(path, dir, Some(tilde(parent(path), home)))
}

pub fn is_file_subject(node: &Node) -> bool {
    matches!(&node.kind, Kind::Other(k) if k == FILE_KIND)
}

pub fn is_folder_subject(node: &Node) -> bool {
    matches!(&node.kind, Kind::Other(k) if k == FOLDER_KIND)
}

/// One path the service found on disk, and whether it is a directory.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Found {
    pub path: String,
    pub dir: bool,
}

/// The Files route's rows: the user directories under `Folders`, then the
/// recent files under `Recent files`, each trailing its parent. Nothing
/// found is one dim note.
pub fn files_rows(places: &[Found], recent: &[Found], home: &str) -> Vec<Node> {
    let row = |f: &Found, section: &str| Node {
        section: Some(section.into()),
        ..subject(&f.path, f.dir, Some(tilde(parent(&f.path), home)))
    };
    let mut out: Vec<Node> = places.iter().map(|f| row(f, "Folders")).collect();
    let mut seen: HashSet<String> = out.iter().map(|n| n.id.clone()).collect();
    out.extend(recent.iter().map(|f| row(f, "Recent files")).filter(|n| seen.insert(n.id.clone())));
    if out.is_empty() {
        out.push(Node::note(format!("{FILES_ROUTE}.none"), "No recent files or folders"));
    }
    out
}

/// One entry of a folder.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Listed {
    pub name: String,
    pub dir: bool,
}

impl Listed {
    pub fn hidden(&self) -> bool {
        self.name.starts_with('.')
    }
}

/// Folders first, then by name ignoring case; at most [`LISTING_MAX`]
/// visible entries and as many hidden ones.
pub fn sort_listing(mut entries: Vec<Listed>) -> Vec<Listed> {
    entries.sort_by(|a, b| b.dir.cmp(&a.dir).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase())).then_with(|| a.name.cmp(&b.name)));
    let (mut shown, mut hidden) = (0, 0);
    entries.retain(|e| {
        let n = if e.hidden() { &mut hidden } else { &mut shown };
        *n += 1;
        *n <= LISTING_MAX
    });
    entries
}

/// A folder's entries as subjects, those holding every word of `query`;
/// hidden ones only once the query starts with `.`.
pub fn listing_rows(dir: &str, entries: &[Listed], query: &str) -> Vec<Node> {
    let hidden = query.starts_with('.');
    let words: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
    let dir = dir.trim_end_matches('/');
    entries
        .iter()
        .filter(|e| hidden || !e.hidden())
        .filter(|e| {
            let name = e.name.to_lowercase();
            words.iter().all(|w| name.contains(w.as_str()))
        })
        .map(|e| subject(&format!("{dir}/{}", e.name), e.dir, None))
        .collect()
}

/// What the preview pane shows of a file or folder.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Peek {
    /// The first [`PEEK_LINES`] lines.
    Text(Vec<String>),
    /// A picture, decoded apart at the pane's size.
    Image,
    /// Neither text nor a picture the shell decodes.
    Binary,
    /// The entry count and the first [`PEEK_NAMES`] in listing order.
    Folder { count: usize, names: Vec<Listed> },
    /// Why it could not be read.
    Unreadable(String),
}

/// A file or folder's preview and the facts under it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Peeked {
    pub size: Option<u64>,
    /// Milliseconds since the epoch.
    pub modified: Option<i64>,
    pub peek: Peek,
}

/// The pictures the shell's decoder reads.
pub fn is_image(path: &str) -> bool {
    let ext = path.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).unwrap_or_default();
    matches!(ext.as_str(), "png" | "jpg" | "jpeg" | "webp")
}

/// A file's head as its first [`PEEK_LINES`] lines; `None` when it is not
/// text: a NUL byte, or bytes that are not UTF-8 before the cut a
/// [`PEEK_BYTES`] read can leave inside one character.
pub fn text_lines(head: &[u8]) -> Option<Vec<String>> {
    let head = &head[..head.len().min(PEEK_BYTES)];
    if head.contains(&0) {
        return None;
    }
    let text = match std::str::from_utf8(head) {
        Ok(t) => t,
        Err(e) if e.error_len().is_none() => std::str::from_utf8(&head[..e.valid_up_to()]).ok()?,
        Err(_) => return None,
    };
    Some(text.lines().take(PEEK_LINES).map(|l| l.replace('\t', "    ")).collect())
}

/// A folder's preview off its sorted listing.
pub fn folder_peek(entries: Vec<Listed>) -> Peek {
    let count = entries.iter().filter(|e| !e.hidden()).count();
    Peek::Folder { count, names: sort_listing(entries).into_iter().filter(|e| !e.hidden()).take(PEEK_NAMES).collect() }
}

/// A size in the units a file manager uses (powers of 1000).
pub fn size_label(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["KB", "MB", "GB", "TB"];
    if bytes < 1000 {
        return format!("{bytes} B");
    }
    let mut v = bytes as f64 / 1000.0;
    let mut unit = 0;
    while v >= 1000.0 && unit + 1 < UNITS.len() {
        v /= 1000.0;
        unit += 1;
    }
    if v < 10.0 { format!("{v:.1} {}", UNITS[unit]) } else { format!("{v:.0} {}", UNITS[unit]) }
}

/// A modification time, local: `29 Oct 14:02`.
pub fn modified_label(ms: i64) -> String {
    DateTime::from_timestamp_millis(ms).map(|d| d.with_timezone(&Local).format("%-d %b %H:%M").to_string()).unwrap_or_default()
}

/// The pane's facts line for a file: its size and when it last changed.
pub fn facts(p: &Peeked) -> String {
    let mut out = Vec::new();
    match &p.peek {
        Peek::Folder { count, .. } => out.push(if *count == 1 { "1 item".to_owned() } else { format!("{count} items") }),
        _ => out.extend(p.size.map(size_label)),
    }
    out.extend(p.modified.map(modified_label));
    out.join(" \u{B7} ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xbel_paths_newest_first_and_decoded() {
        let xbel = r#"<?xml version="1.0" encoding="UTF-8"?>
<xbel version="1.0" xmlns:bookmark="http://www.freedesktop.org/standards/desktop-bookmarks">
  <bookmark href="file:///home/u/old.txt" added="2026-01-01T10:00:00Z" modified="2026-01-01T10:00:00Z" visited="2026-01-01T10:00:00Z"/>
  <bookmark href="file:///home/u/My%20Report%2C%20final.pdf" added="2026-01-01T10:00:00Z" modified="2026-10-29T12:00:00.123456Z"/>
  <bookmark href="https://example.com/x" modified="2026-10-30T12:00:00Z"/>
  <bookmark href="file://otherhost/home/u/remote.txt" modified="2026-10-30T12:00:00Z"/>
  <bookmark href="file://localhost/home/u/local.txt" modified="2026-06-01T12:00:00Z"/>
  <bookmark href="file:///home/u/old.txt" modified="2025-01-01T10:00:00Z"/>
</xbel>"#;
        assert_eq!(recent_paths(xbel), ["/home/u/My Report, final.pdf", "/home/u/local.txt", "/home/u/old.txt"]);
        assert!(recent_paths("not xml <").is_empty());
    }

    #[test]
    fn user_dirs_expand_home_and_skip_home_itself() {
        let text = "# comment\nXDG_DESKTOP_DIR=\"$HOME\"\nXDG_DOCUMENTS_DIR=\"$HOME/Documents\"\nXDG_MUSIC_DIR=\"/srv/music/\"\nXDG_BAD_DIR=\"relative\"\nXDG_PICTURES_DIR=\"$HOME/Pictures\"\nXDG_DOWNLOAD_DIR=\"$HOME/Documents\"\n";
        assert_eq!(user_dirs(text, "/home/u/"), ["/home/u/Documents", "/srv/music", "/home/u/Pictures"]);
    }

    #[test]
    fn rows_sections_and_ids() {
        let places = [Found { path: "/home/u/Documents".into(), dir: true }];
        let recent = [Found { path: "/home/u/Documents/a b.txt".into(), dir: false }, Found { path: "/home/u/Documents".into(), dir: true }];
        let rows = files_rows(&places, &recent, "/home/u");
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].label, "Documents");
        assert!(is_folder_subject(&rows[0]));
        assert_eq!(rows[0].section.as_deref(), Some("Folders"));
        assert_eq!(rows[0].meta.as_deref(), Some("~"));
        assert_eq!(rows[1].label, "a b.txt");
        assert!(is_file_subject(&rows[1]));
        assert_eq!(rows[1].meta.as_deref(), Some("~/Documents"));
        assert_eq!(rows[1].path, "/home/u/Documents/a b.txt");
        assert!(rows[1].id.starts_with("files.") && !rows[1].id.contains("Documents"));
        let none = files_rows(&[], &[], "/home/u");
        assert_eq!(none.len(), 1);
        assert_eq!(none[0].kind, Kind::Note);
    }

    #[test]
    fn listing_folders_first_hidden_after_a_dot() {
        let l = |n: &str, dir: bool| Listed { name: n.into(), dir };
        let sorted = sort_listing(vec![l("b.txt", false), l(".git", true), l("Zeta", true), l("a.txt", false), l(".env", false), l("alpha", true)]);
        let names: Vec<&str> = sorted.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, [".git", "alpha", "Zeta", ".env", "a.txt", "b.txt"]);
        let rows = listing_rows("/d/", &sorted, "");
        assert_eq!(rows.iter().map(|r| r.label.as_str()).collect::<Vec<_>>(), ["alpha", "Zeta", "a.txt", "b.txt"]);
        assert_eq!(rows[0].path, "/d/alpha");
        assert!(rows[0].meta.is_none());
        let dotted = listing_rows("/d", &sorted, ".e");
        assert_eq!(dotted.iter().map(|r| r.label.as_str()).collect::<Vec<_>>(), [".env"]);
        assert_eq!(folder_peek(sorted), Peek::Folder { count: 4, names: vec![l("alpha", true), l("Zeta", true), l("a.txt", false), l("b.txt", false)] });
    }

    #[test]
    fn listing_caps_each_kind() {
        let many: Vec<Listed> = (0..LISTING_MAX + 20).map(|i| Listed { name: format!("f{i:04}"), dir: false }).chain((0..3).map(|i| Listed { name: format!(".h{i}"), dir: false })).collect();
        let sorted = sort_listing(many);
        assert_eq!(sorted.iter().filter(|e| !e.hidden()).count(), LISTING_MAX);
        assert_eq!(sorted.iter().filter(|e| e.hidden()).count(), 3);
    }

    #[test]
    fn text_peek() {
        let lines: String = (1..=60).map(|i| format!("line {i}\n")).collect();
        let got = text_lines(lines.as_bytes()).unwrap();
        assert_eq!(got.len(), PEEK_LINES);
        assert_eq!(got[0], "line 1");
        assert_eq!(text_lines(b"a\0b"), None);
        assert_eq!(text_lines(&[0xff, 0xfe, b'a']), None);
        // A cut inside a two-byte character still reads as text.
        assert_eq!(text_lines("ok \u{e9}".as_bytes().split_last().unwrap().1).unwrap(), ["ok "]);
        assert_eq!(text_lines(b"\tx").unwrap(), ["    x"]);
    }

    #[test]
    fn labels() {
        assert_eq!(size_label(999), "999 B");
        assert_eq!(size_label(1500), "1.5 KB");
        assert_eq!(size_label(2_400_000), "2.4 MB");
        assert_eq!(size_label(45_000), "45 KB");
        assert!(is_image("/a/B.JPG") && is_image("x.webp") && !is_image("x.pdf") && !is_image("png"));
        assert_eq!(tilde("/home/u/x", "/home/u"), "~/x");
        assert_eq!(tilde("/home/user2/x", "/home/u"), "/home/user2/x");
        assert_eq!(file_path("file:///a%2Fb"), Some("/a/b".into()));
    }
}
