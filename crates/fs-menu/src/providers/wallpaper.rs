//! The wallpaper route: the picker directory scan, the pairing model that
//! reads a listing as light/dark pairs and singles
//! (specs/2026-11-04-wallpaper-picker.md §3), and the tiles' rows.
//!
//! The picker's tiles live inside the launcher, so they are ordinary display
//! rows, kind "image", carrying the file shown; every piece of machinery the
//! launcher has (`activate(index)` over IPC, the actions column, the close
//! paths) applies to them unchanged.

use std::collections::{BTreeMap, HashMap};

use serde::Deserialize;

use crate::node::{Kind, Node};

/// The file name of a path.
pub fn image_basename(path: &str) -> &str {
    match path.rfind('/') {
        Some(cut) => &path[cut + 1..],
        None => path,
    }
}

/// The picker directory scan, as one argv. There is no directory-listing
/// type in the toolkit, and two callers need the same answer: the picker route
/// re-scans on every entry so a directory edited between opens is picked up,
/// and the thumbnail service scans the configured directory at startup to
/// prerender its thumbnails. One definition of what counts as a pickable
/// image, so the grid and the cache can never disagree about the listing.
///
/// Both mode subdirectories are named as starting points alongside the
/// directory itself: `find` reports a missing one on stderr (swallowed) and
/// carries on with the rest, so one invocation covers every layout, and
/// `-maxdepth 1` per starting point is what keeps an unrelated subdirectory
/// of wallpapers out of the listing. `sort -u` because a case-insensitive
/// filesystem answers both `Dark` and `dark` with the same directory.
pub fn picker_scan_command(dir: &str) -> Vec<String> {
    [
        "sh",
        "-c",
        "find \"$1\" \"$1/Dark\" \"$1/dark\" \"$1/Light\" \"$1/light\" -maxdepth 1 -type f \
         \\( -iname \"*.png\" -o -iname \"*.jpg\" -o -iname \"*.jpeg\" -o -iname \"*.webp\" \\) \
         2>/dev/null | sort -u",
        "sh",
        dir,
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

/// The mode a path's first directory under `base` files it in.
fn filed_mode(path: &str, base: &str) -> Option<Mode> {
    let rest = if base.is_empty() {
        path
    } else {
        path.strip_prefix(base)?.strip_prefix('/')?
    };
    let (segment, _) = rest.split_once('/')?;
    match segment.to_lowercase().as_str() {
        "dark" => Some(Mode::Dark),
        "light" => Some(Mode::Light),
        _ => None,
    }
}

/// Rows for the "wallpaper" route. `query` filters on the basename only: a
/// path's directory component is identical for every row in a listing, so
/// matching it would make every query match everything.
pub fn image_rows(paths: &[String], query: &str) -> Vec<Node> {
    let q = fs_js::trim(query).to_lowercase();
    paths
        .iter()
        .filter(|p| q.is_empty() || image_basename(p).to_lowercase().contains(&q))
        .map(|p| Node {
            parent_id: Some("wallpaper".to_string()),
            path: p.clone(),
            ..Node::new(format!("wallpaper.{p}"), image_basename(p), Kind::Image)
        })
        .collect()
}

/// The picker's rows for `items` in `section`: each tile's node id is
/// `wallpaper.<item id>`, its label the title, its path the file it shows now
/// (a pair's dark half when `dark`, the Sky's `sky` source). `query` matches
/// the title and the file names.
pub fn tile_rows(items: &[Item], section: &str, dark: bool, sky: &str, query: &str) -> Vec<Node> {
    let q = fs_js::trim(query).to_lowercase();
    items
        .iter()
        .filter(|item| {
            q.is_empty()
                || item.title.to_lowercase().contains(&q)
                || item.files().iter().any(|f| image_basename(f).to_lowercase().contains(&q))
        })
        .map(|item| {
            let path = match &item.kind {
                ItemKind::Pair { light, dark: d } => if dark { d.clone() } else { light.clone() },
                ItemKind::Single { path, .. } => path.clone(),
                ItemKind::Sky => sky.to_owned(),
            };
            Node {
                parent_id: Some("wallpaper".to_string()),
                path,
                section: Some(section.to_owned()),
                ..Node::new(format!("wallpaper.{}", item.id), item.title.clone(), Kind::Image)
            }
        })
        .collect()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Mode {
    Light,
    Dark,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Credit {
    pub author: String,
    pub license: String,
    pub license_url: String,
    pub source: String,
    pub origin: String,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ItemKind {
    Pair { light: String, dark: String },
    /// `filed` is the mode its directory files it under; `None` is a
    /// picture for both modes.
    Single { path: String, filed: Option<Mode> },
    Sky,
}

/// One tile of the picker.
#[derive(Clone, Debug, PartialEq)]
pub struct Item {
    pub id: String,
    pub kind: ItemKind,
    pub title: String,
    pub credit: Option<Credit>,
}

impl Item {
    /// The files the tile stands for: a pair's two halves, a single's one.
    pub fn files(&self) -> Vec<&str> {
        match &self.kind {
            ItemKind::Pair { light, dark } => vec![light, dark],
            ItemKind::Single { path, .. } => vec![path],
            ItemKind::Sky => Vec::new(),
        }
    }
}

/// The scan's listing read as pairs and singles, ordered by title.
/// `identical` says whether two files hold the same bytes; it is only asked
/// about files sharing a name.
pub fn items(paths: &[String], base_dir: &str, identical: impl Fn(&str, &str) -> bool) -> Vec<Item> {
    let base = base_dir.trim_end_matches('/');
    let mut by_name: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for p in paths {
        by_name.entry(image_basename(p)).or_default().push(p);
    }
    let mut out = Vec::new();
    let mut loose: Vec<(&str, Option<Mode>)> = Vec::new();
    for files in by_name.values_mut() {
        files.sort_unstable();
        files.dedup();
        // Dark, then light, then the root, so a root copy folds into the
        // copy its subdirectory files.
        let mut kept: Vec<(&str, Option<Mode>)> = Vec::new();
        for want in [Some(Mode::Dark), Some(Mode::Light), None] {
            for &p in files.iter().filter(|p| filed_mode(p, base) == want) {
                match kept.iter_mut().find(|(k, _)| identical(k, p)) {
                    Some(copy) if want.is_some() && copy.1.is_some() && copy.1 != want => copy.1 = None,
                    Some(_) => {}
                    None => kept.push((p, want)),
                }
            }
        }
        let dark = kept.iter().position(|k| k.1 == Some(Mode::Dark));
        let light = kept.iter().position(|k| k.1 == Some(Mode::Light));
        if let (Some(d), Some(l)) = (dark, light) {
            out.push(pair(kept[l].0, kept[d].0));
            let (d, l) = (kept[d].0, kept[l].0);
            kept.retain(|k| k.0 != d && k.0 != l);
        }
        loose.extend(kept);
    }

    let keys: Vec<Vec<(String, Mode)>> = loose.iter().map(|(p, _)| mode_keys(stem(image_basename(p)))).collect();
    let mut darks: HashMap<&str, Vec<usize>> = HashMap::new();
    for (j, ks) in keys.iter().enumerate() {
        for (key, _) in ks.iter().filter(|(_, m)| *m == Mode::Dark) {
            darks.entry(key.as_str()).or_default().push(j);
        }
    }
    let mut edges: Vec<(usize, usize)> = Vec::new();
    for (i, ks) in keys.iter().enumerate() {
        for (key, _) in ks.iter().filter(|(_, m)| *m == Mode::Light) {
            edges.extend(darks.get(key.as_str()).into_iter().flatten().filter(|&&j| j != i).map(|&j| (i, j)));
        }
    }
    edges.sort_by_key(|&(i, j)| {
        let (l, d) = (loose[i].0, loose[j].0);
        (parent(l) != parent(d), image_basename(l), image_basename(d), l, d)
    });
    let mut taken = vec![false; loose.len()];
    for (i, j) in edges {
        if !taken[i] && !taken[j] {
            taken[i] = true;
            taken[j] = true;
            out.push(pair(loose[i].0, loose[j].0));
        }
    }
    for (&(path, filed), _) in loose.iter().zip(&taken).filter(|(_, t)| !**t) {
        out.push(Item {
            id: path.to_string(),
            kind: ItemKind::Single { path: path.to_string(), filed },
            title: title_of(image_basename(path)),
            credit: None,
        });
    }
    out.sort_by(|a, b| (a.title.to_lowercase(), &a.id).cmp(&(b.title.to_lowercase(), &b.id)));
    out
}

fn pair(light: &str, dark: &str) -> Item {
    Item {
        id: light.to_string(),
        kind: ItemKind::Pair { light: light.to_string(), dark: dark.to_string() },
        title: title_of(image_basename(light)),
        credit: None,
    }
}

fn parent(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(dir, _)| dir)
}

fn stem(name: &str) -> &str {
    match name.rfind('.') {
        Some(dot) if dot > 0 => &name[..dot],
        _ => name,
    }
}

fn tokens(stem: &str) -> Vec<&str> {
    stem.split(['-', '_', '.', ' ', '(', ')', '[', ']', '{', '}'])
        .filter(|t| !t.is_empty())
        .collect()
}

/// The keys a name pairs on, one per mode token it carries: the other
/// tokens with the mode token's place marked. A whole `dark`/`light` and a
/// last `d`/`l` mark differently, so `x-dark` never pairs with `x-l`.
fn mode_keys(stem: &str) -> Vec<(String, Mode)> {
    let toks: Vec<String> = tokens(stem).iter().map(|t| t.to_lowercase()).collect();
    let mut out = Vec::new();
    for (i, t) in toks.iter().enumerate() {
        let (mark, mode) = match t.as_str() {
            "dark" => ("\0word", Mode::Dark),
            "light" => ("\0word", Mode::Light),
            "d" if i + 1 == toks.len() && i > 0 => ("\0letter", Mode::Dark),
            "l" if i + 1 == toks.len() && i > 0 => ("\0letter", Mode::Light),
            _ => continue,
        };
        let key: Vec<&str> = toks.iter().enumerate().map(|(k, t)| if k == i { mark } else { t.as_str() }).collect();
        out.push((key.join("\u{1}"), mode));
    }
    out
}

/// A file name read as a title: the mode token, any `<w>x<h>` token and any
/// all-digit token of five or more digits dropped, the first letter
/// upper-case. A name that leaves nothing is its own title.
pub fn title_of(name: &str) -> String {
    let toks = tokens(stem(name));
    let last = toks.len().saturating_sub(1);
    let kept: Vec<&str> = toks
        .iter()
        .enumerate()
        .filter(|&(i, t)| {
            let lower = t.to_lowercase();
            let digits = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
            let size = lower.split_once('x').is_some_and(|(w, h)| digits(w) && digits(h));
            let mode = lower == "dark" || lower == "light" || (i == last && i > 0 && (lower == "d" || lower == "l"));
            !(mode || size || (digits(t) && t.len() >= 5))
        })
        .map(|(_, t)| *t)
        .collect();
    let joined = kept.join(" ");
    let mut chars = joined.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => name.to_string(),
    }
}

#[derive(Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct Entry {
    id: String,
    title: String,
    author: String,
    license: String,
    license_url: String,
    source: String,
    origin: String,
    file: Option<String>,
    light: Option<String>,
    dark: Option<String>,
}

/// The built-ins out of `catalog.json` in `dir`, in its order. An entry
/// whose file (or either half) `exists` denies is left out; JSON that does
/// not parse is no built-ins.
pub fn catalog(json: &str, dir: &str, exists: impl Fn(&str) -> bool) -> Vec<Item> {
    let Ok(entries) = serde_json::from_str::<Vec<Entry>>(json) else {
        return Vec::new();
    };
    let dir = dir.trim_end_matches('/');
    let file = |f: &Option<String>| {
        f.as_deref().filter(|f| !f.is_empty()).map(|f| format!("{dir}/{f}")).filter(|p| exists(p))
    };
    entries
        .into_iter()
        .filter_map(|e| {
            let (kind, shown) = match &e.file {
                Some(_) => {
                    let path = file(&e.file)?;
                    (ItemKind::Single { path: path.clone(), filed: None }, path)
                }
                None => {
                    let (light, dark) = (file(&e.light)?, file(&e.dark)?);
                    (ItemKind::Pair { light: light.clone(), dark }, light)
                }
            };
            let name = image_basename(&shown).to_string();
            Some(Item {
                id: if e.id.is_empty() { name.clone() } else { e.id },
                title: if e.title.is_empty() { title_of(&name) } else { e.title },
                kind,
                credit: Some(Credit {
                    author: e.author,
                    license: e.license,
                    license_url: e.license_url,
                    source: e.source,
                    origin: e.origin,
                }),
            })
        })
        .collect()
}
