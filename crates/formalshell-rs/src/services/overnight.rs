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

use fs_system::overnight::{self, AURA_ZONES, SCREEN_PERCENT};
use fs_upower::{PowerProfiles, Profile};
use serde_json::{Map, Value, json};

use crate::runtime::Ctx;
use crate::services::display;
use crate::services::state::{self, Field};

thread_local! {
    /// The record this thread last wrote while active, for the merges the
    /// LED listing and the Aura probe make once they finish.
    static RECORD: RefCell<Option<Map<String, Value>>> = const { RefCell::new(None) };
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

/// The `PowerProfile` enum numbers, what the record has always held.
fn profile_number(p: Profile) -> i64 {
    match p {
        Profile::PowerSaver => 0,
        Profile::Balanced => 1,
        Profile::Performance => 2,
    }
}

fn profile_of(n: i64) -> Option<Profile> {
    match n {
        0 => Some(Profile::PowerSaver),
        1 => Some(Profile::Balanced),
        2 => Some(Profile::Performance),
        _ => None,
    }
}

fn write(record: Option<Map<String, Value>>) {
    state::set(vec![Field::Overnight(record.clone().map_or(Value::Null, Value::Object))]);
    RECORD.with_borrow_mut(|r| *r = record);
}

/// Merges one key into the stored record; nothing once disabled.
fn record(key: &str, value: Value) {
    let next = RECORD.with_borrow(|r| {
        r.clone().map(|mut m| {
            m.insert(key.into(), value);
            m
        })
    });
    if next.is_some() {
        write(next);
    }
}

async fn output(argv: &[&str]) -> Option<(i32, String)> {
    let out = crate::services::proc::command(argv[0]).args(&argv[1..]).stderr(async_process::Stdio::null()).output().await.ok()?;
    Some((out.status.code().unwrap_or(-1), String::from_utf8_lossy(&out.stdout).into_owned()))
}

async fn profiles() -> Option<PowerProfiles> {
    let conn = crate::services::bus::system().await.ok()?;
    PowerProfiles::connect(&conn).await.ok()
}

const LED_LIST: &str = "for d in /sys/class/leds/*; do printf '%s\\t%s\\t%s\\n' \"${d##*/}\" \"$(cat \"$d/trigger\" 2>/dev/null)\" \"$(cat \"$d/brightness\" 2>/dev/null)\"; done";
const LED_OFF: &str = "for n; do brightnessctl -q -d \"$n\" set 0; done";
const LED_RESTORE: &str = "while [ $# -ge 2 ]; do brightnessctl -q -d \"$1\" set \"$2\"; shift 2; done";
// `asusctl aura power <zone>` with no flags clears every power state for the
// zone, which is off. Exit 3 is this script's own "no asusctl".
const AURA_OFF: &str = "command -v asusctl >/dev/null || exit 3; for z; do asusctl aura power \"$z\" >/dev/null 2>&1; done; exit 0";
// The states the host's asus-aura unit sets at boot (sleep left off, so the
// zones stay dark while suspended).
const AURA_ON: &str = "for z; do asusctl aura power \"$z\" --boot --awake --shutdown >/dev/null 2>&1; done; exit 0";

fn sh(script: &str, args: &[String]) -> Vec<String> {
    let mut argv = vec!["sh".to_owned(), "-c".to_owned(), script.to_owned(), "sh".to_owned()];
    argv.extend(args.iter().cloned());
    argv
}

async fn run(argv: Vec<String>) -> Option<(i32, String)> {
    let refs: Vec<&str> = argv.iter().map(String::as_str).collect();
    output(&refs).await
}

pub fn enable(ctx: &Ctx) {
    // A second enable that reached here before the store saw the first.
    if RECORD.with_borrow(Option::is_some) {
        return;
    }
    let generation = next_generation();
    // Empty until the reads below land, but the store reads active at once,
    // so a disable right behind this one is not dropped as a no-op.
    write(Some(Map::new()));
    let ctx = ctx.clone();
    ctx.clone().spawn(async move {
        let profiles = profiles().await;
        let active = match &profiles {
            Some(p) => p.state().await.ok().map(|s| s.active),
            None => None,
        };
        let screens = display::levels(&ctx).await;
        if !current(generation) {
            return;
        }
        let mut rec = Map::new();
        if let Some(p) = active {
            rec.insert("profile".into(), json!(profile_number(p)));
        }
        let backlight = screens.iter().find(|(id, ..)| id == "backlight").map_or(-1, |(_, pct, _)| *pct);
        rec.insert("backlight".into(), json!(backlight));
        let ddc: Map<String, Value> =
            screens.iter().filter(|(id, pct, _)| id != "backlight" && *pct > SCREEN_PERCENT).map(|(id, pct, _)| (id.clone(), json!(pct))).collect();
        rec.insert("ddc".into(), Value::Object(ddc));
        rec.insert("leds".into(), json!({}));
        rec.insert("aura".into(), json!(false));
        write(Some(rec));
        // Recorded first, so a restart from here on still knows what to put back.
        for (id, pct, _) in &screens {
            if *pct > SCREEN_PERCENT {
                display::set_level(&ctx, id, SCREEN_PERCENT);
            }
        }
        if let (Some(p), Some(Profile::Performance)) = (&profiles, active) {
            let _ = p.set_active(Profile::Balanced).await;
        }
        let zones: Vec<String> = AURA_ZONES.iter().map(|z| z.to_string()).collect();
        let leds = async {
            let Some((_, text)) = run(sh(LED_LIST, &[])).await else { return };
            let leds = overnight::parse_leds(&text);
            if leds.is_empty() || !current(generation) {
                return;
            }
            let snapshot: Map<String, Value> = overnight::led_snapshot(&leds).into_iter().map(|(n, b)| (n, json!(b))).collect();
            record("leds", Value::Object(snapshot));
            let names: Vec<String> = leds.into_iter().map(|l| l.name).collect();
            let _ = run(sh(LED_OFF, &names)).await;
        };
        let aura = async {
            if let Some((0, _)) = run(sh(AURA_OFF, &zones)).await
                && current(generation)
            {
                record("aura", json!(true));
            }
        };
        futures_lite::future::zip(leds, aura).await;
    });
}

/// `snap`: the record state.json holds, which is what gets put back unless
/// this thread holds a newer one.
pub fn disable(ctx: &Ctx, snap: Value) {
    next_generation();
    let snap = RECORD.with_borrow_mut(Option::take).map_or(snap, Value::Object);
    write(None);
    let mut screens: Vec<(String, i64)> = Vec::new();
    if let Some(level) = snap.get("backlight").and_then(Value::as_f64).filter(|l| *l >= 0.0) {
        screens.push(("backlight".into(), level.round() as i64));
    }
    if let Some(Value::Object(ddc)) = snap.get("ddc") {
        screens.extend(ddc.iter().filter_map(|(id, pct)| Some((id.clone(), pct.as_f64()?.round() as i64))));
    }
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
        if let Some(want) = snap.get("profile").and_then(Value::as_i64).and_then(profile_of)
            && let Some(p) = profiles().await
            && p.state().await.is_ok_and(|s| s.active != want)
        {
            let _ = p.set_active(want).await;
        }
        let leds: Vec<(String, Value)> = match snap.get("leds") {
            Some(Value::Object(m)) => m.iter().map(|(k, v)| (k.clone(), v.clone())).collect(),
            _ => Vec::new(),
        };
        let restore = overnight::restore_args(&leds);
        if snap.get("aura") == Some(&Value::Bool(true)) {
            let zones: Vec<String> = AURA_ZONES.iter().map(|z| z.to_string()).collect();
            let _ = run(sh(AURA_ON, &zones)).await;
        }
        if !restore.is_empty() {
            let _ = run(sh(LED_RESTORE, &restore)).await;
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
