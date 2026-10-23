//! The Spaces cell's preview as the shell drives it (hover, `peek` and
//! the card's own timers): which workspace it
//! shows, the pointer's way in and out, and its window captures, live while
//! the card is open. The card itself is `surfaces::panel::workspace_preview`
//! on the panel host.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use super::App;
use super::capture::Owner;
use crate::services::{appicon, compositor};
use crate::surfaces::panel::workspace_preview::{self as card, Shared, WorkspacePreview};

/// The cell's hover delay before a card opens, and the card's grace once the
/// pointer is on neither the chip nor the card.
const HOVER: Duration = Duration::from_millis(400);
const GRACE: Duration = Duration::from_millis(200);

#[derive(Default)]
pub struct State {
    shared: Rc<RefCell<Shared>>,
    /// The open card's workspace (id, idx) and whether the pointer opened it.
    open: Option<(String, i64, bool)>,
    /// A chip waiting out the hover delay.
    hover_at: Option<(Instant, i64)>,
    close_at: Option<Instant>,
    on_chip: Option<i64>,
    on_card: bool,
    capturing: bool,
}

/// One chip off `workspaces status`: its id, idx, label, whether it would
/// preview from the pointer, and its rect in the bar window.
struct Chip {
    id: String,
    idx: i64,
    label: String,
    previews: bool,
    rect: (f64, f64, f64, f64),
}

fn chips(status: &Value) -> Vec<Chip> {
    let n = |v: &Value| v.as_f64().unwrap_or(0.0);
    status["slots"]
        .as_array()
        .map(|slots| {
            slots
                .iter()
                .map(|s| Chip {
                    id: s["id"].as_str().unwrap_or_default().to_owned(),
                    idx: s["idx"].as_i64().unwrap_or(-1),
                    label: s["label"].as_str().unwrap_or_default().to_owned(),
                    previews: s["active"] != true && s["icons"].as_array().is_some_and(|i| !i.is_empty()),
                    rect: (n(&s["rect"]["x"]), n(&s["rect"]["y"]), n(&s["rect"]["width"]), n(&s["rect"]["height"])),
                })
                .collect()
        })
        .unwrap_or_default()
}

impl App {
    fn preview_enabled(&self) -> bool {
        self.store.config.bool("workspaces.preview") != Some(false)
    }

    fn preview_up(&self) -> bool {
        self.panel.as_ref().is_some_and(|p| p.is_open() && p.id() == "workspaces")
    }

    /// Opens the card on `chip`, handing over whatever card is up.
    fn preview_show(&mut self, chip: &Chip, pointer: bool) {
        let along = if self.bar.edge().is_vertical() { chip.rect.1 + chip.rect.3 / 2.0 } else { chip.rect.0 + chip.rect.2 / 2.0 };
        if self.preview_up() {
            self.close_panels();
        }
        compositor::send(compositor::Command::RefreshWindows);
        let queries: Vec<appicon::Query> = self
            .store
            .compositor
            .snapshot
            .windows
            .iter()
            .filter(|w| w.workspace_id == chip.id)
            .map(|w| appicon::Query {
                id: card::icon_key(&w.id),
                app_id: w.app_id.clone(),
                initial_class: w.initial_class.clone(),
                initial_title: w.initial_title.clone(),
                pid: w.pid,
            })
            .collect();
        appicon::probe((self.store.theme.theme.space.huge * 2.0).round() as u32, crate::scene::raster_scale(), queries);
        let module = WorkspacePreview::new(chip.id.clone(), chip.idx, chip.label.clone(), pointer, self.peek.shared.clone());
        self.peek.open = Some((chip.id.clone(), chip.idx, pointer));
        self.peek.hover_at = None;
        self.peek.close_at = None;
        let now = Instant::now();
        self.open_host(Box::new(module), Some(along), false, now);
    }

    /// `workspaces peek <n>`.
    pub fn preview_peek(&mut self, n: i64) -> String {
        let Some(status) = self.bar.workspaces_status() else { return "error: no workspaces cell on any bar".into() };
        if !self.preview_enabled() {
            return "error: preview is off (workspaces.preview)".into();
        }
        match chips(&status).into_iter().find(|c| c.idx == n) {
            Some(chip) => {
                self.preview_show(&chip, false);
                "ok".into()
            }
            None => format!("error: no workspace {n} on {}", status["output"].as_str().unwrap_or_default()),
        }
    }

    pub fn preview_close(&mut self) {
        if self.preview_up() {
            self.close_panels();
        }
    }

    /// The pointer moved: onto a chip that previews, onto the card, or off
    /// both. `owner_bar`/`owner_card` say which surface it is on.
    pub(super) fn preview_pointer(&mut self, on_bar: bool, on_card: bool, at: (f64, f64)) {
        let now = Instant::now();
        let chip = if on_bar && self.preview_enabled() {
            self.bar.workspaces_status().map(|s| chips(&s)).and_then(|cs| {
                cs.into_iter().find(|c| at.0 >= c.rect.0 && at.0 < c.rect.0 + c.rect.2 && at.1 >= c.rect.1 && at.1 < c.rect.1 + c.rect.3)
            })
        } else {
            None
        };
        let pointer_card = self.preview_up() && self.peek.open.as_ref().is_some_and(|o| o.2);
        self.peek.on_card = on_card && pointer_card;
        let previewing = chip.as_ref().filter(|c| c.previews);
        self.peek.on_chip = previewing.map(|c| c.idx);
        match previewing {
            Some(c) if pointer_card => {
                if self.peek.open.as_ref().is_some_and(|o| o.1 == c.idx) {
                    self.peek.close_at = None;
                } else {
                    let c = Chip { id: c.id.clone(), idx: c.idx, label: c.label.clone(), previews: true, rect: c.rect };
                    self.preview_show(&c, true);
                }
            }
            Some(c) => {
                if self.peek.hover_at.is_none_or(|(_, idx)| idx != c.idx) {
                    self.peek.hover_at = Some((now + HOVER, c.idx));
                }
            }
            None => {
                self.peek.hover_at = None;
                if pointer_card && !self.peek.on_card {
                    self.peek.close_at.get_or_insert(now + GRACE);
                } else if self.peek.on_card {
                    self.peek.close_at = None;
                }
            }
        }
    }

