// Portions from Pond (BSD-3-Clause, Copyright 2026 Butterfly Interaction Inc.)
//! Pond's Wi-Fi card: the title with the radio's switch, a hairline, then
//! one row per device and network in range (wired, then connected, known
//! and by signal), each a signal glyph and its name. The connected row sits
//! in the soft fill with Disconnect at its end, a secured one carries a
//! lock. Saved networks out of range wait behind a closed "Known networks"
//! row, a secured network nobody knows opens its passphrase under its row,
//! and Pond's dim footer rows hold sharing, the saved password and the
//! speed test.
//!
//! The panel holds the scanner while it is open; its close ends a running
//! speed test (`network::Hold`).

use fs_devices::network::speedtest;
use fs_menu::field;

use super::{Edit, Effect, Panel, View};
use crate::services::clipboard;
use crate::services::devices::{self, network as net};
use crate::store::Topic;
use crate::ui::{El, Event, Ink, Variant, What, w};

/// The mask the passphrase field draws.
const MASK: char = '\u{25CF}';

#[derive(Clone, Copy, PartialEq, Eq)]
enum Field {
    Identity,
    Password,
}

#[derive(Default)]
pub struct Network {
    hold: Option<net::Hold>,
    /// The SSID whose prompt is open.
    prompt: Option<String>,
    identity: field::Field,
    password: field::Field,
    field: Option<Field>,
    /// The "Known networks" disclosure, closed on every open.
    known_open: bool,
}

fn row_key(ssid: &str) -> String {
    format!("wifi:{ssid}")
}

