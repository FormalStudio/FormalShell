//! The JSON socket: every method of `fs-wm-proto`, the state broadcast once
//! per turn and only when it differs from the last one sent.

use formalwm_core::{color::Rgba, geom::Rect as Box2, view};
use fs_wm_proto::{self as proto, event, method};
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use shojiwm_rs::{
    ipc::IpcServer,
    prelude::*,
    ssd::{OutputTransformSnapshot, window_model::LayerKindSnapshot},
};

use crate::{
    engine::{Engine, ask, enter},
    session,
};

fn params<T: DeserializeOwned>(value: &Value) -> Result<T, String> {
    serde_json::from_value(value.clone()).map_err(|error| error.to_string())
}

/// A method that changes the model: `f` says whether it did.
fn command<T: DeserializeOwned + 'static>(
    server: &IpcServer,
    name: &str,
    f: impl Fn(&mut Engine, T) -> Result<bool, String> + 'static,
) {
    server.handle(name, move |value| {
        let request = params::<T>(value)?;
        ask(|engine| {
            if f(engine, request)? {
                engine.commit();
            }
            Ok(Value::Null)
        })
    });
}

pub fn serve() -> Option<IpcServer> {
    let server = IpcServer::new()
        .inspect_err(|error| tracing::error!(%error, "the JSON socket could not be bound"))
        .ok()?;

    server.handle(method::STATE_GET, |_| {
        ask(|engine| serde_json::to_value(engine.state()).map_err(|error| error.to_string()))
    });
    command(
        &server,
        method::WORKSPACE_FOCUS,
        |engine, request: proto::WorkspaceFocus| Ok(engine.model.focus_workspace(&request.id)),
    );
    command(
        &server,
        method::WORKSPACE_FOCUS_AT,
        |engine, request: proto::WorkspaceFocusAt| {
            Ok(engine
                .model
                .focus_workspace_at(request.idx, request.output.as_deref()))
        },
    );
    command(
        &server,
        method::WINDOW_FOCUS,
        |engine, request: proto::WindowRef| {
            let id = engine.resolve(&request.id)?;
            Ok(engine.model.focus_window(&id))
        },
    );
    command(
        &server,
        method::WINDOW_CLOSE,
        |engine, request: proto::WindowRef| {
            let id = engine.resolve(&request.id)?;
            if let Some(win) = engine.windows.get(&id) {
                win.handle.close();
            }
            Ok(false)
        },
    );
    command(
        &server,
        method::WINDOW_FLOAT,
        |engine, request: proto::WindowFloat| {
            let id = engine.resolve(&request.id)?;
            Ok(engine.set_floating(&id, request.floating))
        },
    );
    command(
        &server,
        method::WINDOW_FULLSCREEN,
        |engine, request: proto::WindowRef| {
            let id = engine.resolve(&request.id)?;
            engine.toggle_fullscreen(&id);
            Ok(false)
        },
    );
    command(
        &server,
        method::WINDOW_MOVE_TO,
        |engine, request: proto::WindowMoveTo| {
            let id = engine.resolve(&request.id)?;
            let workspace = usize::try_from(request.idx - 1)
                .ok()
                .and_then(|index| {
                    engine
                        .model
                        .stack(&request.output)
                        .get(index)
                        .map(|ws| ws.id.clone())
                })
                .ok_or_else(|| format!("no workspace {} on {}", request.idx, request.output))?;
            Ok(engine.model.move_to_workspace(&id, &workspace, false))
        },
    );
    command(
        &server,
        method::WINDOW_SWAP,
        |engine, request: proto::WindowSwap| {
            let (a, b) = (engine.resolve(&request.a)?, engine.resolve(&request.b)?);
            Ok(engine.model.swap(&a, &b))
        },
    );
    command(
        &server,
        method::WINDOW_MOVE_COLUMN,
        |engine, request: proto::WindowMoveColumn| {
            let id = engine.resolve(&request.id)?;
            let before = request
                .before
                .map(|before| engine.resolve(&before))
                .transpose()?;
            let moved = engine.model.move_column(&id, before.as_deref());
            Ok(engine.model.focus_window(&id) || moved)
        },
    );
    command(
        &server,
        method::WINDOW_PLACE,
        |engine, request: proto::WindowPlace| {
            let id = engine.resolve(&request.id)?;
            let rect = Box2::new(request.x, request.y, request.width, request.height);
            Ok(engine.model.place(&id, rect))
        },
    );
    command(
        &server,
        method::OVERLAY_PARK,
        |engine, request: proto::OverlayPark| {
            let id = engine.resolve(&request.id)?;
            Ok(engine.model.park(&id, &request.overlay))
        },
    );
    command(
        &server,
        method::OVERLAY_SHOW,
        |engine, request: proto::WindowRef| {
            let id = engine.resolve(&request.id)?;
            Ok(engine.model.show_overlay(&id))
        },
    );
    server.handle(method::ZEN_SET, |value| {
        // `zen.set` may come with no params at all.
        let request = if value.is_null() {
            proto::ZenSet::default()
        } else {
            params::<proto::ZenSet>(value)?
        };
        ask(|engine| {
            let id = request.id.map(|id| engine.resolve(&id)).transpose()?;
            if engine.model.set_zen(id.as_deref(), request.on) {
                engine.commit();
            }
            Ok(Value::Null)
        })
    });
    server.handle(method::OVERVIEW_SET, |value| {
        let request = if value.is_null() {
            proto::OverviewSet::default()
        } else {
            params::<proto::OverviewSet>(value)?
        };
        ask(|engine| {
            engine.set_overview(request.open);
            Ok(Value::Null)
        })
    });
    server.handle(method::SPAWN, |value| {
        let request = params::<proto::Spawn>(value)?;
        if request.argv.is_empty() {
            return Err("spawn: empty argv".to_owned());
        }
        let command = Command::exec(request.argv);
        COMPOSITOR.process.spawn(match request.cwd {
            Some(cwd) => command.cwd(cwd),
            None => command,
        });
        Ok(Value::Null)
    });
    server.handle(method::BIND_ESCAPE, |value| {
        let request = params::<proto::BindEscape>(value)?;
        ask(|engine| {
            match request.argv {
                Some(argv) if !argv.is_empty() => {
                    engine.escapes.insert(request.name, argv);
                }
                _ => {
                    engine.escapes.remove(&request.name);
                }
            }
            engine.bind_escapes();
            Ok(Value::Null)
        })
    });
    server.handle(method::BINDS_LIST, |_| {
        ask(|engine| serde_json::to_value(engine.binds_list()).map_err(|error| error.to_string()))
    });
    server.handle_from(method::BINDS_HAND_OVER, |client, value| {
        let request = params::<proto::BindsHandOver>(value)?;
        ask(|engine| {
            engine.hand_over(client, request)?;
            Ok(Value::Null)
        })
    });
    server.on_disconnect(|client| enter(move |engine| engine.client_gone(client)));
    server.handle(method::CURSOR_POS, |_| {
        let position = COMPOSITOR.cursor.get_position().ok_or("no pointer")?;
        Ok(json!({ "x": position.x, "y": position.y }))
    });
    server.handle(method::CURSOR_HIDE, |value| {
        let request = params::<proto::CursorHide>(value)?;
        ask(|engine| {
            engine.shell_arrived = true;
            engine.cursor_hidden = request.hidden;
            engine.apply_cursor();
            engine.publish();
            Ok(Value::Null)
        })
    });
    server.handle(method::LAYERS_LIST, |_| {
        serde_json::to_value(layers()).map_err(|error| error.to_string())
    });
    server.handle(method::OUTPUTS_GET, |_| {
        serde_json::to_value(outputs()).map_err(|error| error.to_string())
    });
    server.handle(method::OUTPUTS_SET, |value| {
        let request = params::<proto::OutputSet>(value)?;
        if COMPOSITOR.output.get(&request.name).is_none() {
            return Err(format!("no such output: {}", request.name));
        }
        session::override_output(request)?;
        Ok(Value::Null)
    });
    server.handle(method::OUTPUTS_CREATE, |value| {
        let name = session::create_output(params::<proto::OutputCreate>(value)?)?;
        serde_json::to_value(proto::OutputRef { name }).map_err(|error| error.to_string())
    });
    server.handle(method::OUTPUTS_REMOVE, |value| {
        session::remove_output(&params::<proto::OutputRef>(value)?.name)?;
        Ok(Value::Null)
    });
    server.handle(method::THEME_SET, |value| {
        let theme = params::<proto::Theme>(value)?;
        ask(|engine| {
            engine.set_theme(theme);
            Ok(Value::Null)
        })
    });
    server.handle(method::SESSION_EXIT, |_| {
        session::exit();
        Ok(Value::Null)
    });

    COMPOSITOR.on_disable({
        let server = server.clone();
        move |_| server.close()
    });
    COMPOSITOR
        .event
        .on_keyboard_layout_change(|_| enter(|engine| engine.publish()));
    Some(server)
}

