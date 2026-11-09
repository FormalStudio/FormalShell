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

    /// ffmpeg's `-vf` for a thumbnail `size` wide.
    pub fn filter(self, size: u32) -> String {
        match self {
            Mode::Fit => format!("scale=w='min(iw,{size})':h='min(ih,{size})':force_original_aspect_ratio=decrease:flags=lanczos"),
            Mode::Wide => {
                let h = size * 9 / 16;
                format!("scale={size}:{h}:force_original_aspect_ratio=increase:flags=lanczos,crop={size}:{h}")
            }
            Mode::Cover => format!("scale={size}:{size}:force_original_aspect_ratio=increase:flags=lanczos,crop={size}:{size}"),
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
        assert!(Mode::Wide.filter(640).contains("scale=640:360:") && Mode::Wide.filter(640).ends_with("crop=640:360"));
        assert!(Mode::Fit.filter(512).contains("min(iw,512)"));
    }
}
