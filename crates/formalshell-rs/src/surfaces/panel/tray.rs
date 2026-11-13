//! `trayoverflow`: the tray's second bar, every StatusNotifier item as an
//! icon in a `panel.chip` square on a headerless card, up to [`COLUMNS`] to a
//! row. A click is the item's
//! Activate (its menu for an item that is only a menu), a middle click its
//! SecondaryActivate, and a right click its shell-owned menu, hung off this
//! card.

use std::cell::{Cell, RefCell};

use super::{Effect, Nav, Out, Panel, View, Walk};
use crate::scene::Bitmap;
use crate::services::tray::{self, Item};
use crate::surfaces::bar::cell::{Action, TrayClick};
use crate::store::Topic;
use crate::ui::{Button, El, Event, On, What, w};

/// Chips to a row before the card wraps.
pub const COLUMNS: usize = 8;

pub fn columns(n: usize) -> usize {
    n.clamp(1, COLUMNS)
}

/// The pictures last fitted, and the items and size they were fitted for.
#[derive(Default)]
struct Fitted {
    items: Vec<Item>,
    size: (u32, f64),
    pictures: Vec<Option<Bitmap>>,
}

#[derive(Default)]
pub struct Tray {
    icons: RefCell<Fitted>,
    columns: Cell<usize>,
}

/// An item's chip, by its key.
struct Chip(String);

impl crate::ui::on::HitId for Chip {}

impl std::fmt::Display for Chip {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "item:{}", self.0)
    }
}

impl Tray {
    fn pictures(&self, items: &[Item], size: u32, scale: f64) -> Vec<Option<Bitmap>> {
        let mut icons = self.icons.borrow_mut();
        if icons.items != items || icons.size != (size, scale) {
            let pictures = items.iter().map(|i| i.icon.as_ref().and_then(|r| r.bitmap(size, scale))).collect();
            *icons = Fitted { items: items.to_vec(), size: (size, scale), pictures };
        }
        icons.pictures.clone()
    }

    fn item<'a>(v: &'a crate::store::Store, on: &On) -> Option<&'a Item> {
        v.tray.by_key(&on.get::<Chip>()?.0)
    }

    fn click(item: &Item, button: Button, fx: &mut Effect) {
        let key = item.key.clone();
        match button {
            Button::Left if item.only_menu && item.has_menu => fx.push(Out::Menu(key, None)),
            Button::Right if item.has_menu => fx.push(Out::Menu(key, None)),
            // Through the shell, which closes the card first and asks an
            // activation token for the window the item opens.
            Button::Left => fx.push(Out::Act(Action::Tray { key, click: TrayClick::Activate, offset: 0.0 })),
            Button::Middle => {
                fx.service(move |ctx| tray::secondary_activate(ctx, key));
                fx.push(Out::Close);
            }
            Button::Right => {}
        }
    }
}

impl Panel for Tray {
    fn title(&self, _: &View) -> String {
        "Tray".into()
    }

    fn reads(&self) -> &'static [Topic] {
        &[Topic::Tray]
    }

    fn header(&self) -> bool {
        false
    }

    fn fits(&self, v: &View) -> bool {
        !v.store.tray.items.is_empty()
    }

    fn body(&self, v: &View) -> El {
        let s = &v.theme.space;
        let items = &v.store.tray.items;
        if items.is_empty() {
            return w::panel_empty(s, "No items");
        }
        let size = v.theme.font_size.body.round().max(1.0);
        let pictures = self.pictures(items, size as u32, v.raster);
        let cols = columns(items.len());
        self.columns.set(cols);
        let cells: Vec<El> = items
            .iter()
            .zip(pictures)
            .map(|(item, picture)| {
                let mark = match picture {
                    Some(b) => w::picture(Some(b), size),
                    // No picture and no way to draw one: the shell's own
                    // unknown mark, never a stand-in from another icon set.
                    None => w::icon("circle-help").ink(crate::ui::Ink::Muted),
                };
                let tip = [&item.tooltip, &item.title, &item.id].into_iter().find(|t| !t.is_empty()).cloned().unwrap_or_default();
                w::glyph_cell(mark, s.control_height)
                    .interactive()
                    .stop(Chip(item.key.clone()))
                    .on(Chip(item.key.clone()))
                    .tip(tip)
            })
            .collect();
        let rows = cells.chunks(cols).map(|r| w::row(s.panel_gap, r.to_vec())).collect();
        w::column(s.panel_gap, rows)
    }

    fn nav(&self, _: Option<&On>) -> Nav {
        Nav { walk: Walk::Grid(self.columns.get()), steps: false }
    }

    fn event(&mut self, ev: &Event, fx: &mut Effect) {
        if ev.what == What::Click
            && let Some(item) = Self::item(fx.store, &ev.on).cloned()
        {
            Self::click(&item, ev.button, fx);
        }
    }

    fn activate(&mut self, stop: &On, fx: &mut Effect) {
        if let Some(item) = Self::item(fx.store, stop).cloned() {
            Self::click(&item, Button::Left, fx);
        }
    }
}