impl Network {
    fn state<'a>(fx: &'a Effect) -> &'a net::Network {
        &fx.store.devices.network
    }

    fn open_prompt(&mut self, ssid: &str, enterprise: bool) {
        if self.prompt.as_deref() != Some(ssid) {
            self.identity.clear();
            self.password.clear();
        }
        self.password.secret = true;
        self.prompt = Some(ssid.to_owned());
        self.field = Some(if enterprise { Field::Identity } else { Field::Password });
    }

    fn cancel_prompt(&mut self) {
        self.prompt = None;
        self.identity.clear();
        self.password.clear();
        self.field = None;
    }

    /// `_activateWifiRow` and WifiService.activate.
    fn activate_row(&mut self, ssid: &str, fx: &mut Effect) {
        let n = Self::state(fx);
        if n.action.is_some() || self.prompt.as_deref() == Some(ssid) {
            return;
        }
        let Some(row) = n.row(ssid).cloned() else { return };
        let ssid = ssid.to_owned();
        if row.connected {
            fx.service(move |ctx| net::disconnect(ctx, ssid));
            return;
        }
        let retype = n.failure.as_ref().is_some_and(|f| f.ssid == ssid && f.secret);
        if retype || (row.secured && !row.known) {
            self.open_prompt(&ssid, row.enterprise);
            return;
        }
        fx.service(move |ctx| net::connect(ctx, ssid, net::Secret::Saved));
    }

    /// `_submitPassword`.
    fn submit(&mut self, fx: &mut Effect) {
        let n = Self::state(fx);
        let Some(ssid) = self.prompt.clone() else { return };
        if n.action.is_some() || self.password.is_empty() {
            return;
        }
        let Some(row) = n.row(&ssid) else { return };
        let secret = if row.enterprise {
            if self.identity.is_empty() {
                self.field = Some(Field::Identity);
                return;
            }
            net::Secret::Eap { identity: self.identity.text().to_owned(), password: self.password.text().to_owned() }
        } else {
            net::Secret::Psk(self.password.text().to_owned())
        };
        fx.service(move |ctx| net::connect(ctx, ssid, secret));
    }

    fn forget(&self, ssid: &str, fx: &mut Effect) {
        let n = Self::state(fx);
        if n.action.is_some() || !n.row(ssid).is_some_and(|r| r.known && !r.connected) {
            return;
        }
        let ssid = ssid.to_owned();
        fx.service(move |ctx| net::forget(ctx, ssid));
    }

    fn wired(name: &str, fx: &mut Effect) {
        let connected = Self::state(fx).wired_rows.iter().any(|d| d.name == name && d.connected);
        let name = name.to_owned();
        fx.service(move |ctx| net::wired_toggle(ctx, name, connected));
    }

    fn speed(fx: &mut Effect) {
        if !Self::state(fx).speed.running() {
            fx.service(|ctx| {
                net::speed_start(ctx);
            });
        }
    }

    /// The prompt shows while its network has not connected.
    fn prompt_for<'a>(&self, n: &'a net::Network) -> Option<&'a net::Row> {
        let ssid = self.prompt.as_deref()?;
        n.row(ssid).filter(|r| !r.connected)
    }

    fn wifi_row(&self, v: &View, r: &net::Row) -> El {
        let s = &v.theme.space;
        let n = &v.store.devices.network;
        let key = row_key(&r.ssid);
        let mine = |ssid: &str| ssid == r.ssid;
        let status = match (&n.action, &n.failure) {
            (Some((kind, ssid)), _) if mine(ssid) => Some((
                match kind {
                    net::ActionKind::Connect => "Connecting",
                    net::ActionKind::Disconnect => "Disconnecting",
                    net::ActionKind::Forget => "Forgetting",
                }
                .to_owned(),
                false,
            )),
            (_, Some(f)) if mine(&f.ssid) => Some((f.text.clone(), true)),
            _ => None,
        };
        let failed = status.as_ref().is_some_and(|(_, f)| *f);
        let name = if r.ssid.is_empty() { w::label("Hidden network").ink(Ink::Muted) } else { w::label(r.ssid.clone()) };
        // Out of range has no bars to show, and an empty glyph would read as
        // a weak signal.
        let mut top = if r.in_range { vec![w::icon(signal_glyph(r.signal)), name.elide().fill()] } else { vec![w::icon("wifi-off").ink(Ink::Dim), name.elide().fill()] };
        // Forget reveals on the row the pointer is on.
        if r.known && !r.connected && v.hovered.as_deref() == Some(key.as_str()) && n.action.is_none() {
            top.push(w::icon("trash").ink(Ink::Dim).on(format!("forget:{}", r.ssid)).tip("Forget"));
        }
        if r.connected {
            top.push(w::button("Disconnect").variant(Variant::Ghost).enabled(n.action.is_none()).on(format!("row:{}", r.ssid)));
        } else if r.secured {
            top.push(w::icon("lock").ink(Ink::Dim));
        }
        let mut parts = vec![w::row(s.md, top).fill().height(s.control_height)];
        if let Some((text, failed)) = status {
            parts.push(w::section_label(s, &text, None, false).ink(if failed { Ink::Destructive } else { Ink::Dim }).pad_start(s.lg + s.md));
        }
        if self.prompt_for(n).is_some_and(|p| p.ssid == r.ssid) {
            let mut fields = Vec::new();
            if r.enterprise {
                fields.push(w::field(&self.identity, "Identity (user@domain)", self.field == Some(Field::Identity), None).key("identity"));
            }
            let masked: String = std::iter::repeat_n(MASK, self.password.text().chars().count()).collect();
            let error = n.failure.as_ref().filter(|f| failed && mine(&f.ssid)).map(|f| f.text.as_str());
            fields.push(w::input(&masked, "Passphrase", self.field == Some(Field::Password), error).caret(self.password.caret_chars()).key("passphrase"));
            parts.push(w::column(s.xs, fields).pad(0.0, s.xs, 0.0, s.sm));
        }
        w::cell(w::column(0.0, parts).fill()).ghost().cell_state(|c| c.open = r.connected).interactive().stop(key).on(format!("row:{}", r.ssid))
    }

    /// The disclosure row over saved networks out of range, then those rows
    /// while it is open.
    fn known(&self, v: &View, rows: &[&net::Row]) -> El {
        let chevron = if self.known_open { "chevron-down" } else { "chevron-right" };
        let mut parts = vec![action_row(v, "history", &format!("Known networks ({})", rows.len()), vec![w::icon(chevron).ink(Ink::Dim)]).stop("known").on("known")];
        if self.known_open {
            parts.extend(rows.iter().map(|r| self.wifi_row(v, r)));
        }
        w::column(0.0, parts)
    }

    /// Pond's "Add a network" rows: dim glyph and words, what each opens
    /// under it. Share, the saved password while connected, then the speed
    /// test with its last result.
    fn footer(&self, v: &View, wifi: bool) -> Vec<El> {
        let s = &v.theme.space;
        let n = &v.store.devices.network;
        let mut out = Vec::new();
        if wifi {
            let open = n.qr != net::Qr::Closed;
            out.push(action_row(v, "share-2", if open { "Hide network code" } else { "Share network" }, Vec::new()).stop("share").on("share"));
            match &n.qr {
                net::Qr::Generating => out.push(w::panel_empty(s, "Generating")),
                net::Qr::Failed(e) => out.push(w::section_label(s, e, None, true)),
                net::Qr::Shown(rows) => out.push(w::cell(w::matrix(rows.clone()))),
                net::Qr::Closed => {}
            }
        }
        if wifi && n.ssid.is_some() {
            let (label, value) = match &n.reveal {
                net::Reveal::Idle => ("Show password", None),
                net::Reveal::Shown(secret) => ("Hide password", Some(w::value(secret.clone()).ink(Ink::Fg))),
                net::Reveal::Reading => ("Hide password", Some(w::section_label(s, "Reading", None, false).ink(Ink::Dim))),
                net::Reveal::Failed(e) => ("Show password", Some(w::section_label(s, e, None, false).ink(Ink::Destructive))),
            };
            out.push(action_row(v, "key-round", label, value.into_iter().collect()).stop("password").on("password"));
        }
        let st = &n.speed;
        let rate = |live: bool, live_v: f64, result: f64| {
            if live {
                speedtest::format_mbps(live_v)
            } else if result > 0.0 {
                speedtest::format_mbps(result)
            } else {
                "--".into()
            }
        };
        let down = rate(st.phase == net::Phase::Down, st.down.live_mbps, st.down_result);
        let up = rate(st.phase == net::Phase::Up, st.up.live_mbps, st.up_result);
        let label = match st.phase {
            net::Phase::Resolving => "Resolving interface",
            net::Phase::Down => "Measuring down",
            net::Phase::Up => "Measuring up",
            _ => "Speed test",
        };
        let result = w::value(format!("\u{2193} {down}  \u{2191} {up} Mbps"));
        out.push(action_row(v, "gauge", label, vec![result]).stop("speedtest").on("speedtest"));
        if !st.error.is_empty() {
            out.push(w::panel_text(s, w::caption(st.error.clone()).ink(Ink::Destructive)));
        }
        out
    }
}

