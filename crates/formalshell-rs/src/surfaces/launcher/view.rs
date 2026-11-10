use super::*;

/// A `LAUNCHER` length in whole pixels.
pub(super) fn launcher_px(base: f64) -> f64 {
    base.round()
}

/// The window: the card, its surface and the three bands' widgets.
pub struct Shown {
    pub modal: Modal,
    head: Ui,
    pub(super) body: Ui,
    preview: Ui,
    /// The mirror's feed box and the picture inside it, on the surface.
    pub feed: Option<(IRect, Option<IRect>)>,
    /// The body's viewport on the output, for `menu status`.
    pub body_rect: Option<IRect>,
    foot: Ui,
    rules: Vec<NodeId>,
    pub output: (f64, f64),
    pub(super) scale: f64,
    pub wake: Option<Instant>,
    /// The card's top, settled: the scroll's own animation.
    scroll: Animated,
    /// The actions column's own, and its box on the surface while it is
    /// drawn.
    column_scroll: Animated,
    column_rect: Option<Rect>,
    /// The field band over the rule, which the token and the query centre in.
    field_rect: Option<Rect>,
    /// The preview pane's box on the surface while it is drawn, and when
    /// it last arrived.
    pane_rect: Option<Rect>,
    pane_since: Option<Instant>,
    /// The card's width and height travelling between level kinds.
    morph: (Animated, Animated),
    /// The level and halves the card was last sized for, and the query.
    sized_for: Option<(SizeKey, String)>,
    /// The card size the content was last laid out for.
    laid_for: (f64, f64),
    /// The view the content was last laid out for.
    laid_view: Option<ViewId>,
    /// The cursor's fill and ring, one for the whole body, painted between
    /// `sel_anchor` and the body's own nodes so the cells draw over it.
    sel_anchor: NodeId,
    sel_nodes: Vec<NodeId>,
    /// Its box in body content coordinates (x, y, w, h), travelling.
    sel: [Animated; 4],
    /// The level and query it was last drawn on, while it is shown: a move
    /// inside the same list travels, anything else lands at once.
    sel_on: Option<(Option<String>, String)>,
    /// The card centred both ways, as Pond's settings is.
    centred: bool,
    /// The embedded panel's content top on the surface, scroll included.
    pub(super) panel_origin: f64,
}

impl Shown {
    /// The launcher's deform amount, lighter than a popout card's.
    pub const DEFORM_AMOUNT: f64 = 0.1;

    pub fn new(theme: &Theme, mut modal: Modal, output: (f64, f64), scale: f64) -> Self {
        let top = modal.layer.as_ref().map_or(modal.win.card.top_node(), |l| l.top);
        let scene = match &mut modal.layer {
            Some(l) => &mut l.scene,
            None => &mut modal.win.card.scene,
        };
        let mut marker = |prev: NodeId| {
            let id = scene.add_after(Some(prev), IRect::default(), crate::scene::Paint::Rect { fill: fs_theme::color::Rgba::TRANSPARENT, radius: 0.0 });
            scene.set_visible(id, false);
            id
        };
        let sel_anchor = marker(top);
        let body_anchor = marker(sel_anchor);
        let sel = || Animated::new(0.0, Clock::SpatialFast.curve());
        Self {
            modal,
            head: Ui::new(Some(top)),
            body: Ui::new(Some(body_anchor)),
            preview: Ui::new(Some(top)),
            feed: None,
            body_rect: None,
            foot: Ui::new(Some(top)),
            rules: Vec::new(),
            output,
            scale,
            wake: None,
            scroll: Animated::new(0.0, Clock::SpatialFast.curve()),
            column_scroll: Animated::new(0.0, Clock::SpatialFast.curve()),
            column_rect: None,
            field_rect: None,
            pane_rect: None,
            pane_since: None,
            laid_for: (0.0, 0.0),
            laid_view: None,
            morph: (Animated::new(theme.space.popup_width_menu, Clock::Spatial.curve()), Animated::new(0.0, Clock::Spatial.curve())),
            sized_for: None,
            sel_anchor,
            sel_nodes: Vec::new(),
            sel: [sel(), sel(), sel(), sel()],
            sel_on: None,
            centred: false,
            panel_origin: 0.0,
        }
    }

    pub fn finished(&self, now: Instant) -> bool {
        self.modal.win.finished(now)
    }

    pub fn animating(&self, now: Instant) -> bool {
        self.modal.win.animating(now)
            || self.scroll.running(now)
            || self.column_scroll.running(now)
            || self.morph.0.running(now)
            || self.morph.1.running(now)
            || self.sel.iter().any(|a| a.running(now))
            || self.wake.is_some_and(|w| w <= now)
    }

    /// Whether the content itself moves, past the card carrying it: on a
    /// layer, the card's own motion and its size morph need no new layout,
    /// the compositor moves and cuts the layer.
    pub fn content_animating(&self, now: Instant) -> bool {
        let card = self.modal.layer.is_none() && (self.modal.win.animating(now) || self.morph.0.running(now) || self.morph.1.running(now));
        card || self.scroll.running(now) || self.column_scroll.running(now) || self.wake.is_some_and(|w| w <= now)
    }

