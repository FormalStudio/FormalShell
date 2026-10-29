//! The adapter's power `Switch` and a rescan button in
//! the header, a hero naming the one connected device (or the adapter), and
//! the rows split into Paired (connected first, each carrying a check) and
//! Available. Enter connects or disconnects the row under the cursor, or
//! pairs, trusts and connects an available one; `x` forgets a paired one.
//! Trust and forget show on the row holding the cursor.
//!
//! The action in flight and its failure live in the service
//! (`services::devices::bluetooth`), settled by BlueZ's own answers; the
//! VM has no adapter, so the rig sees the honest "No adapter" row.

use fs_devices::bluetooth::{self as model, Device};

use super::{Effect, Panel, View};
use crate::services::devices::bluetooth::{ActionKind, Ask, Bluetooth as Bt, ask};
use crate::store::Topic;
use crate::ui::{El, Event, Ink, What, w};

#[derive(Default)]
pub struct Bluetooth;

#[derive(Clone, Copy, PartialEq)]
enum Bucket {
    Connected,
    Paired,
    Available,
}

fn label(d: &Device) -> &str {
    if d.name.is_empty() { &d.device_name } else { &d.name }
}

fn rows(bt: &Bt) -> Vec<(Bucket, &Device)> {
    let b = model::buckets(&bt.devices, bt.discovering);
    let mut out: Vec<(Bucket, &Device)> = b.connected.into_iter().map(|d| (Bucket::Connected, d)).collect();
    out.extend(b.known.into_iter().map(|d| (Bucket::Paired, d)));
    out.extend(b.available.into_iter().map(|d| (Bucket::Available, d)));
    out
}

fn find<'a>(bt: &'a Bt, address: &str) -> Option<(Bucket, &'a Device)> {
    rows(bt).into_iter().find(|(_, d)| d.address == address)
}

fn row(v: &View, bt: &Bt, bucket: Bucket, d: &Device) -> El {
    let s = &v.theme.space;
    let key = format!("dev:{}", d.address);
    let revealed = v.cursor == Some(key.as_str());
    let busy = bt.action.as_ref().filter(|(a, _)| *a == d.address).map(|(_, k)| *k);
    let failed = bt.failure.as_ref().filter(|(a, _)| *a == d.address && busy.is_none()).map(|(_, t)| *t);
    let trust_pending = matches!(busy, Some(ActionKind::Trust | ActionKind::Untrust));
    let status = match (busy, failed) {
        (Some(k), _) => k.text().to_owned(),
        (None, Some(t)) => t.to_owned(),
        _ => {
            let activity = model::activity_text(Some(d));
            if !activity.is_empty() {
                activity.to_owned()
            } else if d.trusted && !trust_pending {
                "Trusted".to_owned()
            } else {
                String::new()
            }
        }
    };
    let mut trailing = Vec::new();
    let battery = model::battery_text(Some(d));
    if !battery.is_empty() {
        trailing.push(w::value(battery).ink(Ink::Dim));
    }
    let idle = bt.action.is_none();
    if d.paired && revealed {
        let word = if d.trusted { "Untrust" } else { "Trust" };
        let el = w::section_label(s, word, None, false).ink(Ink::Dim);
        trailing.push(if idle { el.on(format!("trust:{}", d.address)) } else { el });
    }
    if bucket == Bucket::Paired && revealed {
        let el = w::icon("trash").ink(Ink::Dim);
        trailing.push(if idle { el.on(format!("forget:{}", d.address)).tip("Forget") } else { el });
    }
    if bucket == Bucket::Connected {
        trailing.push(w::icon("check").ink(Ink::Primary));
    }
    let glyph = if bucket == Bucket::Connected { "bluetooth-connected" } else { "bluetooth" };
    let mut line = vec![w::icon(glyph)];
    line.push(w::label(label(d)).elide());
    line.extend(trailing);
    let mut parts = vec![w::row(s.icon_gap, line).fill()];
    if !status.is_empty() {
        let ink = if failed.is_some() { Ink::Destructive } else { Ink::Dim };
        parts.push(w::section_label(s, &status, None, false).ink(ink));
    }
    let cell = w::cell(w::column(s.xs, parts)).ghost().stop(key);
    let cell = if idle { cell.interactive().on(format!("row:{}", d.address)) } else { cell };
    cell
}

fn run(address: &str, kind: ActionKind) {
    ask(Ask::Run(address.to_owned(), kind));
}

fn activate(bt: &Bt, address: &str) {
    if bt.action.is_some() {
        return;
    }
    let Some((bucket, _)) = find(bt, address) else { return };
    run(
        address,
        match bucket {
            Bucket::Connected => ActionKind::Disconnect,
            Bucket::Paired => ActionKind::Connect,
            Bucket::Available => ActionKind::Pair,
        },
    );
}

