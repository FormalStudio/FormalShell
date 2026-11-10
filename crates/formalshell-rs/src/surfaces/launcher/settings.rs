use super::*;

/// A panel module drawn as the launcher's body, with the panel host's own
/// cursor model.
pub struct Embedded {
    pub module: Box<dyn crate::surfaces::panel::Panel>,
    pub cursor: crate::surfaces::panel::host::Cursor,
    pub id: PanelId,
    /// The module's wants, taken as it starts and held while the launcher
    /// holds it.
    pub wants: crate::services::wants::Holding,
    /// `Panel::start` has run.
    pub started: bool,
    /// The module's width, as the window last read it.
    pub width: f64,
}

impl Embedded {
    pub(super) fn new(id: PanelId) -> Option<Self> {
        let mut module = crate::surfaces::panel::build(id, crate::surfaces::panel::Args::default())?;
        module.opened();
        let cursor = crate::surfaces::panel::host::Cursor::starting(module.cursor_start());
        Some(Self { module, cursor, id, wants: Default::default(), started: false, width: 0.0 })
    }
}

impl Drop for Embedded {
    fn drop(&mut self) {
        self.module.closed();
    }
}

impl Shown {
    /// The embedded panel's body, ending as far above the card's edge as
    /// its last thing stands off the sides (panel-language spec §2).
    pub(super) fn panel_el(&self, m: &Model, store: &Store, theme: &Theme) -> Option<El> {
        let p = m.panel.as_ref().filter(|_| m.on_panel())?;
        let v = crate::surfaces::panel::View {
            store,
            theme,
            output: self.output,
            cursor: p.cursor.shown(),
            hovered: self.hovered_stop(),
            now: Instant::now(),
            motion_scale: self.scale,
            raster: self.modal.win.surface.scale(),
        };
        Some(p.module.body(&v))
    }

    pub(super) fn measure_panel(&self, m: &mut Model, store: &Store, theme: &Theme, kit: &mut Kit) {
        let Some(p) = m.panel.as_mut() else { return };
        let v = crate::surfaces::panel::View {
            store,
            theme,
            output: self.output,
            cursor: None,
            hovered: None,
            now: Instant::now(),
            motion_scale: self.scale,
            raster: self.modal.win.surface.scale(),
        };
        p.width = crate::surfaces::panel::card_width(p.module.as_ref(), p.id.desc().width, &v, kit);
        let Some(el) = self.panel_el(m, store, theme) else { return };
        let width = m.panel_body(theme).1;
        m.layout.content_h = ui::measure(&el, width, theme, kit).1;
        m.layout.row_ends = ui::row_ends(&el, width, theme, kit);
        m.layout.close = w::closing_room(&theme.space, &el);
    }

    /// The smallest scroll that shows the embedded panel's cursor stop.
    pub fn follow_panel(&self, m: &mut Model) {
        let Some(st) = m.panel.as_ref().and_then(|p| p.cursor.shown()).and_then(|k| self.body.stops.iter().find(|st| st.key == k)) else { return };
        let top = st.rect.y as f64 - self.panel_origin;
        let (h, view) = (st.rect.h as f64, m.body_h);
        let mut next = m.scroll;
        if top < next {
            next = top;
        } else if top + h > next + view {
            next = top + h - view;
        }
        m.scroll = next.clamp(0.0, (m.layout.content_h - view).max(0.0));
        m.travels = true;
        m.wheeled = false;
        m.touch.settle();
    }
}
