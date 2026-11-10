//! The machine left compiling while its owner sleeps.
//! Screens to 1%, every LED the shell may touch off, the Aura zones dark,
//! and a Performance profile dropped to Balanced (power-saver would also
//! pin the CPU's energy preference and drag a long build out).
//!
//! Everything changed is recorded in state.json's `overnight` before or as
//! it lands, so disable restores it even after a restart: `{ profile,
//! backlight, ddc, leds, aura }`, backlight -1 with no backlight to dim.
//! The UI decides enable or disable off the store's record; the work runs
//! here, on the service thread.
//!
//! The screens are the display service's devices, read and written through
//! its per-device writers: `ddc` records each monitor's percent keyed by
//! connector, and disable puts every screen back through the same queue,
//! so a write still out from the dim lands before the restore and the rows
//! the brightness keys step from end on the restored level.

use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::time::Duration;

use fs_system::overnight::{self, SCREEN_PERCENT};
use fs_upower::Profile;
use fs_js::lenient;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Number, Value, json};

use crate::runtime::Ctx;
use crate::services::devices::power;
use crate::services::display;
use crate::services::lights;
use crate::services::proc;
use crate::services::state::{self, Field};

/// state.json's `overnight`, present while it is on. Empty from the enable
/// until the reads land, each key then written as it is known. Numbers keep
/// the form they were written in and are rounded only on the way back.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Record {
    /// The `PowerProfile` enum number.
    #[serde(deserialize_with = "lenient::field", skip_serializing_if = "Option::is_none")]
    pub profile: Option<Number>,
    #[serde(deserialize_with = "lenient::field", skip_serializing_if = "Option::is_none")]
    pub backlight: Option<Number>,
    /// Percent by connector.
    #[serde(deserialize_with = "lenient::entries", skip_serializing_if = "Option::is_none")]
    pub ddc: Option<BTreeMap<String, Number>>,
    /// Brightness by LED name, a number or the string `brightnessctl` read.
    #[serde(deserialize_with = "lenient::field", skip_serializing_if = "Option::is_none")]
    pub leds: Option<BTreeMap<String, Value>>,
    #[serde(deserialize_with = "lenient::field", skip_serializing_if = "Option::is_none")]
    pub aura: Option<bool>,
    /// Keys this shell does not write, kept as they are.
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl Record {
    /// The screens to put back and their percent: the backlight unless there
    /// was none to dim, and every DDC monitor.
    fn screens(&self) -> Vec<(String, i64)> {
        let backlight = self.backlight.as_ref().and_then(Number::as_f64).filter(|l| *l >= 0.0).map(|l| ("backlight".to_owned(), l.round() as i64));
        let ddc = self.ddc.iter().flatten().filter_map(|(id, pct)| Some((id.clone(), pct.as_f64()?.round() as i64)));
        backlight.into_iter().chain(ddc).collect()
    }
}

thread_local! {
    /// The record this thread last wrote while active, for the merges the
    /// LED listing and the Aura probe make once they finish.
    static RECORD: RefCell<Option<Record>> = const { RefCell::new(None) };
    /// Bumped by every enable and disable: an enable still awaiting a read
    /// after a newer call stops there and touches nothing.
    static GENERATION: Cell<u64> = const { Cell::new(0) };
}

fn next_generation() -> u64 {
    GENERATION.with(|g| {
        g.set(g.get() + 1);
        g.get()
    })
}

fn current(generation: u64) -> bool {
    GENERATION.with(Cell::get) == generation
}

/// The record holds a profile as its place in [`Profile::ALL`], power
/// saver first.
fn profile_number(p: Profile) -> i64 {
    Profile::ALL.iter().position(|x| *x == p).unwrap_or(1) as i64
}

fn profile_of(n: i64) -> Option<Profile> {
    usize::try_from(n).ok().and_then(|i| Profile::ALL.get(i)).copied()
}

fn write(record: Option<Record>) {
    state::set(vec![Field::Overnight(record.clone())]);
    RECORD.with_borrow_mut(|r| *r = record);
}

/// Merges into the stored record; nothing once disabled.
fn merge(change: impl FnOnce(&mut Record)) {
    let next = RECORD.with_borrow(|r| {
        r.clone().map(|mut rec| {
            change(&mut rec);
            rec
        })
    });
    if next.is_some() {
        write(next);
    }
}

const LED_TIMEOUT: Duration = Duration::from_secs(10);

const LED_LIST: &str = "for d in /sys/class/leds/*; do printf '%s\\t%s\\t%s\\n' \"${d##*/}\" \"$(cat \"$d/trigger\" 2>/dev/null)\" \"$(cat \"$d/brightness\" 2>/dev/null)\"; done";
const LED_OFF: &str = "for n; do brightnessctl -q -d \"$n\" set 0; done";
const LED_RESTORE: &str = "while [ $# -ge 2 ]; do brightnessctl -q -d \"$1\" set \"$2\"; shift 2; done";
fn sh(script: &str, args: &[String]) -> Vec<String> {
    let mut argv = vec!["sh".to_owned(), "-c".to_owned(), script.to_owned(), "sh".to_owned()];
    argv.extend(args.iter().cloned());
    argv
}


