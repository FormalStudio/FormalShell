use super::*;

/// What the preview pane shows for a subject (launcher spec §7), `None`
/// for one with no preview.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PaneKind {
    Clip,
    File,
    Folder,
    Window,
    Package,
}

impl PaneKind {
    pub fn name(self) -> &'static str {
        match self {
            PaneKind::Clip => "clipboard",
            PaneKind::File => "file",
            PaneKind::Folder => "folder",
            PaneKind::Window => "window",
            PaneKind::Package => "package",
        }
    }
}

pub fn pane_kind(n: &Node) -> Option<PaneKind> {
    if matches!(n.parent_id.as_deref(), Some("clipboard" | "share.history")) && n.kind != NodeKind::Note {
        Some(PaneKind::Clip)
    } else if fs_menu::providers::is_file_subject(n) {
        Some(PaneKind::File)
    } else if fs_menu::providers::is_folder_subject(n) {
        Some(PaneKind::Folder)
    } else if n.kind == NodeKind::Window {
        Some(PaneKind::Window)
    } else if fs_menu::providers::is_package(n) {
        Some(PaneKind::Package)
    } else {
        None
    }
}

/// The picture a pane subject shows and where it is read from: a
/// clipboard capture or an image file through the decoder, or the window
/// the pane captures.
pub enum PanePicture<'a> {
    Decoded(&'a str),
    Window(&'a str),
}

pub fn pane_picture(n: &Node) -> Option<PanePicture<'_>> {
    match pane_kind(n)? {
        PaneKind::Clip if !n.thumb_source.is_empty() => Some(PanePicture::Decoded(&n.thumb_source)),
        PaneKind::File if fs_menu::providers::is_image(&n.path) => Some(PanePicture::Decoded(&n.path)),
        PaneKind::Window => Some(PanePicture::Window(&n.path)),
        _ => None,
    }
}

/// An Install row's screenshot box in a pane `w` wide: the width, at most
/// 10:16 of it tall.
pub fn shot_box(w: f64) -> (u32, u32) {
    (w.max(1.0).round() as u32, (w * 0.625).max(1.0).round() as u32)
}

/// The Install pane, as tall as what `fspec info` had to say: the icon,
/// name and summary, the description clamped, the catalog's facts as
/// label-over-value lines in two columns, then the first screenshot once
/// fetched, in what height is left. A field the catalog left out is not
/// drawn.
pub(super) fn package_pane(store: &Store, row: &Node, theme: &Theme, kit: &mut Kit, width: f64, height: f64) -> El {
    let s = &theme.space;
    let info = store.packages.info.get(&row.path);
    let ok = info.and_then(|i| i.as_ref().ok());
    let found = store.packages.search.as_ref().and_then(|(_, f)| f.as_ref().ok()).and_then(|f| f.iter().find(|p| p.id == row.path));
    let icon_px = launcher_px(48.0);
    let mark = match store.menu.row_icons.get(&row.icon_source).cloned().flatten() {
        Some(image) => {
            let density = image.pixmap.width() as f64 / icon_px.max(1.0);
            w::picture(Some(image.at(density)), icon_px)
        }
        None => w::icon("package").size(Type::Display).ink(Ink::Muted).width(Size::Px(icon_px)).height(icon_px),
    };
    let name = ok.and_then(|i| i.name.clone()).unwrap_or_else(|| row.label.clone());
    let summary = ok.and_then(|i| i.summary.clone()).or_else(|| found.and_then(|p| p.description.clone())).unwrap_or_default();
    let text_w = (width - icon_px - s.lg).max(1.0);
    let mut head = vec![w::text(name).size(Type::Heading).weight(Weight::Medium).elide()];
    if !summary.is_empty() {
        head.push(w::para(summary, Type::BodySmall, Weight::Normal, Ink::Muted, 2));
    }
    let mut parts = vec![w::row(s.lg, vec![mark, w::column(s.xs, head).width(Size::Px(text_w))])];
    if let Some(d) = ok.and_then(|i| i.description.clone()) {
        parts.push(w::para(d, Type::BodySmall, Weight::Normal, Ink::Fg, 4));
    }
    let source = ok.map(|i| i.source.clone()).or_else(|| found.map(|p| p.source.clone())).filter(|v| !v.is_empty());
    let version = ok.and_then(|i| i.version.clone()).or_else(|| found.and_then(|p| p.version.clone()));
    let facts: Vec<(&str, String)> = [
        ("Version", version),
        ("Source", source),
        ("Download", ok.and_then(|i| i.download.clone())),
        ("Installed size", ok.and_then(|i| i.installed.clone())),
        ("License", ok.and_then(|i| i.license.clone())),
        ("Developer", ok.and_then(|i| i.developer.clone())),
    ]
    .into_iter()
    .filter_map(|(k, v)| v.filter(|v| !v.is_empty()).map(|v| (k, v)))
    .collect();
    let col_w = ((width - s.lg) / 2.0).floor().max(1.0);
    let line = |(label, value): &(&str, String)| {
        w::column(s.xxs, vec![w::text(label.to_string()).size(Type::BodySmall).weight(Weight::Medium), w::caption(value.clone()).ink(Ink::Muted).elide()]).width(Size::Px(col_w))
    };
    let grid: Vec<El> = facts.chunks(2).map(|pair| w::row(s.lg, pair.iter().map(line).collect())).collect();
    if !grid.is_empty() {
        parts.push(w::column(s.md, grid));
    }
    if let Some(Err(why)) = info {
        parts.push(w::caption(why.clone()).ink(Ink::Muted));
    }
    let shot = ok.and_then(|i| i.screenshot.as_ref()).and_then(|u| store.packages.shots.get(u)).cloned().flatten();
    let (bw, bh) = shot_box(width);
    if let Some(bitmap) = shot.and_then(|path| store.clipboard.preview.as_ref().filter(|p| p.path == path && p.size.0 == (bw, bh)).and_then(|p| p.bitmap.clone())) {
        let used = ui::measure(&w::column(s.lg, parts.clone()), width, theme, kit).1 + s.lg;
        let (iw, ih) = (f64::from(bitmap.pixmap.width()).max(1.0), f64::from(bitmap.pixmap.height()).max(1.0));
        let h = (f64::from(bw) * ih / iw).min(f64::from(bh)).min(height - used).floor();
        // Too short a strip to show anything is left out.
        if h >= 48.0 {
            let pw = (h * iw / ih).min(f64::from(bw)).floor();
            parts.push(w::picture(Some(bitmap), h).width(Size::Px(pw)).height(h));
        }
    }
    w::column(s.lg, parts)
}