/// Bars for a 0..1 signal, Pond's three steps plus none.
fn signal_glyph(signal: f64) -> &'static str {
    match signal {
        x if x >= 0.67 => "wifi",
        x if x >= 0.34 => "wifi-high",
        x if x > 0.0 => "wifi-low",
        _ => "wifi-zero",
    }
}

/// A row that opens or runs something: its glyph and words dim until the
/// pointer is on it, whatever it carries trailing.
fn action_row(v: &View, glyph: &str, label: &str, trailing: Vec<El>) -> El {
    let s = &v.theme.space;
    let mut parts = vec![w::icon(glyph).ink(Ink::Dim), w::label(label).ink(Ink::Dim).elide().fill()];
    parts.extend(trailing);
    w::cell(w::row(s.md, parts).fill().height(s.control_height)).ghost().interactive()
}

impl Panel for Network {
    fn id(&self) -> &'static str {
        "network"
    }

    fn title(&self, _: &View) -> String {
        "Wi-Fi".into()
    }


    fn reads(&self) -> &'static [Topic] {
        &[Topic::Devices]
    }

    fn actions(&self, v: &View) -> Vec<El> {
        let n = &v.store.devices.network;
        vec![
            w::switch(n.wifi_enabled).on("radio"),
            w::icon_button("refresh-cw").enabled(n.wifi_device.is_some()).tip("Rescan").on("rescan").key("rescan"),
        ]
    }

    fn body(&self, v: &View) -> El {
        let s = &v.theme.space;
        let n = &v.store.devices.network;
        let mut rows: Vec<El> = n
            .wired_rows
            .iter()
            .map(|d| {
                let mut r = vec![w::icon("ethernet-port"), w::label(d.name.clone()).elide().fill()];
                r.push(w::button(if d.connected { "Disconnect" } else { "Connect" }).variant(Variant::Ghost).on(format!("wired:{}", d.name)));
                w::cell(w::row(s.md, r).fill().height(s.control_height))
                    .ghost()
                    .cell_state(|c| c.open = d.connected)
                    .interactive()
                    .stop(format!("wired:{}", d.name))
                    .on(format!("wired:{}", d.name))
            })
            .collect();
        if n.wired_rows.is_empty() && n.wifi_device.is_none() {
            rows.push(w::panel_empty(s, "No devices"));
        }
        let mut away = Vec::new();
        if n.wifi_device.is_some() {
            let (near, out): (Vec<&net::Row>, Vec<&net::Row>) = n.rows.iter().partition(|r| r.in_range);
            away = out;
            if near.is_empty() {
                rows.push(w::panel_empty(s, if n.wifi_enabled { "Scanning" } else { "Wi-Fi is off" }));
            }
            rows.extend(near.iter().map(|r| self.wifi_row(v, r)));
        }
        let mut parts = vec![w::column(0.0, rows)];
        let mut more = Vec::new();
        if !away.is_empty() {
            more.push(self.known(v, &away));
        }
        more.extend(self.footer(v, n.wifi_device.is_some()));
        parts.push(w::column(0.0, more));
        w::column(s.sm, parts)
    }

    fn opened(&mut self) {
        self.hold = Some(net::Hold::new());
    }

    fn closed(&mut self) {
        self.hold = None;
        self.known_open = false;
        self.cancel_prompt();
    }

    fn editing(&self) -> bool {
        self.prompt.is_some()
    }

    fn call(&mut self, verb: &str, _arg: &str) -> Option<String> {
        (verb == "knownOpen").then(|| self.known_open.to_string())
    }

    fn edit(&mut self, e: Edit, fx: &mut Effect) {
        let enterprise = self.prompt.as_deref().and_then(|p| Self::state(fx).row(p)).is_some_and(|r| r.enterprise);
        if self.prompt_for(Self::state(fx)).is_none() {
            self.cancel_prompt();
            return;
        }
        let field = self.field.unwrap_or(Field::Password);
        let text = if field == Field::Identity { &mut self.identity } else { &mut self.password };
        match e {
            Edit::Key(k, mods, t) => match text.key(k, mods, t.as_deref()) {
                field::Outcome::Copy(t) | field::Outcome::Cut(t) => clipboard::command(clipboard::Cmd::SetText(t)),
                field::Outcome::Paste => clipboard::command(clipboard::Cmd::Paste),
                _ => {}
            },
            Edit::Paste(t) => {
                text.paste(&t);
            }
            Edit::Tab if enterprise => {
                self.field = Some(if field == Field::Identity { Field::Password } else { Field::Identity });
            }
            Edit::Tab => {}
            Edit::Submit if field == Field::Identity => self.field = Some(Field::Password),
            Edit::Submit => self.submit(fx),
            Edit::Cancel => self.cancel_prompt(),
        }
    }

    fn event(&mut self, ev: &Event, fx: &mut Effect) {
        match (ev.on.as_str(), &ev.what) {
            ("radio", What::Toggle(on)) => {
                let on = *on;
                fx.service(move |ctx| devices::run(ctx, devices::Op::Wifi(on)));
            }
            ("rescan", What::Click) => fx.service(|ctx| devices::run(ctx, devices::Op::Rescan)),
            ("speedtest", What::Click) => Self::speed(fx),
            ("share", What::Click) => fx.service(net::qr_toggle),
            ("password", What::Click) => fx.service(net::reveal_toggle),
            ("known", What::Click) => self.known_open = !self.known_open,
            (on, What::Click) => {
                if let Some(ssid) = on.strip_prefix("row:") {
                    self.activate_row(ssid, fx);
                } else if let Some(ssid) = on.strip_prefix("forget:") {
                    self.forget(ssid, fx);
                } else if let Some(name) = on.strip_prefix("wired:") {
                    Self::wired(name, fx);
                }
            }
            _ => {}
        }
    }

    fn activate(&mut self, stop: &str, fx: &mut Effect) {
        if stop == "speedtest" {
            Self::speed(fx);
        } else if stop == "share" {
            fx.service(net::qr_toggle);
        } else if stop == "password" {
            fx.service(net::reveal_toggle);
        } else if stop == "known" {
            self.known_open = !self.known_open;
        } else if let Some(ssid) = stop.strip_prefix("wifi:") {
            self.activate_row(ssid, fx);
        } else if let Some(name) = stop.strip_prefix("wired:") {
            Self::wired(name, fx);
        }
    }

    fn delete(&mut self, stop: &str, fx: &mut Effect) {
        if let Some(ssid) = stop.strip_prefix("wifi:") {
            self.forget(ssid, fx);
        }
    }
}
