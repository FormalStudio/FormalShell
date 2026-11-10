//! Each surface reads its slices of the store; [`changed`] runs a store
//! change's side effects and hands it to the window owners reading it.

pub mod activity;
pub mod atlas;
pub mod bar;
pub mod capture;
pub mod card;
pub mod card_window;
pub mod desktop_clock;
pub mod dissolve;
pub mod headset;
pub mod launcher;
pub mod lock;
pub mod login;
pub mod modal;
pub mod osd;
pub mod packages;
pub mod panel;
pub mod popup;
pub mod sidebar;
pub mod overview;
pub mod switcher;
pub mod tray_menu;
pub mod toasts;
pub mod tooltip;
pub mod veil;

use fs_theme::palette::ColorRole;
use crate::services::lights;
use crate::store::Topic;
use crate::wayland::{App, theme_inputs};

/// What a store change asks of the services, then of every window owner
/// that reads its topic (`Owner::reads`).
pub fn changed(app: &mut App, topic: Topic) {
    if matches!(topic, Topic::Compositor | Topic::State | Topic::Display | Topic::Config) {
        app.hdr.reconcile(&app.store);
    }
    match topic {
        Topic::Compositor => app.zen_changed(),
        Topic::Config => {
            use crate::settings::{DEPTH_GENERATE, LOCALSEND_ALIAS, LOCALSEND_DIR, LOCALSEND_RECEIVE, PICKER_DIRECTORY};
            let c = &app.store.config;
            crate::services::picker::command(crate::services::picker::Cmd::Boot(PICKER_DIRECTORY.get(c).to_owned()));
            crate::services::localsend::command(crate::services::localsend::Cmd::Config {
                receive: LOCALSEND_RECEIVE.get(c),
                alias: LOCALSEND_ALIAS.get(c).to_owned(),
                dir: LOCALSEND_DIR.get(c).to_owned(),
            });
            crate::services::depth::command(crate::services::depth::Cmd::Config { enabled: DEPTH_GENERATE.get(c), dir: PICKER_DIRECTORY.get(c).to_owned() });
            theme_inputs(app);
            lights_palette(app);
        }
        Topic::State => {
            theme_inputs(app);
            lights::record(app.store.state.data.lights.clone());
        }
        Topic::Theme => {
            lights_palette(app);
            // A sky step the engine published moves the desktop's source.
            theme_inputs(app);
        }
        Topic::Menu | Topic::Activity => {
            // An fspec change landing moves the launcher's rows, whichever
            // of the two told of it.
            if crate::services::packages::land(&mut app.store) {
                app.launcher_store_changed();
            }
        }
        Topic::Capture => crate::wayland::capture_host::events(app),
        Topic::Clipboard => app.field_paste(),
        Topic::Display => {
            let scales = &app.store.display.scales;
            crate::services::menu::ask(crate::services::menu::Ask::Scale(scales.primary()));
            crate::services::tray::scale(scales.largest());
            crate::services::notifications::scale(scales.largest());
        }
        _ => {}
    }
    app.owners_changed(topic);
}

/// The theme's primary as the keyboard lights' wallpaper colour.
fn lights_palette(app: &App) {
    let c = app.store.theme.theme.colors.get(ColorRole::Primary);
    lights::palette(fs_system::lights::hex_from_rgb(f64::from(c.r) * 255.0, f64::from(c.g) * 255.0, f64::from(c.b) * 255.0));
}