/// The pane's name and facts lines under what it shows.
pub(super) fn pane_foot_h(theme: &Theme, kit: &mut Kit, w: f64) -> f64 {
    let s = &theme.space;
    ui::measure(&w::text("Ag"), w, theme, kit).1 + ui::measure(&w::caption("Ag").mono(), w, theme, kit).1 + s.row_gap * 2.0
}

/// The pane's picture box for a pane `w` by `h`.
pub fn preview_box(theme: &Theme, kit: &mut Kit, w: f64, h: f64) -> (u32, u32) {
    (w.max(1.0).round() as u32, (h - pane_foot_h(theme, kit, w) - theme.space.lg).max(1.0).round() as u32)
}

/// A frame of a `from` window fitted inside `box_`, its shape kept.
pub fn fit_inside(from: (f64, f64), box_: (u32, u32)) -> (u32, u32) {
    let (bw, bh) = (f64::from(box_.0), f64::from(box_.1));
    if from.0 <= 0.0 || from.1 <= 0.0 {
        return box_;
    }
    let k = (bw / from.0).min(bh / from.1);
    ((from.0 * k).round().max(1.0) as u32, (from.1 * k).round().max(1.0) as u32)
}

/// The preview pane, DESIGN's one card inside the surface: what the
/// selected subject holds in the room above, then its name and a line of
/// facts in `mono`.
pub(super) fn preview_el(m: &Model, store: &Store, row: &Node, theme: &Theme, kit: &mut Kit, width: f64, height: f64) -> El {
    use fs_menu::providers::Peek;
    let s = &theme.space;
    let Some(kind) = pane_kind(row) else { return w::space(0.0) };
    let (bw, bh) = preview_box(theme, kit, width, height);
    let (bw_f, bh_f) = (f64::from(bw), f64::from(bh));
    let boxed = |el: El| el.width(Size::Px(bw_f)).height(bh_f);
    let decoded = |path: &str| {
        store.clipboard.preview.as_ref().filter(|p| p.path == path && p.size.0 == (bw, bh)).map(|p| (p.bitmap.clone(), p.dims))
    };
    // The picture hangs from the pane's top at its own shape, the space
    // under it keeping the name and facts on the pane's foot.
    let picture = |image: Option<crate::scene::Bitmap>| {
        let h = image.as_ref().map_or(bh_f, |b| {
            let (iw, ih) = (f64::from(b.pixmap.width()).max(1.0), f64::from(b.pixmap.height()));
            (bw_f * ih / iw).min(bh_f).round()
        });
        boxed(w::column(0.0, vec![w::picture(image, h).width(Size::Px(bw_f)).height(h), w::space((bh_f - h).max(0.0))]))
    };
    let line = ui::measure(&w::caption("Ag").mono(), width, theme, kit).1.max(1.0);
    let lines_el = |lines: &[String]| {
        let fits = ((bh_f / line).floor() as usize).max(1);
        let col: Vec<El> = lines.iter().take(fits).map(|l| w::caption(l.clone()).mono().ink(Ink::Fg).elide()).collect();
        boxed(w::column(0.0, col))
    };
    let muted = |t: &str| boxed(w::column(0.0, vec![w::space((bh_f / 2.0 - line).max(0.0)), w::text(t.to_owned()).ink(Ink::Muted).centred()]));
    let peeked = || store.menu.peek.as_ref().filter(|(p, _)| *p == row.path).map(|(_, p)| p);
    let dims = |d: Option<(u32, u32)>| d.map(|(w_, h_)| format!("{w_} \u{D7} {h_}"));
    let join = |parts: Vec<String>| parts.into_iter().filter(|p| !p.is_empty()).collect::<Vec<_>>().join(" \u{B7} ");
    let (content, title, facts) = match kind {
        PaneKind::Clip if !row.thumb_source.is_empty() => {
            let got = decoded(&row.thumb_source);
            let facts = join(vec![dims(got.as_ref().and_then(|g| g.1)).unwrap_or_default(), row.time.clone()]);
            (picture(got.and_then(|g| g.0)), "Image".to_owned(), facts)
        }
        PaneKind::Clip if row.emoji_only => (boxed(w::column(0.0, vec![w::space((bh_f / 2.0 - line * 2.0).max(0.0)), w::text(row.full_text.clone()).size(Type::Display).centred()])), "Text".to_owned(), row.time.clone()),
        PaneKind::Clip => {
            let body_line = ui::measure(&w::text("Ag"), width, theme, kit).1.max(1.0);
            let lines = ((bh_f / body_line).floor() as usize).max(1);
            (boxed(w::para(row.full_text.clone(), Type::Body, Weight::Normal, Ink::Fg, lines)), "Text".to_owned(), row.time.clone())
        }
        PaneKind::File | PaneKind::Folder => {
            let p = peeked();
            let facts = p.map(fs_menu::providers::facts).unwrap_or_default();
            // A picture shows off its own decode, which is asked beside the
            // peek; the peek only adds its facts.
            let content = match p.map(|p| &p.peek) {
                _ if matches!(kind, PaneKind::File) && fs_menu::providers::is_image(&row.path) && !matches!(p.map(|p| &p.peek), Some(Peek::Unreadable(_))) => {
                    let got = decoded(&row.path);
                    let facts = join(vec![dims(got.as_ref().and_then(|g| g.1)).unwrap_or_default(), facts.clone()]);
                    return pane_card(picture(got.and_then(|g| g.0)), &row.label, &facts, s);
                }
                None | Some(Peek::Image) => muted(""),
                Some(Peek::Text(lines)) if lines.iter().all(|l| l.trim().is_empty()) => muted("Empty file"),
                Some(Peek::Text(lines)) => lines_el(lines),
                Some(Peek::Binary) => muted("No preview for this file"),
                Some(Peek::Folder { names, .. }) if names.is_empty() => muted("Empty folder"),
                Some(Peek::Folder { names, .. }) => {
                    let row_h = ui::measure(&w::text("Ag"), width, theme, kit).1.max(1.0) + s.xs;
                    let fits = ((bh_f / row_h).floor() as usize).max(1);
                    let col: Vec<El> = names
                        .iter()
                        .take(fits)
                        .map(|e| w::row(s.sm, vec![w::icon(if e.dir { "folder" } else { "file" }).ink(Ink::Dim), w::text(e.name.clone()).elide()]).height(row_h))
                        .collect();
                    boxed(w::column(0.0, col))
                }
                Some(Peek::Unreadable(why)) => muted(why),
            };
            (content, row.label.clone(), facts)
        }
        PaneKind::Package => return package_pane(store, row, theme, kit, width, height),
        PaneKind::Window => {
            let facts = row.meta.clone().filter(|m| !m.is_empty()).map(|ws| format!("Workspace {ws}")).unwrap_or_default();
            let content = match &m.pane_frame {
                Some(frame) => picture(Some(frame.clone())),
                None => muted(""),
            };
            (content, row.label.clone(), facts)
        }
    };
    pane_card(content, &title, &facts, s)
}

/// Pond's pane: the preview at the top, its name and facts at the foot,
/// no card of its own round them.
pub(super) fn pane_card(content: El, title: &str, facts: &str, s: &fs_theme::tokens::Space) -> El {
    let foot = vec![content, w::space((s.lg - s.row_gap).max(0.0)), w::text(title.to_owned()).weight(Weight::Medium).elide(), w::caption(facts.to_owned()).ink(Ink::Muted).elide()];
    w::column(s.row_gap, foot).fill()
}