fn transform(transform: OutputTransformSnapshot) -> u8 {
    match transform {
        OutputTransformSnapshot::Normal => 0,
        OutputTransformSnapshot::Rotate90 => 1,
        OutputTransformSnapshot::Rotate180 => 2,
        OutputTransformSnapshot::Rotate270 => 3,
        OutputTransformSnapshot::Flipped => 4,
        OutputTransformSnapshot::Flipped90 => 5,
        OutputTransformSnapshot::Flipped180 => 6,
        OutputTransformSnapshot::Flipped270 => 7,
    }
}

fn layers() -> Vec<proto::Layer> {
    COMPOSITOR
        .layer
        .stacked()
        .into_iter()
        .filter_map(|layer| {
            // The core places a layer within its output.
            let output = COMPOSITOR.output.logical_rect(&layer.output_name)?;
            Some(proto::Layer {
                namespace: layer.namespace.unwrap_or_default(),
                layer: match layer.layer {
                    LayerKindSnapshot::Background => proto::LayerLevel::Background,
                    LayerKindSnapshot::Bottom => proto::LayerLevel::Bottom,
                    LayerKindSnapshot::Top => proto::LayerLevel::Top,
                    LayerKindSnapshot::Overlay => proto::LayerLevel::Overlay,
                },
                x: output.x.round() as i32 + layer.position.x,
                y: output.y.round() as i32 + layer.position.y,
                width: layer.position.width,
                height: layer.position.height,
                pid: layer.pid.map_or(0, i64::from),
                output: layer.output_name,
            })
        })
        .collect()
}

