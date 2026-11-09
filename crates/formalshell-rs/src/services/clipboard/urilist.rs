//! GTK4 apps copy an image as a GdkFileList, not as pixels: Loupe and
//! Nautilus both offer `text/uri-list` (RFC 2483: CRLF lines, `#` comments)
//! with the path again as text/plain, and no image/* type at all. This reads
//! such an offer down to the one local image it names.

/// What ffmpeg's image2 demuxer decodes to a single frame. No svg: that
/// would need a rasteriser the wrapper's PATH does not carry.
pub const EXTENSIONS: [&str; 9] = [
    "png", "jpg", "jpeg", "webp", "gif", "bmp", "tif", "tiff", "avif",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImageFile {
    pub uri: String,
    pub path: String,
    pub png: bool,
}

/// The one local image `uri_list` names, or None for anything else (several
/// files, a remote uri, a file that is not a picture).
pub fn image_file(uri_list: Option<&str>) -> Option<ImageFile> {
    let cleaned = uri_list?.replace('\0', "");
    let mut uris = Vec::new();
    for line in cleaned.split('\n') {
        let line = line.strip_suffix('\r').unwrap_or(line);
        if !line.is_empty() && !line.starts_with('#') {
            uris.push(line);
        }
    }
    if uris.len() != 1 {
        return None;
    }
    let uri = uris[0];

    let rest = uri.strip_prefix("file://")?;
    let slash = rest.find('/')?;
    let raw_path = &rest[slash..];
    // The path is matched with `.*` up to the end of the line, which does
    // not span these.
    if raw_path.contains(['\r', '\u{2028}', '\u{2029}']) {
        return None;
    }
    let path = decode_uri_component(raw_path)?;

    let ext = match path.rfind('.') {
        Some(dot) => path[dot + 1..].to_lowercase(),
        None => String::new(),
    };
    if !EXTENSIONS.contains(&ext.as_str()) {
        return None;
    }
    Some(ImageFile {
        uri: uri.to_string(),
        path,
        png: ext == "png",
    })
}

/// `decodeURIComponent`: a `%` not followed by two hex digits, or escapes
/// that do not decode to valid UTF-8, fail.
fn decode_uri_component(s: &str) -> Option<String> {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hi = hex(*bytes.get(i + 1)?)?;
            let lo = hex(*bytes.get(i + 2)?)?;
            out.push(hi * 16 + lo);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

fn hex(b: u8) -> Option<u8> {
    (b as char).to_digit(16).map(|d| d as u8)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_local_png() {
        let f = image_file(Some("file:///home/k/Pictures/shot.png\r\n")).unwrap();
        assert_eq!(f.path, "/home/k/Pictures/shot.png");
        assert_eq!(f.uri, "file:///home/k/Pictures/shot.png");
        assert!(f.png);
    }

    #[test]
    fn percent_escapes_and_case() {
        let f = image_file(Some("file:///home/k/My%20Pictures/caf%C3%A9.JPG")).unwrap();
        assert_eq!(f.path, "/home/k/My Pictures/café.JPG");
        assert!(!f.png);
    }

    #[test]
    fn host_and_comment_lines() {
        let f = image_file(Some("# copied\r\nfile://macbook/tmp/a.webp\r\n")).unwrap();
        assert_eq!(f.path, "/tmp/a.webp");
    }

    #[test]
    fn leaked_nul_is_stripped() {
        assert_eq!(image_file(Some("\0file:///tmp/a.png")).unwrap().path, "/tmp/a.png");
    }

    #[test]
    fn rejects_everything_else() {
        assert_eq!(image_file(Some("file:///tmp/a.png\r\nfile:///tmp/b.png\r\n")), None);
        assert_eq!(image_file(Some("https://example.com/a.png")), None);
        assert_eq!(image_file(Some("file:///tmp/notes.txt")), None);
        assert_eq!(image_file(Some("file:///tmp/noext")), None);
        assert_eq!(image_file(Some("file:///tmp/bad%ZZ.png")), None);
        assert_eq!(image_file(Some("")), None);
        assert_eq!(image_file(None), None);
    }
}