    /// The field's line, the head band's height under the card's top and
    /// the foot band's, each band with its rule.
    fn metrics(theme: &Theme, m: &Model, store: &Store, output_h: f64, kit: &mut Kit) -> (f64, f64, f64, f64) {
        let s = &theme.space;
        let line = ui::measure(&w::text("Ag").size(Type::Field), 100.0, theme, kit).1.max(s.control_height);
        // A held page wears a panel card's frame: its header row, hairline
        // and the room under its body.
        let page = m.on_panel().then(|| crate::surfaces::panel::Frame::new(theme, true));
        let head = match page {
            Some(f) => f.body(),
            None => launcher_px(fs_theme::tokens::LAUNCHER.field_band) + theme.border_width,
        };
        let foot = match page {
            Some(f) => f.foot + m.layout.close,
            None if m.footless() => 0.0,
            None => theme.border_width + s.control_height,
        };
        let cap = (m.kind_height(theme, output_h) - head - foot).max(0.0);
        let body = if m.mode == Mode::Input {
            0.0
        } else if m.on_panel() {
            ui::fit_rows(&m.layout.row_ends, m.layout.content_h, cap)
        } else if m.content_tall() {
            // The card hugs its list.
            let pane = match m.pane() {
                Some(row) => {
                    let (_, pw, _) = m.pane_extent(theme);
                    let inner = (cap - s.lg * 2.0).max(0.0);
                    ui::measure(&preview_el(m, store, row, theme, kit, pw, inner), pw, theme, kit).1.min(inner) + s.lg * 2.0
                }
                None => 0.0,
            };
            // An empty level has no rows to hug: room for its words, a
            // control height either side.
            let content = if m.rows.is_empty() {
                ui::measure(&empty_words(m, s), m.body_width(theme), theme, kit).1 + s.control_height * 2.0
            } else {
                m.layout.content_h
            };
            let tall = content.max(pane);
            if tall > cap && pane == 0.0 && m.view == View::Rows { m.whole_rows(cap, s.lg) } else { tall.min(cap) }
        } else {
            cap
        };
        (line, head, foot, body)
    }

    /// The stop under the pointer, whichever of its parts it is on, as the
    /// panel host reads it.
    pub fn hovered_stop(&self) -> Option<String> {
        let path = self.body.hover.as_ref()?;
        let hit = self.body.hits.iter().find(|h| &h.path == path)?;
        if hit.stop.is_some() {
            return hit.stop.clone();
        }
        let r = hit.rect;
        self.body
            .hits
            .iter()
            .filter(|h| h.stop.is_some() && h.rect.x <= r.x && h.rect.y <= r.y && h.rect.right() >= r.right() && h.rect.bottom() >= r.bottom())
            .min_by_key(|h| h.rect.w * h.rect.h)
            .and_then(|h| h.stop.clone())
    }

    /// The body's stops, for the embedded panel's cursor.
    pub fn stops(&self) -> &[ui::Stop] {
        &self.body.stops
    }

    /// The card at this frame of its size morph. Off a top bar it is
    /// centred across the output, its top 30% down. Off a side bar it is
    /// centred both ways in the area the bar leaves: the windows tile in
    /// that area, so its centre is where the eye reads the screen's centre
    /// beside a 160px column. On a layer that is all a frame of the morph
    /// does: the content stays laid out at the size the card is headed for.
    pub fn place_card(&mut self, theme: &Theme, now: Instant) {
        let (w_, h_) = (self.morph.0.value(now).round(), self.morph.1.value(now).round());
        let pad = theme.space.panel_padding;
        let near = |extent: f64, size: f64| {
            let max = extent - size - pad;
            if max < pad { pad } else { (extent * 0.3).clamp(pad, max) }.round()
        };
        let centred = |extent: f64, size: f64| ((extent - size) / 2.0).max(pad).round();
        let line = self.modal.win.card.line_at();
        let (x, y) = match self.modal.win.card.edge {
            Edge::Left => (line + centred(self.output.0 - line, w_), centred(self.output.1, h_)),
            Edge::Right => (centred(self.output.0 - line, w_), centred(self.output.1, h_)),
            _ if self.centred => (((self.output.0 - w_) / 2.0).round(), centred(self.output.1, h_)),
            _ => (((self.output.0 - w_) / 2.0).round(), near(self.output.1, h_)),
        };
        self.modal.place(Rect::new(x, y, x + w_, y + h_), now);
    }

    /// Lays the card out and draws the three bands.
    pub fn layout(&mut self, m: &mut Model, store: &Store, theme: &Theme, kit: &mut Kit, now: Instant) {
        m.output_w = self.output.0;
        m.relayout(theme, kit);
        self.centred = m.on_panel();
        if m.on_panel() {
            self.measure_panel(m, store, theme, kit);
        }
        let (line, head_h, foot_h, body_h) = Self::metrics(theme, m, store, self.output.1, kit);
        let chrome = head_h + foot_h;
        if (m.body_h - body_h).abs() > 0.5 {
            m.body_h = body_h;
            m.follow();
        }
        // A new level, or the column or pane taking half the card, travels
        // into its size. A re-rank inside a level lands at once, a typed
        // query's above all: the list for it is whole on the next frame.
        // The first frame after an open lands on it too.
        let (tw, th) = (m.card_width(theme), chrome + body_h);
        self.modal.look_hint(tw.round() as i32, th.round() as i32);
        let key = m.size_key();
        // The query the list is ranked by: the actions column takes the
        // field over and gives it back on close, which is no typing.
        let query = m.list_text().to_owned();
        let typed = self.sized_for.as_ref().is_some_and(|(k, q)| k.level == key.level && *q != query);
        let resized = self.sized_for.as_ref().is_some_and(|(k, _)| *k != key);
        let morphing = self.morph.0.running(now) || self.morph.1.running(now);
        self.sized_for = Some((key, query));
        if self.modal.win.surface.mapped() && self.modal.open && !typed && (resized || morphing) {
            let ms = Clock::Spatial.ms(theme) * self.scale;
            self.morph.0.set(now, tw, ms);
            self.morph.1.set(now, th, ms);
        } else {
            self.morph.0.jump(tw);
            self.morph.1.jump(th);
        }
        self.place_card(theme, now);
        if let Some(coast) = m.touch.tick(now, m.max_scroll()) {
            m.scroll = coast;
            m.travels = false;
            m.wheeled = false;
        }
        glide(&mut self.scroll, now, m.scroll, m.wheeled, m.travels, theme, self.scale);
        match &mut m.column {
            Some(c) => {
                c.row_h = launcher_px(fs_theme::tokens::LAUNCHER.row_height);
                c.view_h = (body_h - theme.space.sm * 2.0).max(0.0);
                c.scroll = c.scroll.min(c.max_scroll());
                if let Some(coast) = c.touch.tick(now, c.max_scroll()) {
                    c.scroll = coast;
                    c.wheeled = false;
                    c.travels = false;
                }
                glide(&mut self.column_scroll, now, c.scroll, c.wheeled, c.travels, theme, self.scale);
            }
            None => self.column_scroll.jump(0.0),
        }
        self.draw(m, store, theme, kit, now, line, body_h, (tw, th));
        if let Some(p) = m.panel.as_mut() {
            p.cursor.sync(&self.body.stops);
            p.cursor.travels = false;
        }
        self.laid_for = (tw, th);
        self.laid_view = Some(m.view_id());
    }

