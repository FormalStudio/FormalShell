//! What the settings say about the machine: outputs, input devices, the
//! environment, the cursor, and the session's start, reload and exit.

use std::{
    cell::RefCell,
    collections::BTreeMap,
    io::{Read, Write},
    os::unix::net::UnixStream,
    path::PathBuf,
    rc::Rc,
    time::Instant,
};

use formalwm_core::{
    geom::Rect as Box2,
    model::OutputGeo,
    settings::{Accel, ModeChoice, OutputFacts, Pointer, Settings, Touchpad},
};
use fs_wm_proto::{self as proto, event};
use serde_json::json;
use shojiwm_rs::{
    compositor::{OutputHdr, OutputMode, OutputPosition, OutputResolution},
    prelude::*,
    runtime_input::{
        RuntimeInputAccelProfile, RuntimeInputClickMethod, RuntimeInputDeviceConfig,
        RuntimeKeyboardInputConfig, RuntimePointerInputConfig, RuntimeTouchpadInputConfig,
    },
    runtime_workspace::{
        RuntimeWorkspaceConfigUpdate, RuntimeWorkspaceEntry, RuntimeWorkspaceGroupConfig,
    },
    ssd::WaylandOutputSnapshot,
};

use crate::{
    engine::{self, Engine, enter, look_of},
    layers,
};

/// How long the cursor waits for the shell before it shows anyway.
const SHELL_WAIT_MS: f64 = 10_000.0;

/// What `outputs.set` asked for, on top of the settings.
#[derive(Clone, Debug, Default)]
pub struct OutputOverride {
    pub enabled: Option<bool>,
    pub scale: Option<f64>,
    pub mirror: Option<String>,
    pub color: Option<proto::Color>,
}

thread_local! {
    static OVERRIDES: RefCell<BTreeMap<String, OutputOverride>> = const { RefCell::new(BTreeMap::new()) };
    /// The ext-workspace model as of the last turn; its factory runs inside
    /// turns and cannot read the engine.
    static WORKSPACES: RefCell<RuntimeWorkspaceConfigUpdate> = RefCell::new(RuntimeWorkspaceConfigUpdate::default());
}

pub fn settings_dir() -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
        .unwrap_or_default()
        .join("formalwm")
}

pub fn override_of(output: &str) -> OutputOverride {
    OVERRIDES.with(|overrides| overrides.borrow().get(output).cloned().unwrap_or_default())
}

pub fn override_output(request: proto::OutputSet) {
    OVERRIDES.with(|overrides| {
        let mut overrides = overrides.borrow_mut();
        let entry = overrides.entry(request.name).or_default();
        if request.enabled.is_some() {
            entry.enabled = request.enabled;
        }
        if request.scale.is_some() {
            entry.scale = request.scale;
        }
        if let Some(mirror) = request.mirror {
            entry.mirror = Some(mirror).filter(|source| !source.is_empty());
        }
        if request.color.is_some() {
            entry.color = request.color;
        }
    });
    COMPOSITOR.output.reconfigure();
}

fn facts(output: &WaylandOutputSnapshot) -> OutputFacts<'_> {
    OutputFacts {
        name: &output.name,
        description: output.description.as_deref().unwrap_or_default(),
        make: output.make.as_deref().unwrap_or_default(),
        model: output.model.as_deref().unwrap_or_default(),
        serial: output.serial.as_deref().unwrap_or_default(),
    }
}

fn accel(accel: Option<Accel>) -> Option<RuntimeInputAccelProfile> {
    accel.map(|accel| match accel {
        Accel::Adaptive => RuntimeInputAccelProfile::Adaptive,
        Accel::Flat => RuntimeInputAccelProfile::Flat,
    })
}

fn pointer(pointer: &Pointer) -> RuntimePointerInputConfig {
    RuntimePointerInputConfig {
        pointer_accel: pointer.sensitivity,
        accel_profile: accel(pointer.accel),
        natural_scroll: pointer.natural_scroll,
        ..Default::default()
    }
}

fn touchpad(touchpad: &Touchpad) -> RuntimeTouchpadInputConfig {
    RuntimeTouchpadInputConfig {
        pointer_accel: touchpad.sensitivity,
        accel_profile: accel(touchpad.accel),
        natural_scroll: touchpad.natural_scroll,
        tap_to_click: touchpad.tap_to_click,
        click_method: touchpad.clickfinger.map(|clickfinger| {
            if clickfinger {
                RuntimeInputClickMethod::Clickfinger
            } else {
                RuntimeInputClickMethod::ButtonAreas
            }
        }),
        scroll_factor: touchpad.scroll_factor,
        disable_while_typing: touchpad.disable_while_typing,
        ..Default::default()
    }
}

