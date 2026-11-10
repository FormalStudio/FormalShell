use super::*;

pub const MIRROR_ROUTE: &str = "mirror";

/// The mirror's feed: the picture in its box, or why there is none.
pub(super) fn mirror_body(m: &Model, store: &Store, theme: &Theme) -> El {
    let s = &theme.space;
    let (w, h) = m.mirror_box(theme);
    let mi = &store.mirror;
    let frame = mi.frame.as_ref().filter(|(id, _)| *id == m.mirror_current).map(|(_, b)| b.clone());
    let feed = match frame {
        Some(b) => w::picture(Some(b), h).width(Size::Px(w)),
        None => {
            let word = if !mi.listed {
                ""
            } else if mi.cameras.is_empty() {
                "No camera"
            } else if !mi.error.is_empty() {
                mi.error.as_str()
            } else {
                "Starting\u{2026}"
            };
            let line = 20.0;
            w::column(0.0, vec![w::space(((h - line) / 2.0).max(0.0)), w::text(word).ink(Ink::Muted).centred(), w::space(((h - line) / 2.0).max(0.0))])
                .width(Size::Px(w))
        }
    };
    w::column(0.0, vec![w::space(s.md), w::row(0.0, vec![w::space(s.sm), feed])])
}