    /// On a layer, the card is morphing toward a size its content is
    /// already laid out for: another layout (a monitor refresh, rows
    /// settling) waits for the morph to land, since on a starved CPU one
    /// layout of a long view outlasts several frames of the morph. A key,
    /// click or call that moved the view is never held.
    pub fn hold_layout(&self, m: &Model, now: Instant) -> bool {
        let morphing = self.morph.0.running(now) || self.morph.1.running(now);
        morphing
            && self.modal.layer.is_some()
            && self.laid_for == (self.morph.0.target(), self.morph.1.target())
            && self.laid_view.as_ref() == Some(&m.view_id())
    }

    #[allow(clippy::too_many_arguments)]
    fn draw(&mut self, m: &Model, store: &Store, theme: &Theme, kit: &mut Kit, now: Instant, line: f64, body_h: f64, target: (f64, f64)) {
        // On a layer the content is laid out in the layer's own pixels at
        // the size the card is headed for: the compositor moves, cuts and
        // fades it with the card.
        let (frame, alpha, clip) = match &self.modal.layer {
            Some(_) => {
                let size = IRect::new(0, 0, target.0.round() as i32, target.1.round() as i32);
                (size, 1.0, size)
            }
            None => (self.modal.win.card.content.0, self.modal.win.card.content.1, self.modal.win.card.content_clip()),
        };
        let s = theme.space.clone();
        let pad = s.panel_padding;
        let panel = self.panel_el(m, store, theme);
        let (fx, fy, fw) = (frame.x as f64, frame.y as f64, frame.w as f64);
        let inner_w = (fw - pad * 2.0).max(0.0);
        self.wake = None;
        // Where the layer's pixels sit on the output once settled, for the
        // rects reported out.
        if let Some(l) = &mut self.modal.layer {
            l.fit(frame.w, frame.h);
        }
        let on_output = match &self.modal.layer {
            Some(l) => { let r = l.place(&self.modal.win.card, true); (r.x, r.y) }
            None => (0, 0),
        };
        let scene = match &mut self.modal.layer {
            Some(l) => &mut l.scene,
            None => &mut self.modal.win.card.scene,
        };
        self.head.motion_scale = self.scale;
        self.body.motion_scale = self.scale;
        self.foot.motion_scale = self.scale;

        // The field band: the line centred in it `xxl` in from either side,
        // a rule as far in under it. A held page's band is a panel card's
        // header, row and hairline where the panel host draws them.
        let page = m.on_panel().then(|| crate::surfaces::panel::Frame::new(theme, true));
        let (band, row_top, row_h) = match page {
            Some(f) => (f.rule(theme), fy + f.top, f.head),
            None => {
                let band = launcher_px(fs_theme::tokens::LAUNCHER.field_band);
                (band, fy + ((band - line) / 2.0).round(), line)
            }
        };
        let field_x = m.field_inset(theme);
        let head_rect = Rect::new(fx + field_x, row_top, fx + fw - field_x, row_top + row_h);
        // The row stands the band's height whatever it holds, so a token
        // with no query beside it centres in the band too.
        let head = header(m, store, theme, self.modal.win.surface.scale()).height(row_h);
        self.field_rect = Some(Rect::new(fx, fy, fx + fw, fy + band));
        let d = self.head.draw(&head, head_rect, Some(clip), alpha, theme, kit, scene, now);
        if d.animating {
            self.wake = Some(now);
        }
        let bw = theme.border_width.round().max(1.0) as i32;
        let rule_top = (fy + band).round() as i32;
        let body_top = match page {
            Some(f) => fy + f.body(),
            None => rule_top as f64 + theme.border_width,
        };
        let (body_x, body_w) = if m.on_panel() {
            let (x, w_) = m.panel_body(theme);
            (fx + x, w_)
        } else if m.bleeds() {
            (fx, fw)
        } else {
            (fx + pad, inner_w)
        };
        let sep = theme.box_style(Role::Separator, None).fill;
        let sep = sep.with_alpha(sep.a * alpha);

        // The body.
        let viewport = IRect::new(body_x.round() as i32, body_top.round() as i32, body_w.round() as i32, body_h.round() as i32);
        let rows_end = IRect::new(viewport.x, viewport.y, viewport.w, (body_h - m.layout.foot).max(0.0).round() as i32);
        let body_clip = rows_end.intersect(&clip);
        let view_clip = viewport.intersect(&clip);
        self.body_rect = (body_h > 0.0).then(|| IRect::new(viewport.x + on_output.0, viewport.y + on_output.1, viewport.w, viewport.h));
        let scroll = self.scroll.value(now) + m.touch.value(now);
        let body = match panel {
            Some(el) => el,
            None if body_h > 0.0 => body_el(m, store, theme, kit, scroll, body_h),
            None => w::space(0.0),
        };
        let held = m.panel.as_ref().filter(|_| m.on_panel());
        self.body.halo_owned = false;
        self.body.fill_owned = held.is_none();
        self.body.cursor = held.and_then(|p| p.cursor.shown()).map(str::to_owned);
        self.body.ring = held.is_some_and(|p| p.cursor.keyed);
        let origin = (body_x, body_top - scroll);
        self.panel_origin = origin.1;
        let d = self.body.draw(&body, Rect::new(origin.0, origin.1, origin.0 + body_w, origin.1 + m.layout.content_h), Some(body_clip), alpha, theme, kit, scene, now);
        if d.animating {
            self.wake = Some(now);
        }

        // The cursor's fill travels between cells on `spatialFast` (DESIGN.md
        // §1 "Motion"), keyboard and pointer alike; a wrap, a filter and a
        // level change land at once.
        let id = m.rows.get(m.cursor).map(|r| r.id.as_str());
        let stop = id.and_then(|id| self.body.stops.iter().find(|st| st.key.split_once(':').is_some_and(|(_, k)| k == id))).cloned();
        let mut p = Painter::new(scene, &mut self.sel_nodes, Some(body_clip)).after(Some(self.sel_anchor));
        match stop {
            Some(st) => {
                let on = (m.level.clone(), m.query.text().to_owned());
                let travels = self.sel_on.as_ref() == Some(&on) && (m.travels || !m.from_keys);
                let local = [st.rect.x as f64 - origin.0, st.rect.y as f64 - origin.1, st.rect.w as f64, st.rect.h as f64];
                let ms = Clock::SpatialFast.ms(theme) * self.scale;
                for (a, t) in self.sel.iter_mut().zip(local) {
                    if travels { a.set_on(now, t, ms, Clock::SpatialFast.curve()) } else { a.jump(t) }
                }
                self.sel_on = Some(on);
                let [x, y, w_, h] = [0, 1, 2, 3].map(|i| self.sel[i].value(now));
                let r = ui::irect(Rect::new(origin.0 + x, origin.1 + y, origin.0 + x + w_, origin.1 + y + h));
                let b = if matches!(m.view, View::Rows | View::AppGrid | View::Emoji) {
                    theme.box_style(Role::LauncherRow, Some("selected"))
                } else {
                    let mut b = theme.box_style(Role::Cell, Some("selected"));
                    b.border = None;
                    b.wash = None;
                    theme.with_cursor(b, m.from_keys, true)
                };
                crate::ui::boxes::paint(&mut p, r, &b, st.radius, alpha, 0.0);
                if self.sel.iter().any(|a| a.running(now)) {
                    self.wake = Some(now);
                }
            }
            None => self.sel_on = None,
        }
        p.finish();
        self.feed = (m.view == View::Mirror).then(|| {
            let (w, h) = m.mirror_box(theme);
            let feed = IRect::new((fx + pad + s.sm).round() as i32, (body_top + s.md).round() as i32, w as i32, h as i32);
            let pic = store.mirror.frame.as_ref().filter(|(id, _)| *id == m.mirror_current).map(|(_, b)| {
                let (bw, bh) = b.size();
                IRect::new(feed.x + (feed.w - bw) / 2, feed.y + (feed.h - bh) / 2, bw, bh)
            });
            let out = |r: IRect| IRect::new(r.x + on_output.0, r.y + on_output.1, r.w, r.h);
            (out(feed), pic.map(out))
        });
        // The actions column over the right half, else the selected
        // subject's preview there.
        self.column_rect = None;
        self.pane_rect = None;
        if let Some(c) = m.column.as_ref().filter(|_| body_h > 0.0) {
            let half = (body_w * m.list_share(theme)).round();
            let r = Rect::new(body_x + half, body_top, body_x + body_w, body_top + body_h);
            let scroll = self.column_scroll.value(now) + c.touch.value(now);
            let (el, settling) = column_el(m, theme, scroll, now, self.scale);
            let clip = ui::irect(r).intersect(&view_clip);
            let d = self.preview.draw(&el, Rect::new(r.x0, r.y0 - scroll, r.x1, r.y1 - scroll), Some(clip), alpha, theme, kit, scene, now);
            if d.animating || settling {
                self.wake = Some(now);
            }
            self.column_rect = Some(r);
            self.pane_since = None;
        } else if let Some(row) = m.pane().filter(|_| body_h > 0.0) {
            let (dx, pw, ph) = m.pane_extent(theme);
            let r = Rect::new(body_x + dx, body_top + s.lg, body_x + dx + pw, body_top + s.lg + ph);
            let el = preview_el(m, store, row, theme, kit, r.width(), r.height());
            // The pane's arrival fades its content in on `effects` while the
            // card widens, as the actions column's does.
            let since = *self.pane_since.get_or_insert(now);
            let ms = Clock::Effects.ms(theme) * self.scale;
            let t = crate::motion::progress(since, ms, now);
            let el = if t < 1.0 { el.fade(Clock::Effects.curve().ease(t) as f32) } else { el };
            let d = self.preview.draw(&el, r, Some(view_clip), alpha, theme, kit, scene, now);
            if d.animating || t < 1.0 {
                self.wake = Some(now);
            }
            self.pane_rect = Some(r);
        } else {
            self.pane_since = None;
            self.preview.draw(&w::space(0.0), Rect::new(0.0, 0.0, 0.0, 0.0), Some(body_clip), alpha, theme, kit, scene, now);
        }

        // The foot band: the selected subject's keys under the list, on the
        // rows' inset, its rule as short as the field's.
        let foot_rule = (body_top + body_h).round() as i32;
        let foot_top = if body_h > 0.0 { foot_rule as f64 + theme.border_width } else { body_top };
        let foot = if m.footless() { w::space(0.0) } else { footer(m, store, theme) };
        let d = self.foot.draw(&foot, Rect::new(fx + s.lg, foot_top, fx + fw - s.lg, foot_top + s.control_height), Some(clip), alpha, theme, kit, scene, now);
        if d.animating {
            self.wake = Some(now);
        }

        let mut p = Painter::new(scene, &mut self.rules, Some(clip));
        let inset = field_x.round() as i32;
        p.rect(IRect::new(frame.x + inset, rule_top, (frame.w - inset * 2).max(0), bw), sep, 0.0);
        if body_h > 0.0 && !m.footless() {
            p.rect(IRect::new(frame.x + inset, foot_rule, (frame.w - inset * 2).max(0), bw), sep, 0.0);
        }
        // Pond's divider between the list and the side column, `lg` short
        // of the body's ends.
        if let Some(r) = self.column_rect.or(self.pane_rect.map(|_| {
            let half = (body_w * m.list_share(theme)).round();
            Rect::new(body_x + half, body_top, body_x + body_w, body_top + body_h)
        })) {
            let top = (body_top + s.lg).round() as i32;
            p.rect(IRect::new(r.x0.round() as i32, top, bw, ((body_h - s.lg * 2.0).round() as i32).max(0)), sep, 0.0);
        }
        p.finish();
    }