/// Hyprland's name for a device: lower case, everything else a dash.
fn device_name(name: &str) -> String {
    name.trim()
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect()
}

pub fn setup() {
    COMPOSITOR.output.configure(|context| {
        let settings = engine::settings();
        let lid_shut = settings.lid_output.is_some() && crate::lid::shut();
        context
            .connected
            .iter()
            .map(|output| {
                let entry = settings.output_for(&facts(output));
                let over = override_of(&output.name);
                let enabled = over
                    .enabled
                    .or(entry.and_then(|entry| entry.enabled))
                    .unwrap_or(true);
                if !enabled || (lid_shut && settings.is_lid_output(&facts(output))) {
                    return (output.name.clone(), Some(OutputConfig::disabled()));
                }
                let scale = over
                    .scale
                    .or(entry.and_then(|entry| entry.scale))
                    .unwrap_or(1.0);
                let mut config = OutputConfig::extend(scale);
                let choice = entry.map_or(ModeChoice::Preferred, |entry| entry.mode());
                config.resolution = match choice {
                    ModeChoice::Preferred => None,
                    ModeChoice::Exact(mode) => Some(OutputResolution::Exact {
                        width: mode.width,
                        height: mode.height,
                        refresh_rate: mode.refresh,
                    }),
                    ModeChoice::HighRefresh => {
                        let modes: Vec<(i32, i32, f64)> = output
                            .available_modes
                            .iter()
                            .map(|mode| (mode.width, mode.height, mode.refresh_rate))
                            .collect();
                        ModeChoice::highest_refresh(&modes).map(|(width, height, hz)| {
                            OutputResolution::Exact {
                                width: width as u16,
                                height: height as u16,
                                refresh_rate: Some(hz),
                            }
                        })
                    }
                };
                if let Some([x, y]) = entry.and_then(|entry| entry.position) {
                    config.position = Some(OutputPosition::Exact { x, y });
                }
                if let Some(source) = over.mirror {
                    config.mode = Some(OutputMode::Mirror);
                    config.source = Some(source);
                }
                if let Some(color) = over.color {
                    config.hdr = Some(OutputHdr::Enabled(color.cm != "srgb"));
                }
                (output.name.clone(), Some(config))
            })
            .collect()
    });

    COMPOSITOR.input.configure(|config, devices| {
        let settings = engine::settings();
        let input = &settings.input;
        config.global = Some(RuntimeInputDeviceConfig {
            keyboard: Some(RuntimeKeyboardInputConfig {
                repeat_rate: input.keyboard.repeat_rate,
                repeat_delay: input.keyboard.repeat_delay,
                layout: input.keyboard.layout.clone(),
                variant: input.keyboard.variant.clone(),
                options: input.keyboard.options.clone(),
                ..Default::default()
            }),
            pointer: Some(pointer(&input.pointer)),
            touchpad: Some(touchpad(&input.touchpad)),
        });
        for (key, device) in devices {
            if let Some(own) = input.devices.get(&device_name(&device.name)) {
                // A touchpad listed by name takes the same three values.
                config.device.insert(
                    key.clone(),
                    Some(RuntimeInputDeviceConfig {
                        keyboard: None,
                        pointer: Some(pointer(own)),
                        touchpad: Some(RuntimeTouchpadInputConfig {
                            pointer_accel: own.sensitivity,
                            accel_profile: accel(own.accel),
                            natural_scroll: own.natural_scroll,
                            ..touchpad(&input.touchpad)
                        }),
                    }),
                );
            }
        }
    });

    COMPOSITOR.event.on_output_change(|change| {
        let added = !change.added.is_empty();
        enter(move |engine| {
            engine.sync_outputs();
            if added {
                engine.xwayland_primary();
            }
        });
    });
    COMPOSITOR
        .event
        .on_create_layer(|_| enter(|engine| engine.sync_outputs()));
    COMPOSITOR
        .event
        .on_update_layer(|_| enter(|engine| engine.sync_outputs()));
    COMPOSITOR
        .event
        .on_destroy_layer(|_| enter(|engine| engine.sync_outputs()));

    COMPOSITOR
        .workspace
        .configure(|| WORKSPACES.with(|model| model.borrow().clone()));
    COMPOSITOR.workspace.on_activate(|request| {
        let id = request.workspace_id.clone();
        enter(move |engine| {
            if engine.model.focus_workspace(&id) {
                engine.commit();
            }
        });
    });

    COMPOSITOR.on_reload(|| engine::ask(Engine::reload));

    COMPOSITOR.on_enable(|_| {
        enter(|engine| {
            engine.sync_outputs();
            engine.xwayland_primary();
            engine.start_shell();
        });
        wait_for_shell(Instant::now());
    });
}

