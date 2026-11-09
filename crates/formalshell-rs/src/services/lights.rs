//! Keyboard RGB. The one backend is asusd: state is read
//! off its Aura object over busctl (LedMode, LedModeData, Brightness,
//! SupportedBasicModes) and written through asusctl. asusd keeps the effect
//! across reboots itself, so the effect, colour and brightness here are
//! always what the last probe read back; only the colour source, the custom
//! colour and the toggle's restore level live in state.json's `lights`.
//!
//! `source: "wallpaper"` repaints a colour-taking effect with the theme's
//! primary each time the palette settles; picking a colour switches it to
//! "custom". No asusctl, or no Aura object on the bus, leaves `available`
//! false and every setter refused.

use std::sync::LazyLock;
use std::time::{Duration, Instant};

use async_channel::{Receiver, Sender};
use async_io::Timer;
use fs_system::lights as model;
use futures_lite::FutureExt;
use serde_json::{Map, Value, json};

use crate::runtime::Ctx;
use crate::services::proc::{self, argv};
use crate::services::state::{self, Field};
use crate::store;

/// Theme.color.primary crossfades over `motion.reveal`, so every frame of
/// the fade is a new value; the repaint waits for the last one.
const PALETTE_SETTLE: Duration = Duration::from_secs(1);
const PROBE_TIMEOUT: Duration = Duration::from_secs(10);

/// Exit 3 is this script's own "no asusctl", 4 "no Aura object".
const PROBE: &str = "command -v asusctl >/dev/null || exit 3; \
    p=$(busctl --system --list tree xyz.ljones.Asusd 2>/dev/null | grep -m1 \"^/xyz/ljones/aura/\"); \
    [ -n \"$p\" ] || exit 4; \
    g() { busctl --system get-property xyz.ljones.Asusd \"$p\" xyz.ljones.Aura \"$1\" 2>/dev/null; }; \
    echo \"MODE=$(g LedMode)\"; echo \"DATA=$(g LedModeData)\"; \
    echo \"BRIGHT=$(g Brightness)\"; echo \"MODES=$(g SupportedBasicModes)\"";

#[derive(Clone, Debug, PartialEq)]
pub struct State {
    pub available: bool,
    pub effect: String,
    pub colour: String,
    pub speed: String,
    pub brightness: i64,
    /// Id and label of each effect this chassis reports.
    pub effects: Vec<(String, String)>,
    pub last_error: String,
    pub source: String,
    pub custom_colour: String,
    pub palette_colour: String,
}

impl Default for State {
    fn default() -> Self {
        Self {
            available: false,
            effect: String::new(),
            colour: String::new(),
            speed: "med".into(),
            brightness: -1,
            effects: Vec::new(),
            last_error: String::new(),
            source: "wallpaper".into(),
            custom_colour: "ffffff".into(),
            palette_colour: String::new(),
        }
    }
}

pub struct Diff(pub State);

impl State {
    pub fn apply(&mut self, diff: Diff) -> bool {
        let changed = *self != diff.0;
        *self = diff.0;
        changed
    }

    pub fn on(&self) -> bool {
        self.brightness > 0
    }

    pub fn has_effect(&self, id: &str) -> bool {
        self.effects.iter().any(|(e, _)| e == id)
    }

    /// The IPC `status` reply.
    pub fn status(&self) -> String {
        let effects: Vec<&str> = self.effects.iter().map(|(id, _)| id.as_str()).collect();
        let text = |s: &str| Value::String(s.to_owned());
        format!(
            r#"{{"available":{},"on":{},"brightness":{},"effect":{},"color":{},"speed":{},"source":{},"customColor":{},"paletteColor":{},"effects":{},"lastError":{}}}"#,
            self.available,
            self.on(),
            self.brightness,
            text(&self.effect),
            text(&self.colour),
            text(&self.speed),
            text(&self.source),
            text(&self.custom_colour),
            text(&self.palette_colour),
            serde_json::to_string(&effects).unwrap_or_else(|_| "[]".into()),
            text(&self.last_error),
        )
    }
}

pub enum Cmd {
    Refresh,
    Toggle,
    Brightness(i64),
    Effect(String),
    Speed(String),
    Colour(String),
    Source(String),
}

enum Msg {
    Cmd(Cmd),
    Palette(String),
    Record(Value),
    Probed(Option<model::Probe>),
    Written(String),
}

static CHANNEL: LazyLock<(Sender<Msg>, Receiver<Msg>)> = LazyLock::new(async_channel::unbounded);

fn send(msg: Msg) {
    let _ = CHANNEL.0.try_send(msg);
}

