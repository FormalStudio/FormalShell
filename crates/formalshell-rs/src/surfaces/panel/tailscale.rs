//! This machine's tailnet name and the backend's
//! connection state as the hero, this machine's own address, then the peers
//! with a reachability dot. The header's switch runs `tailscale up` or
//! `down`, and a permission failure reads NOT OPERATOR inline rather than
//! pretending the toggle worked. One cursor spans the hero, the address row
//! and each peer: Enter toggles on the hero and copies an address on any
//! other row. NEEDS LOGIN and NO TAILSCALE carry no cursor.

use fs_devices::tailscale as model;

use super::{Effect, Panel, View};
use crate::services::info::tailscale::{Action, Poll};
use crate::services::info::{kick, tailscale};
use crate::services::wants::{Source, Want};
use crate::store::Topic;
use crate::ui::{El, Event, Ink, Weight, w};

pub struct Tailscale {
    _want: Want,
}

impl Tailscale {
    pub fn new() -> Self {
        Self { _want: Want::new(Source::Tailscale) }
    }
}

fn toggle(fx: &Effect) {
    let t = &fx.store.info.tailscale;
    if t.poll != Poll::Ok || t.action != Action::None {
        return;
    }
    let running = t.status.as_ref().is_some_and(|s| s.running);
    fx.service(move |ctx| tailscale::toggle(ctx, running));
}

fn copy(fx: &Effect, ip: Option<String>) {
    if let Some(ip) = ip.filter(|ip| !ip.is_empty()) {
        fx.service(move |ctx| tailscale::copy(ctx, ip));
    }
}

fn self_ip(store: &crate::store::Store) -> Option<String> {
    let t = &store.info.tailscale;
    (t.poll == Poll::Ok).then(|| t.status.as_ref().and_then(model::self_ip)).flatten()
}

impl Panel for Tailscale {
    fn id(&self) -> &'static str {
        "tailscale"
    }

    fn title(&self, _: &View) -> String {
        "Tailscale".into()
    }

    fn icon(&self, _: &View) -> String {
        "network".into()
    }

    fn reads(&self) -> &'static [Topic] {
        &[Topic::Info]
    }

    fn actions(&self, v: &View) -> Vec<El> {
        let t = &v.store.info.tailscale;
        let on = t.poll == Poll::Ok && t.status.as_ref().is_some_and(|s| s.running);
        vec![w::switch(on).on("toggle"), w::icon_button("refresh-cw").tip("Refresh").on("refresh")]
    }

    fn opened(&mut self) {
        kick(Source::Tailscale);
    }

    fn body(&self, v: &View) -> El {
        let s = &v.theme.space;
        let t = &v.store.info.tailscale;
        let label = |text: &str| w::section_label(s, text, None, true);
        let mut parts = Vec::new();
        match t.poll {
            Poll::Unknown => parts.push(label("Loading")),
            Poll::Missing | Poll::Error => parts.push(label("No Tailscale")),
            Poll::NeedsLogin => parts.push(label("Needs login")),
            Poll::Ok => {}
        }
        let Some(status) = t.status.as_ref().filter(|_| t.poll == Poll::Ok) else { return w::column(s.section_gap, parts) };

        let meta = match t.action {
            Action::Up => "Connecting",
            Action::Down => "Disconnecting",
            Action::None if status.running => "Connected",
            Action::None => "Stopped",
        };
        parts.push(
            w::hero(
                s,
                w::Hero {
                    glyph: "network".into(),
                    title: status.self_name.clone().unwrap_or_else(|| "Unknown".into()),
                    meta: meta.into(),
                    readout: String::new(),
                    trailing: None,
                    rail: None,
                    rail_on: None,
                },
            )
            .interactive()
            .stop("status"),
        );
        if !t.error.is_empty() {
            parts.push(
                w::cell(w::section_label(s, t.error, None, false).ink(Ink::Destructive)).cell_state(|c| c.destructive = true),
            );
        }
        if let Some(ip) = model::self_ip(status) {
            let row = w::row(s.icon_gap, vec![w::section_label(s, "IP", None, false), w::spacer(), w::text(ip.clone()).mono().weight(Weight::Medium)]).fill();
            parts.push(w::cell(row).interactive().stop("self").on(format!("copy:{ip}")));
        }

        let primary = v.theme.colors.get("primary");
        let muted = v.theme.colors.get("mutedForeground");
        let mut peers = vec![w::heading(s, "Peers", Some(status.peers.len()), true)];
        if status.peers.is_empty() {
            peers.push(label("None"));
        }
        let rows = status
            .peers
            .iter()
            .enumerate()
            .map(|(i, p)| {
                let mut detail = Vec::new();
                if let Some(os) = &p.os {
                    detail.push(w::caption(os.clone()).ink(Ink::Dim));
                }
                if let Some(ip) = &p.ip {
                    detail.push(w::caption(ip.clone()).mono().ink(Ink::Dim));
                }
                let words = w::column(
                    s.xxs,
                    vec![w::text(p.name.clone()).mono().weight(Weight::Medium).elide(), w::row(s.icon_gap, detail)],
                )
                .fill();
                let row = w::row(s.icon_gap, vec![w::dot(if p.online { primary } else { muted }, s.sm).pulse(t.action == Action::Up), words]).fill();
                let cell = w::cell(row).ghost().stop(format!("peer{i}"));
                match &p.ip {
                    Some(ip) => cell.interactive().on(format!("copy:{ip}")),
                    None => cell,
                }
            })
            .collect();
        peers.push(w::column(0.0, rows));
        parts.push(w::column(s.row_gap, peers));
        w::column(s.section_gap, parts)
    }

    fn event(&mut self, ev: &Event, fx: &mut Effect) {
        match ev.on.as_str() {
            "toggle" => toggle(fx),
            "refresh" => kick(Source::Tailscale),
            on => {
                if let Some(ip) = on.strip_prefix("copy:") {
                    copy(fx, Some(ip.to_owned()));
                }
            }
        }
    }

    fn activate(&mut self, stop: &str, fx: &mut Effect) {
        match stop {
            "status" => toggle(fx),
            "self" => copy(fx, self_ip(fx.store)),
            _ => {
                let peer = stop.strip_prefix("peer").and_then(|i| i.parse::<usize>().ok());
                let ip = peer.and_then(|i| fx.store.info.tailscale.status.as_ref()?.peers.get(i)?.ip.clone());
                copy(fx, ip);
            }
        }
    }
}
