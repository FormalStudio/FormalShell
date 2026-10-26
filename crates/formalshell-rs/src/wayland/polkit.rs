//! The polkit dialog: a modal card on the top layer
//! (`surfaces::modal`), budding off the top line to the output's centre
//! over its scrim and holding the keyboard while a request is open:
//! "Authentication required", the action's message, the identity, the
//! password field and Cancel/Authenticate.

use std::time::Instant;

use smithay_client_toolkit::reexports::client::protocol::wl_surface::WlSurface;
use smithay_client_toolkit::seat::keyboard::{KeyEvent, Keysym};
use smithay_client_toolkit::shell::WaylandSurface;
use smithay_client_toolkit::shell::wlr_layer::{Layer, LayerSurface};
use vello_cpu::kurbo::Rect;
use fs_menu::field::{Field, Outcome};
use zeroize::Zeroizing;

use super::App;
use super::lock::{LockMsg, buffer, field};
use crate::scene::{Bitmap, IRect, NodeId};
use crate::services::clipboard;
use crate::services::polkit::{self, Cmd, Event};
use crate::surfaces::bar::cell::Painter;
use crate::surfaces::modal::{Modal, Part};
use crate::ui::{self, El, Size, Ui, Variant, w};

const NAMESPACE: &str = "formalshell:polkit";

/// The drawer's default `deformAmount`, which the dialog leaves alone.
const DEFORM_AMOUNT: f64 = 0.15;

pub struct Dialog {
    pub(super) modal: Modal,
    ui: Ui,
    avatar_nodes: Vec<NodeId>,
    /// The identity's picture, when it is this session's own account.
    avatar: Option<Bitmap>,
    message: String,
    identity: String,
    prompt: String,
    echo: bool,
    /// Waiting on the helper for a prompt, or on its verdict.
    asked: bool,
    submitted: bool,
    error: bool,
    text: Field<Zeroizing<String>>,
    dirty: bool,
}

impl Dialog {
    pub(super) fn set_avatar(&mut self, avatar: Option<Bitmap>) {
        self.avatar = avatar;
        self.dirty = true;
    }
}

/// Word wrap for body text: greedy lines no wider than `width`.
fn wrap(text: &str, width: f64, theme: &fs_theme::theme::Theme, kit: &mut crate::surfaces::bar::cell::Kit) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    let mut line = String::new();
    for word in text.split_whitespace() {
        let candidate = if line.is_empty() { word.to_owned() } else { format!("{line} {word}") };
        if !line.is_empty() && ui::measure(&w::text(&candidate), 1.0e6, theme, kit).0 > width {
            lines.push(std::mem::replace(&mut line, word.to_owned()));
        } else {
            line = candidate;
        }
    }
    if !line.is_empty() {
        lines.push(line);
    }
    lines
}

impl App {
    pub(super) fn polkit_event(&mut self, event: Event) {
        match event {
            Event::Begin { message, identity } => {
                let own = !identity.is_empty() && std::env::var("USER").is_ok_and(|u| u == identity);
                let mut modal = self.new_modal(["polkit", "polkit-scrim-band", "polkit-scrim"], NAMESPACE, Layer::Overlay, DEFORM_AMOUNT);
                self.carry_popover_look(&mut modal);
                let content = self.content_layer("polkit-content", &modal.surface);
                let top = content.top;
                modal.layer = Some(content);
                self.polkit = Some(Dialog {
                    modal,
                    ui: Ui::new(Some(top)),
                    avatar_nodes: Vec::new(),
                    avatar: None,
                    message,
                    identity,
                    prompt: String::new(),
                    echo: false,
                    asked: false,
                    submitted: false,
                    error: false,
                    text: field(),
                    dirty: true,
                });
                if own && let (Some(rt), Some(tx)) = (&self.runtime, self.lock.tx.clone()) {
                    let home = std::env::var("HOME").unwrap_or_default();
                    let path = self.store.config.avatar_path(&home);
                    let scale = self.polkit.as_ref().map_or_else(crate::scene::raster_scale, |d| d.modal.surface.scale());
                    let px = crate::scene::pixels_at((self.store.theme.theme.space.xxl * 2.0).round(), scale);
                    rt.pool().submit(move || {
                        let _ = tx.send(LockMsg::PolkitAvatar(crate::surfaces::lock::avatar(&path, px).map(|b| b.at(scale))));
                    });
                }
            }
            Event::Prompt { prompt, echo } => {
                if let Some(d) = &mut self.polkit {
                    (d.prompt, d.echo, d.asked, d.submitted, d.dirty) = (prompt, echo, true, false, true);
                }
            }
            Event::Failed => {
                if let Some(d) = &mut self.polkit {
                    (d.error, d.submitted, d.asked, d.dirty) = (true, false, false, true);
                    d.text = field();
                }
            }
            Event::Done => {
                if let Some(d) = &mut self.polkit {
                    d.modal.close(Instant::now());
                    d.dirty = true;
                }
            }
        }
    }

