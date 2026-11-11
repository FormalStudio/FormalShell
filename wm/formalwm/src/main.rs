//! formalwm: the window manager on ShojiWM's compositor. The model, the
//! scene and the settings are `formalwm-core`; this crate is their hands.

mod binds;
mod drag;
mod engine;
mod gestures;
#[cfg(test)]
mod headless;
mod ipc;
mod layers;
mod overview;
mod session;
mod stage;
mod trace;
mod windows;

use std::{collections::BTreeMap, rc::Rc};

use formalwm_core::{model::Model, scene::Scene, settings::Settings};
use fs_wm_proto as proto;
use shojiwm_rs::prelude::*;

use engine::{Engine, Signals, enter};

fn setup() {
    let dir = session::settings_dir();
    // A session has to start whatever the file says; the mistake is logged
    // and the next reload reports it to the shell.
    let settings = Settings::load(&dir).unwrap_or_else(|error| {
        tracing::error!(%error, "the settings were refused, starting on the defaults");
        Settings::default()
    });
    let theme = proto::Theme::default();
    let signals = Signals {
        look: signal(engine::look_of(&theme)),
        stage: signal(stage::Stage::default()),
    };
    let options = settings.options(theme.chrome.gaps_in as f64, theme.chrome.gaps_out as f64);
    engine::install(Engine {
        dir,
        settings: Rc::new(settings),
        specs: Vec::new(),
        model: Model::new(options, &[]),
        scene: Scene::new(&theme.chrome.motion),
        theme,
        windows: BTreeMap::new(),
        signals,
        ipc: None,
        driver: None,
        last_tick: None,
        state_pending: false,
        last_state: String::new(),
        asked_focus: None,
        bind_ids: Vec::new(),
        escapes: BTreeMap::new(),
        cursor_hidden: true,
        shell_arrived: false,
        cursor_idle: false,
        idle_timer: None,
        gesture: Default::default(),
        overview: Default::default(),
        drag: None,
        hold: Default::default(),
    });

    windows::setup(signals);
    stage::setup(signals);
    session::setup();
    gestures::setup();
    drag::setup();
    let ipc = ipc::serve();
    enter(move |engine| {
        engine.ipc = ipc;
        engine.apply_settings();
        engine.apply_cursor();
        engine.sync_outputs();
    });
}

fn main() -> std::process::ExitCode {
    ConfigBuilder::new(setup)
        .name("formalwm")
        .asset_root(layers::install_assets())
        .start_locked(session::starts_locked())
        .run()
}
