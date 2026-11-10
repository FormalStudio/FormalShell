//! The adapter's power `Switch` and a rescan button in
//! the header, the rows split into Paired (connected first, each carrying a
//! check) and Available, and the adapter's name and state under them. Enter connects or disconnects the row under the cursor, or
//! pairs, trusts and connects an available one; `x` forgets a paired one.
//! Trust and forget show on the row holding the cursor.
//!
//! The action in flight and its failure live in the service
//! (`services::devices::bluetooth`), settled by BlueZ's own answers; the
//! VM has no adapter, so the rig sees the honest "No adapter" row.

use fs_devices::bluetooth::{self as model, Device};
use fs_types::BtAddress;

use super::{Effect, Panel, View};
use crate::services::devices::bluetooth::{ActionKind, Bluetooth as Bt, Cmd, send};
use crate::services::wants::Source;
use crate::store::{Store, Topic};
use crate::ui::{El, Event, Ink, On, Variant, What, w};

/// A press on a device's row or its buttons.
enum Press {
    Row(BtAddress),
    Forget(BtAddress),
    Trust(BtAddress),
}

impl crate::ui::on::HitId for Press {}

impl std::fmt::Display for Press {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Press::Row(a) => write!(f, "dev:{a}"),
            Press::Forget(a) => write!(f, "forget:{a}"),
            Press::Trust(a) => write!(f, "trust:{a}"),
        }
    }
}

#[derive(Default)]
pub struct Bluetooth;

#[derive(Clone, Copy, PartialEq)]
enum Bucket {
    Connected,
    Paired,
    Available,
}

fn rows(bt: &Bt) -> Vec<(Bucket, &Device)> {
    let b = model::buckets(&bt.devices, bt.discovering);
    let mut out: Vec<(Bucket, &Device)> = b.connected.into_iter().map(|d| (Bucket::Connected, d)).collect();
    out.extend(b.known.into_iter().map(|d| (Bucket::Paired, d)));
    out.extend(b.available.into_iter().map(|d| (Bucket::Available, d)));
    out
}

fn find(bt: &Bt, address: BtAddress) -> Option<(Bucket, &Device)> {
    rows(bt).into_iter().find(|(_, d)| d.address == address)
}

fn row(v: &View, bt: &Bt, bucket: Bucket, d: &Device) -> El {
    let s = &v.theme.space;
    let revealed = matches!(v.cursor.and_then(|c| c.get::<Press>()), Some(&Press::Row(a)) if a == d.address);
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
        trailing.push(w::panel_value(battery));
    }
    let idle = bt.action.is_none();
    if d.paired && revealed {
        let word = if d.trusted { "Untrust" } else { "Trust" };
        let el = w::button(word).variant(Variant::Outline);
        trailing.push(if idle { el.on(Press::Trust(d.address)) } else { el.enabled(false) });
    }
    if bucket == Bucket::Paired && revealed {
        let el = w::icon("trash").ink(Ink::Muted);
        trailing.push(if idle { el.on(Press::Forget(d.address)).tip("Forget") } else { el });
    }
    let connected = bucket == Bucket::Connected;
    let glyph = if connected { "bluetooth-connected" } else { "bluetooth" };
    let detail = (failed.is_none() && !status.is_empty()).then(|| status.clone());
    let row = w::panel_row_checked(s, glyph, &d.name, detail, trailing, connected).stop(Press::Row(d.address));
    let row = if idle { row.on(Press::Row(d.address)) } else { row };
    match failed {
        Some(_) => w::column(0.0, vec![row, w::panel_error(s, status)]),
        None => row,
    }
}

fn run(address: BtAddress, kind: ActionKind) {
    send(Cmd::Run(address, kind));
}

fn activate(bt: &Bt, address: BtAddress) {
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

fn forget(bt: &Bt, address: BtAddress) {
    if bt.action.is_none() && find(bt, address).is_some_and(|(b, _)| b == Bucket::Paired) {
        run(address, ActionKind::Forget);
    }
}

/// Only a device BlueZ reports `paired` has anything to trust.
fn trust(bt: &Bt, address: BtAddress) {
    if bt.action.is_some() {
        return;
    }
    if let Some((_, d)) = find(bt, address).filter(|(_, d)| d.paired) {
        run(address, if d.trusted { ActionKind::Untrust } else { ActionKind::Trust });
    }
}

impl Panel for Bluetooth {
    fn title(&self, _: &View) -> String {
        "Bluetooth".into()
    }

    fn reads(&self) -> &'static [Topic] {
        &[Topic::Bluetooth]
    }

    fn wants(&self, _: &Store) -> Vec<Source> {
        vec![Source::BluetoothDevices]
    }

    fn actions(&self, v: &View) -> Vec<El> {
        let bt = &v.store.bluetooth;
        let on = bt.powered == Some(true);
        let mut out = vec![w::switch(on).on("power")];
        let rescan = w::icon_button("refresh-cw").tip("Rescan").key("rescan");
        out.push(if on { rescan.on("rescan") } else { rescan.enabled(false) });
        out
    }

    fn body(&self, v: &View) -> El {
        let s = &v.theme.space;
        let bt = &v.store.bluetooth;
        if bt.powered.is_none() {
            return w::column(s.section_gap, vec![w::panel_empty(s, "No adapter")]);
        }
        let all = rows(bt);
        let enabled = bt.powered == Some(true);
        let mut out = Vec::new();
        let paired: Vec<El> = all.iter().filter(|(b, _)| *b != Bucket::Available).map(|(b, d)| row(v, bt, *b, d)).collect();
        let available: Vec<El> = all.iter().filter(|(b, _)| *b == Bucket::Available).map(|(b, d)| row(v, bt, *b, d)).collect();
        if paired.is_empty() && available.is_empty() {
            out.push(w::panel_empty(s, if enabled { "Scanning\u{2026}" } else { "Turn on to scan" }));
        }
        if !paired.is_empty() {
            out.push(w::panel_section_n(s, "Paired", paired.len(), paired, ""));
        }
        if !available.is_empty() {
            out.push(w::panel_section_n(s, "Available", available.len(), available, ""));
        }
        let adapter = format!("{} \u{b7} {}", bt.adapter, bt.state);
        out.push(w::panel_note(s, adapter));
        w::column(0.0, out)
    }

    fn event(&mut self, ev: &Event, fx: &mut Effect) {
        let bt = &fx.store.bluetooth;
        match (ev.on.as_str(), &ev.what) {
            ("power", What::Toggle(on)) => send(Cmd::Power(*on)),
            ("rescan", What::Click) => send(Cmd::Rescan),
            (_, What::Click) => match ev.on.get::<Press>() {
                Some(&Press::Row(a)) => activate(bt, a),
                Some(&Press::Forget(a)) => forget(bt, a),
                Some(&Press::Trust(a)) => trust(bt, a),
                None => {}
            },
            _ => {}
        }
    }

    fn activate(&mut self, stop: &On, fx: &mut Effect) {
        if let Some(&Press::Row(a)) = stop.get::<Press>() {
            activate(&fx.store.bluetooth, a);
        }
    }

    fn delete(&mut self, stop: &On, fx: &mut Effect) {
        if let Some(&Press::Row(a)) = stop.get::<Press>() {
            forget(&fx.store.bluetooth, a);
        }
    }
}