fn forget(bt: &Bt, address: &str) {
    if bt.action.is_none() && find(bt, address).is_some_and(|(b, _)| b == Bucket::Paired) {
        run(address, ActionKind::Forget);
    }
}

/// Only a device BlueZ reports `paired` has anything to trust.
fn trust(bt: &Bt, address: &str) {
    if bt.action.is_some() {
        return;
    }
    if let Some((_, d)) = find(bt, address).filter(|(_, d)| d.paired) {
        run(address, if d.trusted { ActionKind::Untrust } else { ActionKind::Trust });
    }
}

impl Panel for Bluetooth {
    fn id(&self) -> &'static str {
        "bluetooth"
    }

    fn title(&self, _: &View) -> String {
        "Bluetooth".into()
    }

    fn icon(&self, _: &View) -> String {
        "bluetooth".into()
    }

    fn reads(&self) -> &'static [Topic] {
        &[Topic::Devices]
    }

    fn actions(&self, v: &View) -> Vec<El> {
        let bt = &v.store.devices.bluetooth;
        let on = bt.powered == Some(true);
        let mut out = vec![w::switch(on).on("power")];
        let rescan = w::icon_button("refresh-cw").tip("Rescan").key("rescan");
        out.push(if on { rescan.on("rescan") } else { rescan.enabled(false) });
        out
    }

    fn body(&self, v: &View) -> El {
        let s = &v.theme.space;
        let bt = &v.store.devices.bluetooth;
        if bt.powered.is_none() {
            return w::column(s.section_gap, vec![w::section_label(s, "No adapter", None, true)]);
        }
        let all = rows(bt);
        let connected: Vec<&Device> = all.iter().filter(|(b, _)| *b == Bucket::Connected).map(|(_, d)| *d).collect();
        let hero_dev = (connected.len() == 1).then(|| connected[0]);
        let enabled = bt.powered == Some(true);
        let glyph = if !enabled {
            "bluetooth-off"
        } else if connected.is_empty() {
            "bluetooth"
        } else {
            "bluetooth-connected"
        };
        let (title, meta, battery) = match hero_dev {
            Some(d) => {
                let activity = model::activity_text(Some(d));
                (label(d).to_owned(), if activity.is_empty() { "Connected".to_owned() } else { activity.to_owned() }, model::battery_text(Some(d)))
            }
            None => (bt.adapter.clone(), bt.state.to_owned(), String::new()),
        };
        let hero = w::hero(
            s,
            w::Hero {
                glyph: glyph.into(),
                title,
                meta,
                readout: String::new(),
                trailing: (!battery.is_empty()).then(|| w::value(battery).ink(Ink::Dim)),
                rail: None,
                rail_on: None,
            },
        );
        let mut out = vec![hero];
        let paired: Vec<El> = all.iter().filter(|(b, _)| *b != Bucket::Available).map(|(b, d)| row(v, bt, *b, d)).collect();
        let available: Vec<El> = all.iter().filter(|(b, _)| *b == Bucket::Available).map(|(b, d)| row(v, bt, *b, d)).collect();
        if paired.is_empty() && available.is_empty() {
            out.push(w::section_label(s, if enabled { "Scanning\u{2026}" } else { "Turn on to scan" }, None, true));
        }
        if !paired.is_empty() {
            let n = paired.len();
            out.push(w::section(s, "Paired", Some(n), paired));
        }
        if !available.is_empty() {
            let n = available.len();
            out.push(w::section(s, "Available", Some(n), available));
        }
        w::column(s.section_gap, out)
    }

    fn event(&mut self, ev: &Event, fx: &mut Effect) {
        let bt = &fx.store.devices.bluetooth;
        match (ev.on.as_str(), &ev.what) {
            ("power", What::Toggle(on)) => ask(Ask::Power(*on)),
            ("rescan", What::Click) => ask(Ask::Rescan),
            (on, What::Click) => {
                if let Some(a) = on.strip_prefix("row:") {
                    activate(bt, a);
                } else if let Some(a) = on.strip_prefix("forget:") {
                    forget(bt, a);
                } else if let Some(a) = on.strip_prefix("trust:") {
                    trust(bt, a);
                }
            }
            _ => {}
        }
    }

    fn activate(&mut self, stop: &str, fx: &mut Effect) {
        if let Some(a) = stop.strip_prefix("dev:") {
            activate(&fx.store.devices.bluetooth, a);
        }
    }

    fn delete(&mut self, stop: &str, fx: &mut Effect) {
        if let Some(a) = stop.strip_prefix("dev:") {
            forget(&fx.store.devices.bluetooth, a);
        }
    }
}