/// Overnight turned on or off from the UI thread; nothing while it already is.
pub fn set(store: &crate::store::Store, runtime: &crate::runtime::Runtime, want: bool) {
    let active = store.state.data.overnight.as_ref();
    if active.is_some() == want {
        return;
    }
    let snap = active.cloned().unwrap_or_default();
    if want {
        runtime.service(enable);
    } else {
        runtime.service(move |ctx| disable(ctx, snap));
    }
}

pub fn enable(ctx: &Ctx) {
    // A second enable that reached here before the store saw the first.
    if RECORD.with_borrow(Option::is_some) {
        return;
    }
    let generation = next_generation();
    // Empty until the reads below land, but the store reads active at once,
    // so a disable right behind this one is not dropped as a no-op.
    write(Some(Record::default()));
    let ctx = ctx.clone();
    ctx.clone().spawn(async move {
        let profiles = power::profiles().await.ok();
        let active = match &profiles {
            Some(p) => p.state().await.ok().map(|s| s.active),
            None => None,
        };
        let screens = display::levels(&ctx).await;
        if !current(generation) {
            return;
        }
        let backlight = screens.iter().find(|(id, ..)| id == "backlight").map_or(-1, |(_, pct, _)| *pct);
        let ddc = screens.iter().filter(|(id, pct, _)| id != "backlight" && *pct > SCREEN_PERCENT).map(|(id, pct, _)| (id.clone(), (*pct).into())).collect();
        write(Some(Record {
            profile: active.map(|p| profile_number(p).into()),
            backlight: Some(backlight.into()),
            ddc: Some(ddc),
            leds: Some(BTreeMap::new()),
            aura: Some(false),
            extra: Map::new(),
        }));
        // Recorded first, so a restart from here on still knows what to put back.
        for (id, pct, _) in &screens {
            if *pct > SCREEN_PERCENT {
                display::set_level(&ctx, id, SCREEN_PERCENT);
            }
        }
        if let (Some(p), Some(Profile::Performance)) = (&profiles, active) {
            let _ = p.set_active(Profile::Balanced).await;
        }
        let leds = async {
            let text = proc::capture(&sh(LED_LIST, &[]), LED_TIMEOUT).await.stdout;
            let leds = overnight::parse_leds(&text);
            if leds.is_empty() || !current(generation) {
                return;
            }
            let snapshot = overnight::led_snapshot(&leds).into_iter().map(|(n, b)| (n, json!(b))).collect();
            merge(|rec| rec.leds = Some(snapshot));
            let names: Vec<String> = leds.into_iter().map(|l| l.name).collect();
            proc::capture(&sh(LED_OFF, &names), LED_TIMEOUT).await;
        };
        let aura = async {
            if lights::aura_power(true).await && current(generation)
            {
                merge(|rec| rec.aura = Some(true));
            }
        };
        futures_lite::future::zip(leds, aura).await;
    });
}

/// `snap`: the record state.json holds, which is what gets put back unless
/// this thread holds a newer one.
pub fn disable(ctx: &Ctx, snap: Record) {
    next_generation();
    let snap = RECORD.with_borrow_mut(Option::take).unwrap_or(snap);
    write(None);
    let screens = snap.screens();
    // Queued now, behind whatever the dim still has out on each device.
    let unknown: Vec<(String, i64)> = screens.into_iter().filter(|(id, pct)| !restore(ctx, id, *pct)).collect();
    let ctx = ctx.clone();
    ctx.clone().spawn(async move {
        // A restart since the dim: the devices are read before they are set.
        if !unknown.is_empty() {
            display::levels(&ctx).await;
            for (id, pct) in &unknown {
                restore(&ctx, id, *pct);
            }
        }
        if let Some(want) = snap.profile.as_ref().and_then(Number::as_i64).and_then(profile_of)
            && let Ok(p) = power::profiles().await
            && p.state().await.is_ok_and(|s| s.active != want)
        {
            let _ = p.set_active(want).await;
        }
        let leds: Vec<(String, Value)> = snap.leds.unwrap_or_default().into_iter().collect();
        let restore = overnight::restore_args(&leds);
        if snap.aura == Some(true) {
            lights::aura_power(false).await;
        }
        if !restore.is_empty() {
            proc::capture(&sh(LED_RESTORE, &restore), LED_TIMEOUT).await;
        }
    });
}

/// Queues one screen's restore; false while the display service has not
/// read that device yet.
fn restore(ctx: &Ctx, id: &str, percent: i64) -> bool {
    if !display::known(id) {
        return false;
    }
    display::set_level(ctx, id, percent);
    true
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn a_bad_field_keeps_the_rest_and_numbers_round_on_restore() {
        let rec: Record = serde_json::from_value(json!({
            "profile": "x", "backlight": 40.6, "ddc": {"DP-1": 55.5, "DP-2": "x"}, "leds": {"a": 1}, "note": 1
        }))
        .unwrap();
        assert_eq!(rec.profile, None);
        assert_eq!(rec.screens(), [("backlight".to_owned(), 41), ("DP-1".to_owned(), 56)]);
        assert_eq!(serde_json::to_value(&rec).unwrap(), json!({"backlight": 40.6, "ddc": {"DP-1": 55.5}, "leds": {"a": 1}, "note": 1}));
        let none: Record = serde_json::from_value(json!({"backlight": -1})).unwrap();
        assert!(none.screens().is_empty());
    }
}
