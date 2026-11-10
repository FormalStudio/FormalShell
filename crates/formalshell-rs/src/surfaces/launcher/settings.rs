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