    /// What is under the pointer: a body row or cell, a header or footer
    /// control.
    pub fn hit(&self, x: f64, y: f64) -> Option<crate::ui::Hit> {
        let (x, y) = self.local(x, y);
        self.body.hit(x, y).or_else(|| self.preview.hit(x, y)).or_else(|| self.head.hit(x, y)).or_else(|| self.foot.hit(x, y)).cloned()
    }

    /// What a click of `button` at an output point means to the held
    /// panel's body.
    pub fn panel_event(&self, x: f64, y: f64, button: crate::ui::Button) -> Option<crate::ui::Event> {
        let (x, y) = self.local(x, y);
        Ui::event(self.body.hit(x, y)?, x, button)
    }

    /// An output point in the surface's own pixels.
    fn local(&self, x: f64, y: f64) -> (f64, f64) {
        match &self.modal.layer {
            Some(l) => {
                let at = l.place(&self.modal.win.card, !self.modal.win.animating(Instant::now()));
                (x - f64::from(at.x), y - f64::from(at.y))
            }
            None => (x, y),
        }
    }

    /// A box on the surface, settled, as a box on the output.
    fn on_output(&self, r: IRect) -> IRect {
        let at = self.modal.layer.as_ref().map_or((0, 0), |l| {
            let p = l.place(&self.modal.win.card, true);
            (p.x, p.y)
        });
        IRect::new(r.x + at.0, r.y + at.1, r.w, r.h)
    }

