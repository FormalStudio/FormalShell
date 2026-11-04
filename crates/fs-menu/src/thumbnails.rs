//! Cache names for the prerendered thumbnails (thumbnails.js): the wallpaper
//! picker's `cover` squares and 16:9 `wide` tiles (`size` wide), and a
//! clipboard capture's `fit` box. A cached
//! name is `<basename>-<key>-<mode><size>.jpg`; size and mode are in it so
//! changing either invalidates the files by construction. The key hashes
//! UTF-16 units.

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Mode {
    Cover,
    Fit,
    Wide,
}

impl Mode {
    /// Anything that is not "fit" or "wide" is "cover".
    pub fn parse(mode: &str) -> Mode {
        match mode {
            "fit" => Mode::Fit,
            "wide" => Mode::Wide,
            _ => Mode::Cover,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Mode::Cover => "cover",
            Mode::Fit => "fit",
            Mode::Wide => "wide",
        }
    }
}

fn fnv1a(units: impl Iterator<Item = u16>, basis: u32) -> u32 {
    let mut h = basis;
    for u in units {
        h ^= u32::from(u);
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// Two 32-bit FNV-1a passes, the second over the reversed string with a
/// different basis: a wallpaper directory of long shared prefixes is where
/// one 32-bit hash stops being safe.
pub fn path_key(path: &str) -> String {
    let units: Vec<u16> = path.encode_utf16().collect();
    format!("{:08x}{:08x}", fnv1a(units.iter().copied(), 0x811c_9dc5), fnv1a(units.iter().rev().copied(), 0x0100_0193))
}

fn basename(path: &str) -> &str {
    path.rsplit_once('/').map_or(path, |(_, b)| b)
}

fn slug(name: &str) -> String {
    let mut out = String::new();
    let mut run = false;
    for c in name.chars() {
        if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') {
            out.push(c);
            run = false;
        } else if !run {
            out.push('_');
            run = true;
        }
    }
    out.truncate(48);
    if out.is_empty() { "image".into() } else { out }
}

pub fn cache_file_name(path: &str, size: u32, mode: Mode) -> String {
    let base = basename(path);
    let stem = match base.rfind('.') {
        Some(dot) if dot > 0 => &base[..dot],
        _ => base,
    };
    format!("{}-{}-{}{}.jpg", slug(stem), path_key(path), mode.as_str(), size)
}

pub fn cache_path(dir: &str, path: &str, size: u32, mode: Mode) -> String {
    format!("{}/{}", dir.trim_end_matches('/'), cache_file_name(path, size, mode))
}

/// The generator: `$1` the cache directory, `$2` the edge, `$3` the mode,
/// then src/dst pairs. Prints each source as its thumbnail lands. A source
/// not newer than its thumbnail is a hit and `touch`es it, so the 30-day
/// sweep is an LRU. A failed ffmpeg prints nothing.
pub fn warm_script(concurrency: u32) -> String {
    [
        "dir=$1; size=$2; mode=$3; shift 3",
        "mkdir -p \"$dir\" || exit 0",
        "find \"$dir\" -maxdepth 1 -type f \\( -name \"*.part.jpg\" -o -mtime +30 \\) -delete 2>/dev/null",
        "if [ \"$mode\" = fit ]; then",
        "  vf=\"scale=w='min(iw,$size)':h='min(ih,$size)':force_original_aspect_ratio=decrease:flags=lanczos\"",
        "elif [ \"$mode\" = wide ]; then",
        "  h=$((size * 9 / 16))",
        "  vf=\"scale=$size:$h:force_original_aspect_ratio=increase:flags=lanczos,crop=$size:$h\"",
        "else",
        "  vf=\"scale=$size:$size:force_original_aspect_ratio=increase:flags=lanczos,crop=$size:$size\"",
        "fi",
        &format!("printf '%s\\0' \"$@\" | xargs -0 -r -n 2 -P {} sh -c '", concurrency.max(1)),
        "  src=$1; dst=$2",
        "  if [ -f \"$dst\" ] && [ ! \"$src\" -nt \"$dst\" ]; then touch \"$dst\"; printf \"%s\\n\" \"$src\"; exit 0; fi",
        "  tmp=\"$dst.part.jpg\"",
        "  ffmpeg -v error -nostdin -y -i \"$src\" -frames:v 1 -q:v 3 -vf \"$0\" \\",
        "    \"$tmp\" >/dev/null 2>&1 || { rm -f \"$tmp\"; exit 0; }",
        "  mv -f \"$tmp\" \"$dst\" && printf \"%s\\n\" \"$src\"",
        "' \"$vf\"",
    ]
    .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names() {
        let a = cache_file_name("/pics/a.jpg", 512, Mode::Cover);
        assert_ne!(a, cache_file_name("/pics/a.jpg", 256, Mode::Cover));
        assert_ne!(a, cache_file_name("/pics/a.jpg", 512, Mode::Fit));
        assert!(a.starts_with("a-") && a.ends_with("-cover512.jpg"));
        assert_eq!(path_key("/pics/a.jpg").len(), 16);
        assert!(cache_file_name("/pics/.hidden", 512, Mode::Cover).starts_with(".hidden"));
        assert!(cache_file_name(&format!("/pics/{}.jpg", "x y".repeat(80)), 512, Mode::Cover).len() < 100);
        assert_eq!(cache_path("/cache/", "/p/a.png", 512, Mode::Fit), format!("/cache/{}", cache_file_name("/p/a.png", 512, Mode::Fit)));
        let wide = cache_file_name("/pics/a.jpg", 640, Mode::Wide);
        assert!(wide.starts_with("a-") && wide.ends_with("-wide640.jpg"));
        assert_ne!(wide, cache_file_name("/pics/a.jpg", 640, Mode::Cover));
        assert_eq!(Mode::parse(Mode::Wide.as_str()), Mode::Wide);
    }
}