/// What an IPC verb asked for, already checked against the state the UI
/// holds.
pub fn command(cmd: Cmd) {
    send(Msg::Cmd(cmd));
}

/// The theme's primary, as rrggbb, from the UI thread.
pub fn palette(hex: String) {
    send(Msg::Palette(hex));
}

/// state.json's `lights` record, from the UI thread.
pub fn record(value: Value) {
    send(Msg::Record(value));
}

struct Machine {
    ctx: Ctx,
    state: State,
    record: Map<String, Value>,
    palette_ready: bool,
    queue: Vec<Vec<String>>,
    writing: bool,
    probing: bool,
    settle_at: Option<Instant>,
}

impl Machine {
    fn new(ctx: Ctx) -> Self {
        Self {
            ctx,
            state: State::default(),
            record: Map::new(),
            palette_ready: false,
            queue: Vec::new(),
            writing: false,
            probing: false,
            settle_at: None,
        }
    }

    fn publish(&mut self) {
        let source = self.record.get("source").and_then(Value::as_str).filter(|s| model::SOURCES.contains(s)).unwrap_or("wallpaper");
        self.state.source = source.to_owned();
        let custom = model::normalize_hex(self.record.get("colour").and_then(Value::as_str));
        self.state.custom_colour = if custom.is_empty() { "ffffff".into() } else { custom };
        self.ctx.publish(store::Diff::Lights(Diff(self.state.clone())));
    }

    fn on_level(&self) -> i64 {
        match self.record.get("onLevel").and_then(Value::as_i64) {
            Some(n) if n > 0 => n,
            _ => model::BRIGHTNESS.len() as i64 - 1,
        }
    }

    fn save(&mut self, fields: &[(&str, Value)]) {
        for (k, v) in fields {
            self.record.insert((*k).to_owned(), v.clone());
        }
        state::set(vec![Field::Lights(Value::Object(self.record.clone()))]);
    }

    fn wanted_colour(&self) -> String {
        if self.state.source == "custom" { self.state.custom_colour.clone() } else { self.state.palette_colour.clone() }
    }

    /// Updates what the menu shows first, so a tick moves in the same turn
    /// as the key press; the probe after the write corrects it if asusd
    /// disagreed.
    fn apply_effect(&mut self, id: &str, hex: &str, speed: &str) {
        let Some(args) = model::effect_args(id, hex, speed) else { return };
        self.state.effect = id.to_owned();
        if model::uses_colour(id) {
            self.state.colour = hex.to_owned();
        }
        self.send(args);
    }

    /// One write at a time, and a newer write of the same kind replaces a
    /// queued one, so holding Down through the effect list sends the last.
    fn send(&mut self, args: Vec<String>) {
        self.queue.retain(|a| a.get(1) != args.get(1));
        self.queue.push(args);
        self.pump();
    }

    fn pump(&mut self) {
        if self.writing || self.queue.is_empty() {
            return;
        }
        let args = self.queue.remove(0);
        self.writing = true;
        let tx = CHANNEL.0.clone();
        self.ctx.spawn(async move {
            let out = crate::services::proc::command(&args[0])
                .args(&args[1..])
                .stdin(async_process::Stdio::null())
                .stdout(async_process::Stdio::null())
                .output()
                .await;
            let error = match out {
                Ok(o) if o.status.success() => String::new(),
                Ok(o) => {
                    let text = String::from_utf8_lossy(&o.stderr).trim().to_owned();
                    if text.is_empty() { format!("asusctl exited {}", o.status.code().unwrap_or(-1)) } else { text }
                }
                Err(_) => "asusctl exited 127".to_owned(),
            };
            let _ = tx.send(Msg::Written(error)).await;
        });
    }

    fn probe(&mut self) {
        if self.probing {
            return;
        }
        self.probing = true;
        let tx = CHANNEL.0.clone();
        self.ctx.spawn(async move {
            let done = proc::capture(&argv(&["sh", "-c", PROBE]), PROBE_TIMEOUT).await;
            let probe = (done.code == 0).then(|| model::parse_probe(&done.stdout));
            let _ = tx.send(Msg::Probed(probe)).await;
        });
    }

    fn follow_palette(&mut self) {
        let s = &self.state;
        if !s.available || s.source != "wallpaper" || !model::uses_colour(&s.effect) || !self.palette_ready {
            return;
        }
        if s.colour != s.palette_colour {
            let (effect, colour, speed) = (s.effect.clone(), s.palette_colour.clone(), s.speed.clone());
            self.apply_effect(&effect, &colour, &speed);
        }
    }

    fn set_brightness(&mut self, level: i64) {
        let Some(args) = model::brightness_args(level) else { return };
        if !self.state.available {
            return;
        }
        if level == 0 && self.state.brightness > 0 {
            let on = self.state.brightness;
            self.save(&[("onLevel", json!(on))]);
        }
        self.state.brightness = level;
        self.send(args);
    }

