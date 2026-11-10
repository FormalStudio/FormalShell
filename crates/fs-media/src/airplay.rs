//! Pure model for the AirPlay receiver, driven with UxPlay's own file and log
//! shapes.
//!
//! `-md <fn>` overwrites the whole file on every DMAP metadata update, one
//! `Label: value\n` line per field the update carried, and "no data\n" at the
//! start of a session. A field absent from the latest read is absent from that
//! update, so `parse_metadata` never merges across reads. `-ca <fn>` overwrites
//! the same way, but every reset first writes a fixed 95-byte placeholder PNG,
//! so a cover is only real once its size departs from that count. Every line
//! UxPlay logs is INFO level with no prefix, except the ones its own message
//! text spells as `*** ERROR`.

use std::sync::LazyLock;

use regex::Regex;

pub(crate) const COVER_PLACEHOLDER_BYTES: u64 = 95;

// `.` must not match any line terminator (\r, U+2028, U+2029 as well as \n).
static METADATA_LINE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^([A-Za-z][A-Za-z ]*): ([^\n\r\u{2028}\u{2029}]*)$").unwrap());

// report_client_request()'s own format: "connection request from %s (%s) with
// deviceID = %s".
static CONNECTED_LINE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"^connection request from ([^\n\r\u{2028}\u{2029}]+) \(([^\n\r\u{2028}\u{2029}]+)\) with deviceID = ([^\n\r\u{2028}\u{2029}]+)$",
    )
    .unwrap()
});

// Matched by substring: the two sites that log it disagree on the
// "***ERROR"/"*** ERROR" spacing.
const DISCONNECTED_MARK: &str = "lost connection with client";

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Metadata {
    pub title: String,
    pub artist: String,
    pub album: String,
    pub genre: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    Connected {
        name: String,
        model: String,
        device_id: String,
    },
    Disconnected,
    Error {
        message: String,
    },
}

/// Every field this update reported, "" for the rest.
pub fn parse_metadata(text: &str) -> Metadata {
    let mut out = Metadata::default();
    for line in text.split('\n') {
        let Some(m) = METADATA_LINE.captures(line) else {
            continue;
        };
        let value = m[2].to_string();
        match &m[1] {
            "Title" => out.title = value,
            "Artist" => out.artist = value,
            "Album" => out.album = value,
            "Genre" => out.genre = value,
            _ => {}
        }
    }
    out
}

/// A cover file of exactly this size is always the reset placeholder.
pub fn is_placeholder_cover(byte_size: u64) -> bool {
    byte_size == COVER_PLACEHOLDER_BYTES
}

/// One stdout line to a connect/disconnect/error event, or None.
pub fn parse_line(line: &str) -> Option<Event> {
    let text = line.trim();
    if text.is_empty() {
        return None;
    }
    if let Some(m) = CONNECTED_LINE.captures(text) {
        return Some(Event::Connected {
            name: m[1].to_string(),
            model: m[2].to_string(),
            device_id: m[3].to_string(),
        });
    }
    if text.contains(DISCONNECTED_MARK) {
        return Some(Event::Disconnected);
    }
    if text.starts_with("*** ERROR") || text.starts_with("***ERROR") {
        return Some(Event::Error {
            message: text.to_string(),
        });
    }
    None
}

/// `airplay.name`, else the hostname once the probe answers, else the shell's
/// own name.
pub fn resolve_name(configured: &str, hostname: &str) -> String {
    if !configured.is_empty() {
        return configured.to_string();
    }
    if hostname.is_empty() {
        "FormalShell".to_string()
    } else {
        hostname.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_metadata_reads_every_known_field() {
        let m = parse_metadata(
            "Album artist: The Band\nAlbum: A Record\nArtist: The Band\nGenre: Rock\nTitle: A Song\n",
        );
        assert_eq!(m.title, "A Song");
        assert_eq!(m.artist, "The Band");
        assert_eq!(m.album, "A Record");
        assert_eq!(m.genre, "Rock");
    }

    #[test]
    fn parse_metadata_missing_field_is_blank() {
        let m = parse_metadata("Title: A Song\n");
        assert_eq!(m.title, "A Song");
        assert_eq!(m.artist, "");
        assert_eq!(m.album, "");
    }

    #[test]
    fn parse_metadata_no_data_placeholder_is_all_blank() {
        assert_eq!(parse_metadata("no data\n"), Metadata::default());
    }

    #[test]
    fn parse_metadata_empty_text() {
        assert_eq!(parse_metadata("").title, "");
    }

    #[test]
    fn parse_metadata_album_artist_does_not_leak_into_album() {
        assert_eq!(parse_metadata("Album artist: Someone Else\n").album, "");
    }

    #[test]
    fn placeholder_cover() {
        assert!(is_placeholder_cover(95));
        assert!(!is_placeholder_cover(94));
        assert!(!is_placeholder_cover(0));
        assert!(!is_placeholder_cover(48213));
    }

    #[test]
    fn parse_line_connected() {
        let e = parse_line(
            "connection request from Kyan's iPhone (iPhone15,2) with deviceID = 11:22:33:44:55:66",
        );
        assert_eq!(
            e,
            Some(Event::Connected {
                name: "Kyan's iPhone".into(),
                model: "iPhone15,2".into(),
                device_id: "11:22:33:44:55:66".into(),
            })
        );
    }

    #[test]
    fn parse_line_disconnected_either_spacing() {
        assert_eq!(
            parse_line("***ERROR lost connection with client (network problem?)"),
            Some(Event::Disconnected)
        );
        assert_eq!(
            parse_line("*** ERROR lost connection with client (network problem?)"),
            Some(Event::Disconnected)
        );
    }

    #[test]
    fn parse_line_unrelated_is_none() {
        assert_eq!(
            parse_line("UxPlay 1.73 an AirPlay Unix mirroring server"),
            None
        );
        assert_eq!(parse_line(""), None);
        assert_eq!(parse_line("   "), None);
    }

    #[test]
    fn parse_line_error() {
        assert!(matches!(
            parse_line("*** ERROR: could not start mDNS advertising"),
            Some(Event::Error { .. })
        ));
    }

    #[test]
    fn resolve_name_prefers_configured() {
        assert_eq!(resolve_name("Kyan's Desk", "g815"), "Kyan's Desk");
    }

    #[test]
    fn resolve_name_falls_back_to_hostname() {
        assert_eq!(resolve_name("", "g815"), "g815");
    }

    #[test]
    fn resolve_name_falls_back_to_shell_name() {
        assert_eq!(resolve_name("", ""), "FormalShell");
    }
}