fn outputs() -> Vec<proto::Output> {
    COMPOSITOR
        .output
        .state()
        .get_untracked()
        .into_values()
        .map(|output| {
            let mode = output.resolution.filter(|_| output.enabled);
            let over = session::override_of(&output.name);
            proto::Output {
                make: output.make.clone().unwrap_or_default(),
                model: output.model.clone().unwrap_or_default(),
                enabled: output.enabled,
                x: if output.enabled { output.position.x } else { 0 },
                y: if output.enabled { output.position.y } else { 0 },
                width: mode.map_or(0, |mode| mode.width),
                height: mode.map_or(0, |mode| mode.height),
                refresh: mode.map_or(0.0, |mode| mode.refresh_rate),
                scale: output.scale,
                transform: transform(output.transform),
                mirror_of: over.mirror.clone().unwrap_or_default(),
                vrr: output.enabled && session::wants_vrr(&output),
                // An output with no connector lists no modes; it has the one.
                modes: output
                    .available_modes
                    .iter()
                    .chain(mode.iter().filter(|_| output.available_modes.is_empty()))
                    .map(|mode| proto::Mode {
                        width: mode.width,
                        height: mode.height,
                        refresh: mode.refresh_rate,
                    })
                    .collect(),
                color: over.color.clone().unwrap_or_default(),
                name: output.name,
            }
        })
        .collect()
}

impl Engine {
    pub fn state(&self) -> proto::State {
        let chrome = &self.theme.chrome;
        let layouts: Vec<String> = self
            .settings
            .input
            .keyboard
            .layout
            .as_deref()
            .unwrap_or("us")
            .split(',')
            .map(|layout| layout.trim().to_owned())
            .collect();
        let active = COMPOSITOR
            .input
            .keyboard_layout()
            .get_untracked()
            .map_or(0, |layout| layout.index as usize)
            .min(layouts.len().saturating_sub(1));
        let mut state = view::state(
            &self.model,
            &view::Extras {
                overview: self.overview.open,
                keyboard_layouts: layouts,
                keyboard_active: active,
                cursor_hidden: self.cursor_hidden,
                active_border: Rgba::resolve(&chrome.border_color, &self.theme.colors)
                    .unwrap_or(Rgba::BLACK)
                    .hex(),
                blur: chrome.blur,
                motion: chrome.motion,
            },
        );
        for window in &mut state.windows {
            if let Some(win) = self.windows.get(&window.id) {
                window.tag = win.handle.tag().get_untracked().unwrap_or_default();
            }
            window.id = self.public(&window.id);
        }
        state.hand_over = self.hand_over_state();
        if !state.focused_window_id.is_empty() {
            state.focused_window_id = self.public(&state.focused_window_id);
        }
        for workspace in &mut state.workspaces {
            workspace.zen = workspace.zen.take().map(|id| self.public(&id));
        }
        for output in COMPOSITOR.output.state().get_untracked().into_values() {
            if !output.enabled && !state.outputs.iter().any(|known| known.name == output.name) {
                state.outputs.push(proto::StateOutput {
                    name: output.name,
                    scale: output.scale,
                    ..Default::default()
                });
            }
        }
        state
    }

    /// Asks for the state to be broadcast when this turn is over.
    pub fn publish(&mut self) {
        if self.state_pending || self.ipc.is_none() {
            return;
        }
        self.state_pending = true;
        set_timeout(1.0, || {
            enter(|engine| {
                engine.state_pending = false;
                engine.flush();
            })
        });
    }

    fn flush(&mut self) {
        let Some(ipc) = &self.ipc else {
            return;
        };
        let Ok(state) = serde_json::to_value(self.state()) else {
            return;
        };
        let text = state.to_string();
        if text == self.last_state {
            return;
        }
        ipc.broadcast(event::STATE_CHANGED, state);
        self.last_state = text;
    }

    pub fn broadcast(&self, event: &str, payload: Value) {
        if let Some(ipc) = &self.ipc {
            ipc.broadcast(event, payload);
        }
    }
}