    fn command(&mut self, cmd: Cmd) {
        match cmd {
            Cmd::Refresh => self.probe(),
            _ if !self.state.available => {}
            Cmd::Toggle => {
                let level = if self.state.on() { 0 } else { self.on_level() };
                self.set_brightness(level);
            }
            Cmd::Brightness(level) => self.set_brightness(level),
            Cmd::Effect(id) => {
                let (colour, speed) = (self.wanted_colour(), self.state.speed.clone());
                self.apply_effect(&id, &colour, &speed);
            }
            Cmd::Speed(value) => {
                self.state.speed = value.clone();
                if let Some(e) = model::effect(&self.state.effect).filter(|e| e.speed) {
                    let colour = self.wanted_colour();
                    self.apply_effect(e.id, &colour, &value);
                }
            }
            Cmd::Colour(hex) => {
                self.save(&[("source", json!("custom")), ("colour", json!(hex))]);
                let id = if model::uses_colour(&self.state.effect) { self.state.effect.clone() } else { "static".into() };
                let speed = self.state.speed.clone();
                self.state.source = "custom".into();
                self.state.custom_colour = hex.clone();
                self.apply_effect(&id, &hex, &speed);
            }
            Cmd::Source(value) => {
                self.save(&[("source", json!(value))]);
                self.state.source = value;
                if model::uses_colour(&self.state.effect) {
                    let (effect, colour, speed) = (self.state.effect.clone(), self.wanted_colour(), self.state.speed.clone());
                    self.apply_effect(&effect, &colour, &speed);
                }
            }
        }
    }

    fn probed(&mut self, probe: Option<model::Probe>) {
        self.probing = false;
        let Some(probe) = probe else {
            self.state.available = false;
            return;
        };
        let current = model::effect_for_mode(probe.mode);
        self.state.effects = model::supported(&probe.modes).iter().map(|e| (e.id.to_owned(), e.label.to_owned())).collect();
        self.state.effect = current.map_or(String::new(), |e| e.id.to_owned());
        self.state.colour = probe.colour;
        if model::SPEEDS.contains(&probe.speed.as_str()) {
            self.state.speed = probe.speed;
        }
        self.state.brightness = probe.brightness;
        self.state.available = true;
        self.follow_palette();
    }

    fn handle(&mut self, msg: Msg) {
        match msg {
            Msg::Cmd(cmd) => self.command(cmd),
            Msg::Palette(hex) => {
                self.palette_ready = true;
                if self.state.palette_colour != hex {
                    self.state.palette_colour = hex;
                    self.settle_at = Some(Instant::now() + PALETTE_SETTLE);
                }
            }
            Msg::Record(Value::Object(map)) => self.record = map,
            Msg::Record(_) => self.record = Map::new(),
            Msg::Probed(probe) => self.probed(probe),
            Msg::Written(error) => {
                self.writing = false;
                self.state.last_error = error;
                if self.queue.is_empty() {
                    self.probe();
                } else {
                    self.pump();
                }
            }
        }
    }
}

enum Wake {
    Msg(Option<Msg>),
    Settled,
}

pub fn start(ctx: &Ctx) {
    ctx.spawn(run(ctx.clone()));
}

async fn run(ctx: Ctx) {
    let rx = CHANNEL.1.clone();
    let mut m = Machine::new(ctx);
    m.probe();
    loop {
        let recv = async { Wake::Msg(rx.recv().await.ok()) };
        let wake = match m.settle_at {
            Some(at) => {
                let timer = async {
                    Timer::at(at).await;
                    Wake::Settled
                };
                recv.or(timer).await
            }
            None => recv.await,
        };
        match wake {
            Wake::Msg(None) => return,
            Wake::Msg(Some(msg)) => m.handle(msg),
            Wake::Settled => {
                m.settle_at = None;
                m.follow_palette();
            }
        }
        m.publish();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_reads_like_the_golden_reply() {
        let mut s = State::default();
        assert_eq!(
            s.status(),
            r#"{"available":false,"on":false,"brightness":-1,"effect":"","color":"","speed":"med","source":"wallpaper","customColor":"ffffff","paletteColor":"","effects":[],"lastError":""}"#
        );
        s.available = true;
        s.brightness = 3;
        s.effects = vec![("static".into(), "Static".into()), ("breathe".into(), "Breathe".into())];
        assert!(s.on());
        assert!(s.status().contains(r#""effects":["static","breathe"]"#));
        assert!(s.has_effect("breathe") && !s.has_effect("disco"));
    }
}