    /// Whether an output point is over the actions column, which a wheel
    /// there scrolls instead of the list.
    pub fn over_column(&self, x: f64, y: f64) -> bool {
        let (x, y) = self.local(x, y);
        self.column_rect.is_some_and(|r| r.contains((x, y)))
    }

    /// The mode token's box on the output, while one is drawn.
    pub fn token_rect(&self) -> Option<IRect> {
        let r = self.head.hits.iter().find(|h| h.on.as_deref() == Some("token"))?.rect;
        Some(self.on_output(r))
    }

    /// The field band on the output.
    pub fn field_box(&self) -> Option<IRect> {
        self.field_rect.map(|r| self.on_output(ui::irect(r)))
    }

    /// The actions column's box on the output, while it is drawn.
    pub fn column_box(&self) -> Option<IRect> {
        self.column_rect.map(|r| self.on_output(ui::irect(r)))
    }

    /// The preview pane's box on the output, while it is drawn.
    pub fn pane_box(&self) -> Option<IRect> {
        self.pane_rect.map(|r| self.on_output(ui::irect(r)))
    }

    /// The card's settled box on the output.
    pub fn card_box(&self) -> IRect {
        match &self.modal.layer {
            Some(l) => l.place(&self.modal.win.card, true),
            None => self.modal.win.card.live_rect(),
        }
    }

    /// Every list row drawn, by index, its box on the output cut to what
    /// shows.
    pub fn row_boxes(&self) -> Vec<(usize, IRect)> {
        self.body
            .hits
            .iter()
            .filter_map(|h| Some((h.on.as_deref()?.strip_prefix("row:")?.parse().ok()?, self.on_output(h.rect))))
            .collect()
    }

    /// The foot's buttons on the output, each by what a click on it does.
    pub fn foot_hints(&self) -> Vec<(String, IRect)> {
        self.foot.hits.iter().filter_map(|h| Some((h.on.clone()?, self.on_output(h.rect)))).collect()
    }

