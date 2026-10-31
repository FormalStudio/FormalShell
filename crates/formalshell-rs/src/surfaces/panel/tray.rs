//! `trayoverflow`: the tray's second bar, every StatusNotifier item as an
//! icon in a `panel.chip` square on a headerless card, a row at a time. A click is the item's
//! Activate (its menu for an item that is only a menu), a middle click its
//! SecondaryActivate, and a right click its shell-owned menu, hung off this
//! card.

use std::cell::{Cell, RefCell};

use super::{Effect, Panel, View};
use crate::scene::Bitmap;
use crate::services::tray::{self, Item};
use crate::store::Topic;
use crate::ui::{El, Event, Size, What, w};

pub const ID: &str = "trayoverflow";

/// Items to a row before the card wraps.
const COLUMNS: usize = 8;

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

fn stop(item: &Item) -> String {
    format!("item:{}", item.id)
}

fn columns(n: usize) -> usize {
    n.clamp(1, COLUMNS)
}

impl Tray {
    fn pictures(&self, items: &[Item], size: u32) -> Vec<Option<Bitmap>> {
        let scale = crate::scene::raster_scale();
        let mut icons = self.icons.borrow_mut();
        if icons.items != items || icons.size != (size, scale) {
            let pictures = items.iter().map(|i| i.icon.as_ref().and_then(|r| r.bitmap(size, scale))).collect();
            *icons = Fitted { items: items.to_vec(), size: (size, scale), pictures };
        }
        icons.pictures.clone()
    }

    fn item<'a>(v: &'a crate::store::Store, stop: &str) -> Option<&'a Item> {
        let id = stop.strip_prefix("item:")?;
        v.tray.by_id(id)
    }

    fn click(item: &Item, what: &What, fx: &mut Effect) {
        let key = item.key.clone();
        match what {
            What::Click if item.only_menu && item.has_menu => fx.menu = Some(item.id.clone()),
            What::Right if item.has_menu => fx.menu = Some(item.id.clone()),
            What::Click => {
                fx.service(move |ctx| tray::activate(ctx, key));
                fx.close = true;
            }
            What::Middle => {
                fx.service(move |ctx| tray::secondary_activate(ctx, key));
                fx.close = true;
            }
            _ => {}
        }
    }
}

impl Panel for Tray {
    fn id(&self) -> &'static str {
        ID
    }

    fn title(&self, _: &View) -> String {
        "Tray".into()
    }

    fn reads(&self) -> &'static [Topic] {
        &[Topic::Tray]
    }

    fn header(&self) -> bool {
        false
    }

    fn width(&self, v: &View) -> f64 {
        let s = &v.theme.space;
        if v.store.tray.items.is_empty() {
            return s.panel_width;
        }
        let n = columns(v.store.tray.items.len()) as f64;
        n * s.control_height + (n - 1.0) * s.panel_gap + s.panel_padding * 2.0
    }

    fn body(&self, v: &View) -> El {
        let s = &v.theme.space;
        let items = &v.store.tray.items;
        if items.is_empty() {
            return w::panel_empty(s, "No items");
        }
        let size = v.theme.font_size.body.round().max(1.0);
        let pictures = self.pictures(items, size as u32);
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
                w::cell(w::row(0.0, vec![w::spacer(), mark, w::spacer()]).fill().height(s.control_height))
                    .cell_state(|c| {
                        c.glyph = true;
                        c.inset = Some((0.0, 0.0));
                    })
                    .interactive()
                    .width(Size::Px(s.control_height))
                    .height(s.control_height)
                    .stop(stop(item))
                    .on(stop(item))
                    .tip(tip)
            })
            .collect();
        let rows = cells.chunks(cols).map(|r| w::row(s.panel_gap, r.to_vec())).collect();
        w::column(s.panel_gap, rows)
    }

    fn columns(&self) -> usize {
        self.columns.get()
    }

    fn event(&mut self, ev: &Event, fx: &mut Effect) {
        if let Some(item) = Self::item(fx.store, &ev.on).cloned() {
            Self::click(&item, &ev.what, fx);
        }
    }

    fn activate(&mut self, stop: &str, fx: &mut Effect) {
        if let Some(item) = Self::item(fx.store, stop).cloned() {
            Self::click(&item, &What::Click, fx);
        }
    }
}