    fn polkit_part(&self, surface: &WlSurface) -> Option<Part> {
        self.polkit.as_ref()?.modal.part(surface)
    }

    pub(super) fn polkit_owns(&self, layer: &LayerSurface) -> bool {
        self.polkit_part(layer.wl_surface()).is_some()
    }

    pub(super) fn polkit_configure(&mut self, layer: &LayerSurface, width: i32, height: i32) {
        let Some(part) = self.polkit_part(layer.wl_surface()) else { return };
        let Some(d) = &mut self.polkit else { return };
        d.modal.configure(part, width, height);
        d.dirty = true;
    }

    pub(super) fn polkit_frame(&mut self, surface: &WlSurface) -> bool {
        let Some(part) = self.polkit_part(surface) else { return false };
        let Some(d) = &mut self.polkit else { return false };
        if d.modal.frame(part, Instant::now()) {
            d.dirty = true;
            self.sync_join();
        }
        true
    }

    pub(super) fn polkit_joins(&self) -> Vec<(fs_chrome::types::Edge, f64, f64, f64)> {
        self.polkit.as_ref().map_or_else(Vec::new, |d| d.modal.joins().collect())
    }

    /// The clipboard's text into the dialog's field, while it holds the
    /// keyboard.
    pub(super) fn polkit_paste(&mut self, text: &str) -> bool {
        let Some(d) = self.polkit.as_mut().filter(|d| d.modal.open) else { return false };
        if d.asked && !d.submitted && d.text.paste(text) {
            d.dirty = true;
        }
        true
    }

    /// The dialog's keys while it holds the keyboard.
    pub(super) fn polkit_key(&mut self, event: &KeyEvent) -> bool {
        let Some(d) = self.polkit.as_mut().filter(|d| d.modal.open) else { return false };
        let enabled = d.asked && !d.submitted;
        match event.keysym {
            Keysym::Escape => {
                d.text = field();
                polkit::send(Cmd::Cancel);
            }
            Keysym::Return | Keysym::KP_Enter if enabled => {
                (d.submitted, d.error) = (true, false);
                let answer = d.text.replace(buffer());
                polkit::send(Cmd::Submit(answer));
            }
            sym if enabled => {
                d.text.secret = !d.echo;
                match d.text.key(super::field_key(sym), self.mods, event.utf8.as_deref()) {
                    Outcome::Copy(t) | Outcome::Cut(t) => clipboard::command(clipboard::Cmd::SetText(t)),
                    Outcome::Paste => clipboard::command(clipboard::Cmd::Paste),
                    _ => {}
                }
            }
            _ => {}
        }
        d.dirty = true;
        true
    }

    pub(super) fn present_polkit(&mut self, now: Instant) {
        if self.polkit.as_ref().is_some_and(|d| d.modal.finished(now)) {
            self.polkit = None;
            self.sync_join();
            return;
        }
        let qh = self.qh.clone();
        let Some(d) = &mut self.polkit else { return };
        // The card's own motion moves and cuts the content layer, which
        // needs no new layout for it.
        if (d.dirty || !d.modal.surface.mapped) && d.modal.surface.configured {
            d.dirty = false;
            d.modal.sync_region(&self.compositor);
            let _raster = crate::scene::raster_at(d.modal.surface.scale());
            d.ui.motion_scale = self.motion_scale;
            Self::lay_polkit(d, &self.store.theme.theme, &mut self.bar.kit, now);
        }
        // A content effect still running (an error label's shake) lays out
        // again on the next frame.
        let animating = d.modal.animating(now) || d.dirty;
        d.modal.present(animating, now, &qh);
        if animating {
            self.sync_join();
        }
    }