    /// What the pointer hovers in the list and in the column: the row's
    /// index and the action's position, as they were last drawn.
    pub fn hovered(&self) -> (Option<usize>, Option<usize>) {
        let on = |ui: &Ui, prefix: &str| {
            let path = ui.hover.as_deref()?;
            ui.hits.iter().find(|h| h.path == path)?.on.as_deref()?.strip_prefix(prefix)?.parse().ok()
        };
        (on(&self.body, "row:"), on(&self.preview, "action:"))
    }

    pub fn set_hover(&mut self, path: Option<String>) -> bool {
        let changed = self.body.hover != path;
        self.body.hover = path.clone();
        self.preview.hover = path.clone();
        self.head.hover = path.clone();
        self.foot.hover = path;
        changed
    }
}

fn header(m: &Model, store: &Store, theme: &Theme, raster: f64) -> El {
    let s = &theme.space;
    let mut parts = Vec::new();
    let token = m.token(store);
    if !token.is_empty() {
        parts.push(w::token(s, &token).on("token"));
        parts.push(w::space(s.sm));
    }
    for word in m.column_tokens() {
        parts.push(w::token(s, &[word]).on("column-token"));
        parts.push(w::space(s.sm));
    }
    // A held panel takes the keys the field would: its page's title stands
    // where the query would be.
    if let Some(p) = m.panel.as_ref().filter(|_| m.on_panel()) {
        let v = crate::surfaces::panel::View { store, theme, output: (0.0, 0.0), cursor: None, hovered: None, now: Instant::now(), motion_scale: 1.0, raster };
        let title = p.module.title(&v);
        if token.last() != Some(&title) {
            parts.push(crate::surfaces::panel::title(title));
        }
        parts.push(w::spacer());
        return w::row(0.0, parts).fill();
    }
    parts.push(w::edit(&m.query, &m.placeholder(store), Type::Field));
    parts.push(w::space(s.lg));
    parts.push(w::icon("search").size(Type::Heading).ink(Ink::Dim));
    w::row(0.0, parts).fill()
}

/// The selected subject's keys and nothing else, right-aligned, each a
/// button doing what its key does.
fn footer(m: &Model, store: &Store, theme: &Theme) -> El {
    let s = &theme.space;
    let node = m.rows.get(m.cursor);
    let mut hints: Vec<(String, Vec<&'static str>, &'static str)> = Vec::new();
    match m.mode {
        Mode::Menu if m.column.is_some() => {
            let label = if m.column_armed().is_some() { "Confirm" } else { "Run" };
            hints.push((label.into(), vec!["Enter"], "column-run"));
            hints.push(("Back".into(), vec!["Shift", "Tab"], "column-back"));
        }
        Mode::Input => hints.push(("Submit".into(), vec!["Enter"], "primary")),
        Mode::Select => {
            if node.is_some() {
                hints.push(("Select".into(), vec!["Enter"], "primary"));
            }
        }
        Mode::Menu => {
            if let Some(n) = node {
                let subject = m.subject(store, n);
                let bar = fs_menu::actions::action_bar(&fs_menu::actions::ActionCtx {
                    mode: m.mode,
                    node,
                    at_root: m.level.is_none(),
                    picker_select: m.picker.select,
                    confirming: m.confirming(&n.id),
                    ..Default::default()
                });
                // A Style preview runs `theme mode`, which a pinned key refuses.
                let mode_tile = m.view == View::Picker && n.kind == NodeKind::WallpaperMode;
                if mode_tile {
                    let key = crate::services::theme::mode_key(store.config.settings());
                    if key != "light" && key != "dark" {
                        hints.push((format!("Set {} mode", n.label.to_lowercase()), vec!["Enter"], "primary"));
                    }
                } else if let Some(p) = bar.primary {
                    hints.push((p.label, vec!["Enter"], "primary"));
                }
                if !subject.inert() {
                    let all = fs_menu::actions::actions_for(&subject);
                    if let Some(second) = all.get(1).filter(|_| !m.confirming(&n.id)) {
                        hints.push((second.label.clone(), vec!["Shift", "Enter"], "alternate"));
                    }
                }
                if m.view == View::Picker && m.picker_sections.len() > 1 {
                    hints.push(("Next section".into(), vec!["Ctrl", "Tab"], "section"));
                }
                if m.view != View::Mirror && !mode_tile {
                    hints.push(("Actions".into(), vec!["Tab"], "actions"));
                }
                if !subject.inert() && m.view == View::Rows && fs_menu::actions::into_action(&subject).is_some() {
                    hints.push(("Into".into(), vec!["Right"], "into"));
                }
            }
        }
    }
    let mut parts = vec![w::spacer()];
    for (label, keys, on) in hints {
        // The mockup's keys: ↵, ⇥ and ⇧ as glyphs rather than words.
        let keys: Vec<&str> = keys
            .iter()
            .map(|k| match *k {
                "Enter" => "enter-glyph",
                "Tab" => "tab-glyph",
                "Shift" => "shift-glyph",
                other => other,
            })
            .collect();
        // Quieter than the rows: Pond's card has nothing at its foot.
        let content = w::row(s.sm, vec![w::text(label).size(Type::Caption).ink(Ink::Dim), w::chord_keys(s, &keys).fade(0.6)]);
        parts.push(w::cell(content).ghost().interactive().cell_state(|c| c.chip = true).on(on));
    }
    // A band a `controlHeight` tall whether or not a hint is in it.
    parts.push(El::new(ui::el::Kind::Swatch { color: fs_theme::color::Rgba::TRANSPARENT, w: 0.0, h: s.control_height, radius: 0.0, border: false }));
    w::row(s.xs, parts).fill()
}