/// Shows the pointer when the shell has not come for it in time. The wall
/// clock decides: the compositor's own has no time yet when the session
/// starts, and a timer set then comes due at its first tick.
fn wait_for_shell(started: Instant) {
    let left = SHELL_WAIT_MS - started.elapsed().as_secs_f64() * 1000.0;
    if left > 0.0 {
        set_timeout(left, move || wait_for_shell(started));
        return;
    }
    enter(|engine| {
        if !engine.shell_arrived {
            engine.cursor_hidden = false;
            engine.apply_cursor();
            engine.publish();
        }
    });
}

/// The core's own control socket takes `quit`; the engine is a client of
/// it like any other. Off the compositor's thread, which is the one that
/// answers.
pub fn exit() {
    let Some(path) = std::env::var_os("SHOJIWM_SOCKET") else {
        tracing::error!("SHOJIWM_SOCKET is not set, cannot ask the compositor to quit");
        return;
    };
    // The core does not tell the config it is quitting, so nothing else
    // would take the JSON socket's file away.
    let _ = std::fs::remove_file(shojiwm_rs::ipc::default_socket_path());
    std::thread::spawn(move || {
        if let Ok(mut socket) = UnixStream::connect(path) {
            let _ = socket.write_all(b"quit\n");
            let _ = socket.read(&mut [0; 16]);
        }
    });
}

impl Engine {
    /// The model's outputs from the compositor's: their logical boxes and
    /// what the layers leave free of them.
    pub fn sync_outputs(&mut self) {
        let outputs: Vec<WaylandOutputSnapshot> = COMPOSITOR
            .output
            .state()
            .get_untracked()
            .into_values()
            .filter(|output| output.enabled)
            .collect();
        let connected: Vec<OutputFacts<'_>> = outputs.iter().map(facts).collect();
        let specs = self.settings.workspace_specs(&connected);
        let geos: Vec<OutputGeo> = outputs
            .iter()
            .filter_map(|output| {
                let rect = COMPOSITOR.output.logical_rect(&output.name)?;
                let usable = untrack(|| COMPOSITOR.layer.usable_area(&output.name)).unwrap_or(rect);
                let boxed = |rect: Rect| Box2::new(rect.x, rect.y, rect.width, rect.height);
                Some(OutputGeo {
                    name: output.name.clone(),
                    rect: boxed(rect),
                    usable: boxed(usable),
                    scale: output.scale,
                    default_workspace: self
                        .settings
                        .output_for(&facts(output))
                        .and_then(|entry| entry.default_workspace)
                        .map(|number| number.to_string()),
                })
            })
            .collect();
        let same = geos.len() == self.model.outputs.len()
            && geos
                .iter()
                .zip(&self.model.outputs)
                .all(|(geo, output)| geo == &output.geo);
        if same && specs == self.specs {
            return;
        }
        self.model.set_workspaces(&specs);
        self.model.set_outputs(geos);
        self.specs = specs;
        self.commit();
    }

    /// The ext-workspace model bars read: one group per output.
    pub fn publish_workspaces(&self) {
        let model = RuntimeWorkspaceConfigUpdate {
            groups: self
                .model
                .outputs
                .iter()
                .map(|output| RuntimeWorkspaceGroupConfig {
                    id: output.geo.name.clone(),
                    outputs: vec![output.geo.name.clone()],
                    workspaces: self
                        .model
                        .stack(&output.geo.name)
                        .iter()
                        .enumerate()
                        .map(|(index, ws)| RuntimeWorkspaceEntry {
                            id: ws.id.clone(),
                            name: ws.name.clone(),
                            coordinates: vec![index as u32],
                            active: output.active == ws.id,
                            urgent: ws
                                .windows()
                                .any(|id| self.model.window(id).is_some_and(|w| w.urgent)),
                            hidden: false,
                        })
                        .collect(),
                })
                .collect(),
            ..Default::default()
        };
        WORKSPACES.with(|cell| *cell.borrow_mut() = model);
        COMPOSITOR.workspace.reconfigure();
    }