    pub(super) fn preview_deadline(&self) -> Option<Instant> {
        [self.peek.hover_at.map(|h| h.0), self.peek.close_at].into_iter().flatten().min()
    }

    /// The timers that came due, the card's own close when its workspace is
    /// the one on screen, and the captures following the card.
    pub(super) fn preview_present(&mut self) {
        let now = Instant::now();
        if let Some((at, idx)) = self.peek.hover_at
            && at <= now
        {
            self.peek.hover_at = None;
            let chip = self.bar.workspaces_status().map(|s| chips(&s)).and_then(|cs| cs.into_iter().find(|c| c.idx == idx));
            if let Some(c) = chip.filter(|c| c.previews && self.peek.on_chip == Some(idx)) {
                self.preview_show(&c, true);
            }
        }
        if self.peek.close_at.is_some_and(|t| t <= now) {
            self.peek.close_at = None;
            if self.peek.on_chip.is_none() && !self.peek.on_card && self.peek.open.as_ref().is_some_and(|o| o.2) {
                self.preview_close();
            }
        }
        if !self.preview_up() {
            self.peek.open = None;
        }
        let shown = self.panel.as_ref().is_some_and(|p| p.id() == "workspaces");
        if shown {
            let wants: Vec<(String, (u32, u32))> = self
                .peek
                .shared
                .borrow()
                .laid
                .tiles
                .iter()
                .map(|(id, r, _)| (id.clone(), (r.2.round().max(1.0) as u32, r.3.round().max(1.0) as u32)))
                .collect();
            let scale = self.panel.as_ref().map_or(1.0, |p| p.surface.scale());
            self.capture_set(Owner::Preview, &wants, true, scale);
            self.peek.capturing = true;
        } else if self.peek.capturing {
            self.peek.capturing = false;
            self.capture_clear(Owner::Preview);
            self.peek.shared.borrow_mut().thumbs.clear();
        }
    }

    /// A capture landed: the card reads it on its next draw.
    pub(super) fn preview_thumbs_changed(&mut self) {
        let ids: Vec<String> = self.peek.shared.borrow().laid.tiles.iter().map(|t| t.0.clone()).collect();
        let thumbs = ids.into_iter().filter_map(|id| self.capture_image(Owner::Preview, &id).cloned().map(|b| (id, b))).collect();
        self.peek.shared.borrow_mut().thumbs = thumbs;
        self.panel_dirty = true;
    }

    /// The workspace the card shows became the one on screen.
    pub fn preview_changed(&mut self, topic: crate::store::Topic) {
        if topic != crate::store::Topic::Compositor {
            return;
        }
        let focused = &self.store.compositor.snapshot.focused_workspace_id;
        if self.preview_up() && self.peek.open.as_ref().is_some_and(|o| !o.0.is_empty() && o.0 == *focused) {
            self.preview_close();
        }
    }

    /// `workspaces status`'s `preview` block.
    pub fn preview_status(&self) -> Value {
        let Some((_, idx, pointer)) = self.peek.open.clone().filter(|_| self.preview_up()) else {
            return json!({"open": false, "idx": -1, "windows": 0, "captured": 0, "keyboard": false, "miniature": null, "rect": null});
        };
        let shared = self.peek.shared.borrow();
        let laid = &shared.laid;
        let captured = laid.tiles.iter().filter(|t| shared.thumbs.contains_key(&t.0)).count();
        let r = |v: f64| v.round() as i64;
        let thumbs: Vec<Value> = laid
            .tiles
            .iter()
            .filter_map(|(id, b, rect)| {
                let rect = rect.as_ref()?;
                Some(json!({
                    "id": id, "x": r(b.0), "y": r(b.1), "width": r(b.2), "height": r(b.3),
                    "rect": {"x": rect.x, "y": rect.y, "width": rect.width, "height": rect.height},
                }))
            })
            .collect();
        let home = laid.plan.as_ref().map_or((0.0, 0.0), |p| (p.home.x, p.home.y));
        let frame = self.panel.as_ref().map(|p| p.card.rest_rect()).unwrap_or_default();
        json!({
            "open": true,
            "idx": idx,
            "windows": laid.tiles.len(),
            "captured": captured,
            "keyboard": !pointer,
            "miniature": {
                "scale": laid.scale,
                "inset": laid.inset,
                "view": {"width": r(laid.view.0), "height": r(laid.view.1)},
                "content": {"width": r(laid.content.0), "height": r(laid.content.1)},
                "scroll": {"x": r(shared.scroll.0), "y": r(shared.scroll.1)},
                "home": {"x": r(home.0), "y": r(home.1)},
                "thumbs": thumbs,
            },
            "rect": {"x": frame.x, "y": frame.y, "width": frame.w, "height": frame.h},
        })
    }
}