/// The actions column, or the targets one of them asks for: rows of the
/// list's height and inset, the cursor's row filled, faded in on `effects`
/// as it opens, built only where `scroll` puts the viewport. True while
/// that fade runs.
fn column_el(m: &Model, theme: &Theme, scroll: f64, now: Instant, scale: f64) -> (El, bool) {
    let s = &theme.space;
    let Some(c) = &m.column else { return (w::space(0.0), false) };
    let row_h = c.row_h.max(1.0);
    let icon_px = launcher_px(fs_theme::tokens::LAUNCHER.row_icon);
    let armed = m.column_armed();
    let first = (scroll / row_h).floor().max(0.0) as usize;
    let fits = (c.view_h / row_h).ceil() as usize + 1;
    let last = (first + fits).min(c.shown.len());
    let more = c.more(scroll);
    let ms = Clock::Effects.ms(theme) * scale;
    let progress = |since: Instant| crate::motion::progress(since, ms, now);
    let joined = c.joined.map(|(from, at)| (from, progress(at)));
    let mut rows = vec![w::space(first.min(last) as f64 * row_h)];
    let mut drawn_to = last;
    for (at, &i) in c.shown.iter().enumerate().skip(first).take(fits) {
        if let Some((_, n)) = more.filter(|(row, _)| *row == at) {
            rows.push(w::cell(row_words(format!("{n} more")).ink(Ink::Dim)).ghost().height(row_h));
            drawn_to = at + 1;
            break;
        }
        let selected = at == c.cursor;
        let (label, icon, trail, dim, destructive) = match &c.target {
            Some((_, targets)) => {
                let t = &targets[i];
                let meta = t.meta.clone().map(|v| row_words(v).ink(Ink::Dim));
                (t.label.clone(), None, meta, false, false)
            }
            None => {
                let a = &c.actions[i];
                // A destructive label already names what it does (`Uninstall Firefox?`).
                let label = match armed == Some(i) {
                    true if a.destructive => format!("{}?", a.label),
                    true => crate::ui::confirm::ask(&a.label),
                    false => a.label.clone(),
                };
                let trail = matches!(a.run, Run::Into(_)).then(|| w::icon("chevron-right").ink(Ink::Dim));
                (label, a.icon.clone(), trail, a.run == Run::Note, a.destructive)
            }
        };
        let mut lead = Vec::new();
        if let Some(name) = icon {
            let ink = if destructive { Ink::Destructive } else { Ink::Fg };
            lead.push(w::icon(name).ink(ink).width(Size::Px(icon_px)).height(icon_px));
        }
        let mut name = row_words(label);
        if dim {
            name = name.ink(Ink::Dim);
        }
        lead.push(name);
        let mut parts = vec![w::row(s.lg, lead).fill()];
        parts.extend(trail);
        let confirming = armed == Some(i) && c.target.is_none();
        // A plugin's actions fade in under the built-ins as they join.
        let fade = joined.filter(|(from, t)| c.target.is_none() && i >= *from && *t < 1.0).map(|(_, t)| Clock::Effects.curve().ease(t) as f32);
        let row = w::cell(w::row(s.lg, parts).fill())
                .ghost()
                .selected(selected)
                .interactive()
                .cell_state(|st| {
                    st.row = true;
                    st.destructive = confirming;
                })
                .on(format!("action:{at}"))
                .key(format!("a{at}"))
                .height(row_h);
        rows.push(match fade {
            Some(f) => row.fade(f),
            None => row,
        });
    }
    if c.shown.is_empty() {
        rows.push(w::text("No matches").ink(Ink::Muted).pad(s.lg, s.md, s.lg, s.md));
    }
    rows.push(w::space(c.shown.len().saturating_sub(drawn_to) as f64 * row_h));
    let t = progress(c.opened);
    let settling = t < 1.0 || joined.is_some_and(|(_, j)| j < 1.0);
    let el = w::column(0.0, rows).pad(s.sm, s.sm, s.sm, s.sm);
    (if t < 1.0 { el.fade(Clock::Effects.curve().ease(t) as f32) } else { el }, settling)
}

/// A level with no rows: its own note, else what the query found nothing
/// for.
fn empty_words(m: &Model, s: &fs_theme::tokens::Space) -> El {
    let title = match &m.empty {
        Some(n) => n.label.clone(),
        None if m.mode == Mode::Input => String::new(),
        None if m.view == View::Picker && !m.query.is_empty() => "No wallpapers match".into(),
        None if !m.query.is_empty() => format!("No results for \u{201C}{}\u{201D}", m.query.text()),
        None => "No results".into(),
    };
    let mut col = vec![w::text(title).ink(Ink::Muted).centred()];
    if let Some(d) = m.empty.as_ref().and_then(|n| n.desc.clone()).filter(|d| !d.is_empty()) {
        col.push(w::caption(d).mono().centred());
    }
    w::column(s.row_gap, col)
}