    pub fn set_theme(&mut self, theme: proto::Theme) {
        if theme == self.theme {
            return;
        }
        self.theme = theme;
        self.apply_theme();
        self.commit();
    }

    fn apply_theme(&mut self) {
        let chrome = &self.theme.chrome;
        self.signals.look.set(look_of(&self.theme));
        self.scene.set_motion(&chrome.motion);
        self.model.set_options(
            self.settings
                .options(chrome.gaps_in as f64, chrome.gaps_out as f64),
        );
        layers::install(self.settings.clone(), chrome.blur);
        for win in self.windows.values() {
            win.handle
                .set_close_animation_duration(self.scene.close_ms() as u64);
            win.handle
                .state(&crate::windows::BLUR)
                .set(self.blurs(&win.effect));
        }
    }

    /// Hyprland blurs behind what is not opaque: a window the settings or
    /// its rule make translucent, unless the rule says `no_blur`.
    pub fn blurs(&self, effect: &formalwm_core::rules::Effect) -> bool {
        self.theme.chrome.blur
            && !effect.no_blur.unwrap_or(false)
            && effect.opacity.unwrap_or(self.settings.opacity) < 1.0
    }

    /// The settings that do not come through a factory.
    pub fn apply_settings(&mut self) {
        let settings = self.settings.clone();
        engine::set_settings(settings.clone());
        COMPOSITOR.output.reconfigure();
        COMPOSITOR.input.reconfigure();
        COMPOSITOR.env.apply(settings.env.clone());
        COMPOSITOR
            .cursor
            .configure(&settings.cursor.theme, settings.cursor.size);
        self.bind_keys();
        self.apply_theme();
        self.watch_lid();
    }

    /// Reads the settings again. A file with a mistake changes nothing.
    fn reload(&mut self) -> Result<(), String> {
        match Settings::load(&self.dir) {
            Err(error) => {
                tracing::error!(%error, "the settings were refused");
                self.broadcast(event::CONFIG_FAILED, json!({ "error": error }));
                Err(error)
            }
            Ok(settings) => {
                self.settings = Rc::new(settings);
                self.apply_settings();
                self.sync_outputs();
                self.commit();
                self.broadcast(event::CONFIG_RELOADED, json!({}));
                // The compositor drops the cursor override when it swaps a
                // config; this runs after it has.
                set_timeout(1.0, || enter(|engine| engine.apply_cursor()));
                Ok(())
            }
        }
    }

    pub fn apply_cursor(&mut self) {
        let hidden = self.cursor_hidden || self.cursor_idle;
        if let Err(error) = COMPOSITOR.cursor.override_kind(hidden.then_some("none")) {
            tracing::warn!(%error, "the cursor could not be hidden");
        }
    }

    /// The pointer moved or the focus changed: over a window whose rule
    /// hides an idle pointer, the cursor goes after that long without
    /// motion.
    pub fn arm_idle_pointer(&mut self) {
        if let Some(timer) = self.idle_timer.take() {
            timer.cancel();
        }
        if std::mem::take(&mut self.cursor_idle) {
            self.apply_cursor();
        }
        let seconds = self
            .model
            .focused_window()
            .and_then(|id| self.windows.get(id))
            .and_then(|win| win.effect.idle_pointer);
        if let Some(seconds) = seconds {
            self.idle_timer = Some(set_timeout(seconds * 1000.0, || {
                enter(|engine| {
                    engine.idle_timer = None;
                    engine.cursor_idle = true;
                    engine.apply_cursor();
                })
            }));
        }
    }

    /// XWayland has no primary output until told; Proton games open on
    /// whichever it lists first. The server may not be up yet, so this
    /// tries for a while.
    fn xwayland_primary(&mut self) {
        let Some(output) = &self.settings.xwayland_primary else {
            return;
        };
        COMPOSITOR.process.spawn(Command::shell(format!(
            "for _ in 1 2 3 4 5 6 7 8 9 10; do xrandr --output '{output}' --primary 2>/dev/null && break; sleep 1; done"
        )));
    }

    /// The shell's installer writes the script that brings the shell up in
    /// this session; where there is none, nothing starts.
    fn start_shell(&mut self) {
        let script = self.dir.join("formalshell-start.sh");
        if script.is_file() {
            COMPOSITOR.process.spawn(Command::exec([
                "sh".to_owned(),
                script.to_string_lossy().into_owned(),
            ]));
        }
    }
}
