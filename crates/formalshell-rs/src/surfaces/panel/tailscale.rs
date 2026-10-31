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
use crate::ui::{El, Event, Ink, w};

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
        let label = |text: &str| w::panel_empty(s, text);
        let mut parts = Vec::new();
        match t.poll {
            Poll::Unknown => parts.push(label("Loading")),
            Poll::Missing | Poll::Error => parts.push(label("No Tailscale")),
            Poll::NeedsLogin => parts.push(label("Needs login")),
            Poll::Ok => {}
        }
        let Some(status) = t.status.as_ref().filter(|_| t.poll == Poll::Ok) else { return w::column(0.0, parts) };

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
            .stop("status"),
        );
        if !t.error.is_empty() {
            parts.push(w::panel_text(s, w::caption(t.error).ink(Ink::Destructive)));
        }
        if let Some(ip) = model::self_ip(status) {
            parts.push(w::panel_row(s, "copy", "IP", None, vec![w::panel_value(ip.clone())]).stop("self").on(format!("copy:{ip}")));
        }

        let primary = v.theme.colors.get("primary");
        let muted = v.theme.colors.get("mutedForeground");
        let mut peers = vec![w::panel_heading_with(s, "Peers", status.peers.len().to_string())];
        if status.peers.is_empty() {
            peers.push(label("None"));
        }
        let rows = status
            .peers
            .iter()
            .enumerate()
            .map(|(i, p)| {
                let detail = [p.os.clone(), p.ip.clone()].into_iter().flatten().collect::<Vec<_>>().join("  ");
                let dot = w::dot(if p.online { primary } else { muted }, s.sm).pulse(t.action == Action::Up);
                let row = w::panel_row(s, "", p.name.clone(), Some(detail), vec![dot]).stop(format!("peer{i}"));
                match &p.ip {
                    Some(ip) => row.on(format!("copy:{ip}")),
                    None => row,
                }
            })
            .collect();
        peers.push(w::column(0.0, rows));
        parts.push(w::column(s.panel_gap, peers));
        w::column(0.0, parts)
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