/// The body's widgets for the slots inside the viewport, the rest as room.
fn body_el(m: &Model, store: &Store, theme: &Theme, kit: &mut Kit, scroll: f64, view_h: f64) -> El {
    let s = &theme.space;
    let lay = &m.layout;
    if m.view == View::Mirror {
        return mirror_body(m, store, theme);
    }
    if m.rows.is_empty() {
        // A card hugging its content was given a control height either side
        // of the words; a fixed one sets them a little above its middle.
        let above = if m.content_tall() { s.control_height } else { (view_h / 2.0 - s.control_height).max(0.0) };
        return w::column(0.0, vec![w::space(above), empty_words(m, s)]);
    }
    let lo = scroll - view_h * 0.5;
    let hi = scroll + view_h * 1.5;
    let visible = |y: f64, h: f64| y + h >= lo && y <= hi;
    let snap = snapshot(store);
    let mut items: Vec<(f64, f64, El)> = Vec::new();
    for (text, y, x, far) in &lay.headings {
        let h = ui::measure(&heading_el(s, text), 400.0, theme, kit).1;
        if !visible(*y, h) {
            continue;
        }
        let heading = heading_el(s, text).pad_start(s.control_padding_x + x);
        if far.is_empty() {
            items.push((*y, h, heading));
        } else {
            // The pictures' own width, so the far words end on the last
            // column's edge.
            let columns = lay.columns.max(1) as f64;
            let grid = columns * lay.cell_w + (columns - 1.0) * s.lg + s.xs;
            let far = w::caption(far.clone()).elide().hug();
            items.push((*y, h, w::row(s.sm, vec![heading.hug(), w::spacer(), far]).width(Size::Px(grid))));
        }
    }
    for (y, line, caption) in &lay.notes {
        let note = w::column(s.row_gap, vec![w::text(line.clone()).ink(Ink::Muted), w::caption(caption.clone())]).pad_start(s.xs);
        items.push((*y, ui::measure(&note, 400.0, theme, kit).1, note));
    }
    if m.monitor_strip_h > 0.0 {
        // The list's half while the actions column holds the other.
        let width = if lay.halved { (lay.body_w * m.list_share(theme)).round() } else { lay.body_w };
        let strip = monitor_strip(store, theme, &m.monitor_history).width(Size::Px(width));
        items.push((lay.slots.first().map_or(s.md, |s0| s0.y) - m.monitor_strip_h - s.section_gap, m.monitor_strip_h, strip));
    }
    let cells = m.cells();
    let tiles = if m.view == View::Picker { m.picker_tiles(store) } else { Vec::new() };
    for (i, slot) in lay.slots.iter().enumerate() {
        if !visible(slot.y, slot.h) {
            continue;
        }
        let row = &m.rows[i];
        let selected = i == m.cursor;
        let el = if i < cells && m.view == View::AppGrid {
            let gutter = s.sm;
            let image = store.menu.icons.get(&row.icon_source).cloned().flatten();
            let pic = if image.is_some() {
                w::picture(image, s.control_height * 2.0)
            } else {
                El::new(ui::el::Kind::Icon { name: "layout-grid".into(), size: Type::DisplayLarge, ink: Ink::Dim })
                    .width(Size::Px(s.control_height * 2.0))
                    .height(s.control_height * 2.0)
            };
            let content = w::column(s.row_gap, vec![pic.centred(), w::label(row.label.clone()).elide().hug().centred()]);
            w::cell(content)
                .ghost()
                .selected(selected)
                .interactive()
                .cell_state(|st| st.row = true)
                .on(format!("row:{i}"))
                .stop(format!("c:{}", row.id))
                .width(Size::Px(slot.w))
                .pad(gutter, gutter, gutter, gutter)
        } else if m.view == View::Monitor {
            proc_row(m, store, row, i, selected, theme).width(Size::Px(slot.w))
        } else if i < cells && m.view == View::Picker {
            picker_tile(m, store, theme, &tiles, row, i, selected, slot)
        } else if i < cells {
            let glyph = w::text(row.icon.clone()).size(Type::Display).centred();
            w::cell(w::column(0.0, vec![glyph]))
                .ghost()
                .selected(selected)
                .interactive()
                .cell_state(|st| st.row = true)
                .on(format!("row:{i}"))
                .stop(format!("e:{}", row.id))
                .width(Size::Px(slot.w))
                .height(slot.h)
        } else {
            let checked = toggles::checked_for(Some(row), Some(&snap), Some(&store.menu.checked));
            menu_row(m, store, row, i, selected, checked, theme).width(Size::Px(slot.w)).height(slot.h - slot.band)
        };
        items.push((slot.y + slot.band, slot.h - slot.band, el.pad_start(0.0)));
    }
    // Absolutely placed by stacking: a column of rows, each a row of what
    // shares its top.
    items.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut col: Vec<El> = Vec::new();
    let mut y = 0.0;
    let i = 0;
    while i < items.len() {
        let top = items[i].0;
        let mut line = Vec::new();
        let mut h: f64 = 0.0;
        let mut x = 0.0;
        while i < items.len() && (items[i].0 - top).abs() < 0.5 {
            let (_, ih, el) = items.remove(i);
            let slot_x = slot_x_of(m, &el);
            if slot_x > x {
                line.push(w::space(slot_x - x));
                x = slot_x;
            }
            x += match el.width { Size::Px(px) => px, _ => 0.0 };
            line.push(el);
            h = h.max(ih);
        }
        if top > y {
            col.push(w::space(top - y));
        }
        // Exactly the slot's height: the next line's gap is reckoned from
        // the layout, so a line shorter than its slot would pull every line
        // under it up and leave the viewport's foot empty.
        // Keyed by the line's top in the content, so a scroll that culls
        // the lines above it leaves its rows' hover and tweens on them.
        col.push(w::row(0.0, line).top().height(h).key(format!("y{}", top.round() as i64)));
        y = top + h;
    }
    col.push(w::space((lay.content_h - y).max(0.0)));
    w::column(0.0, col)
}