    fn lay_polkit(d: &mut Dialog, theme: &fs_theme::theme::Theme, kit: &mut crate::surfaces::bar::cell::Kit, now: Instant) {
        let s = &theme.space;
        let (w, h) = (d.modal.card.scene.size.w, d.modal.card.scene.size.h);
        let inner = s.popup_width_narrow;
        let enabled = d.asked && !d.submitted;
        let shown = if d.echo { d.text.text().to_owned() } else { "\u{2022}".repeat(d.text.text().chars().count()) };
        let placeholder = if d.submitted {
            "Checking".to_owned()
        } else if d.prompt.trim().is_empty() {
            "Enter Password".to_owned()
        } else {
            d.prompt.trim().to_owned()
        };
        let mut rows: Vec<El> = vec![w::section_label(s, "Authentication required", None, false)];
        rows.push(w::column(0.0, wrap(&d.message, inner, theme, kit).into_iter().map(w::text).collect()));
        // The avatar's slot: the picture goes in over the space the row
        // leaves for it.
        let avatar = d.avatar.as_ref().map(|b| b.logical().0.round());
        let mut avatar_row = None;
        if !d.identity.is_empty() {
            let name = w::value(&d.identity).elide();
            let line = match avatar {
                Some(size) => {
                    // The name sits centred on the picture's own height.
                    let h = ui::measure(&name, inner, theme, kit).1;
                    let v = ((size - h) / 2.0).max(0.0);
                    w::row(s.icon_gap, vec![w::space(size), name.fill().pad(0.0, v, 0.0, v)])
                }
                None => name,
            };
            avatar_row = Some(rows.len());
            rows.push(w::column(s.row_gap, vec![w::section_label(s, "Identity", None, false), line]));
        }
        rows.push(w::input(&shown, &placeholder, enabled, d.error.then_some("Wrong password")).caret(d.text.caret_chars()).width(Size::Px(inner)).enabled(enabled));
        let cancel = w::button("Cancel").variant(Variant::Outline);
        let auth = w::button("Authenticate").enabled(enabled);
        let buttons = w::row(s.control_gap, vec![cancel, auth]);
        let (buttons_w, _) = ui::measure(&buttons, inner, theme, kit);
        rows.push(w::row(0.0, vec![w::space((inner - buttons_w).max(0.0)), buttons]));
        let heights: Vec<f64> = rows.iter().map(|r| ui::measure(r, inner, theme, kit).1).collect();
        let body = w::column(s.section_gap, rows).width(Size::Px(inner));
        let (_, body_h) = ui::measure(&body, inner, theme, kit);
        let card_w = inner + s.panel_padding * 2.0;
        let card_h = body_h + s.panel_padding * 2.0;
        let x0 = ((w as f64 - card_w) / 2.0).round();
        let y0 = ((h as f64 - card_h) / 2.0).round();
        d.modal.place(Rect::new(x0, y0, x0 + card_w, y0 + card_h), now);
        // Laid out in the content layer's own pixels, which the card's
        // travel, cut and fade move.
        let rest = d.modal.card.content_rest();
        let frame = IRect::new(0, 0, rest.w, rest.h);
        let alpha = 1.0;
        let clip = frame;
        let Some(layer) = &mut d.modal.layer else { return };
        layer.fit(frame.w, frame.h);
        let top = layer.top;
        let scene = &mut layer.scene;
        let (x, y) = (frame.x as f64 + s.panel_padding, frame.y as f64 + s.panel_padding);
        let mut p = Painter::new(scene, &mut d.avatar_nodes, Some(clip)).after(Some(top));
        if let (Some(i), Some(b)) = (avatar_row, d.avatar.as_ref()) {
            // Under the Identity label and its gap, centred on the row.
            let label_h = ui::measure(&w::section_label(s, "Identity", None, false), inner, theme, kit).1;
            let above: f64 = heights[..i].iter().map(|h| h + s.section_gap).sum();
            let side = b.logical().0.round();
            let line_h = (heights[i] - label_h - s.row_gap).max(side);
            let at = (x.round() as i32, (y + above + label_h + s.row_gap + (line_h - side) / 2.0).round() as i32);
            p.image(b, at, alpha);
        }
        let after = p.last();
        p.finish();
        d.ui.anchor = after.or(Some(top));
        let drawn = d.ui.draw(&body, Rect::new(x, y, x + inner, y + body_h), Some(clip), alpha, theme, kit, scene, now);
        d.dirty = drawn.animating;
    }
}
