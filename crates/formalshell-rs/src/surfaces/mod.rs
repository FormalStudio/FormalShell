//! Each surface reads its slices of the store; [`changed`] routes a store
//! change to the surfaces that read that slice.

pub mod activity;
pub mod atlas;
pub mod bar;
pub mod battery;
pub mod capture;
pub mod card;
pub mod card_window;
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

use crate::services::lights;
use crate::store::Topic;
use crate::wayland::{App, theme_inputs};

pub fn changed(app: &mut App, topic: Topic) {
    app.lock_changed(topic);
    app.desktop_clock_changed(topic);
    if matches!(topic, Topic::Compositor | Topic::State | Topic::Display | Topic::Config) {
        app.hdr.reconcile(&app.store);
    }
    app.switcher_changed(topic);
    app.overview_changed(topic);
    if topic == Topic::Compositor {
        app.zen_changed();
    }
    app.atlas_changed(topic);
    app.overlay_changed(topic);
    app.preview_changed(topic);
    match topic {
        Topic::Menu => {
            packages::land(&mut app.store);
            app.launcher_store_changed();
        }
        Topic::Packages => {
            app.launcher.owner_answer(&app.store);
            app.launcher_store_changed();
            app.refresh_bar(Some(topic));
        }
        Topic::Config => {
            app.launcher_inputs();
            use crate::settings::{DEPTH_GENERATE, LOCALSEND_ALIAS, LOCALSEND_DIR, LOCALSEND_RECEIVE, PICKER_DIRECTORY};
            let c = &app.store.config;
            crate::services::picker::command(crate::services::picker::Cmd::Boot(PICKER_DIRECTORY.get(c).to_owned()));
            crate::services::localsend::command(crate::services::localsend::Cmd::Config {
                receive: LOCALSEND_RECEIVE.get(c),
                alias: LOCALSEND_ALIAS.get(c).to_owned(),
                dir: LOCALSEND_DIR.get(c).to_owned(),
            });
            crate::services::depth::command(crate::services::depth::Cmd::Config { enabled: DEPTH_GENERATE.get(c), dir: PICKER_DIRECTORY.get(c).to_owned() });
            app.apply_config();
            theme_inputs(app);
            lights_palette(app);
            app.saver_config();
        }
        Topic::Screensaver => app.saver_changed(),
        Topic::Caffeinate => {
            app.hold_inhibitor();
            app.saver_idle_changed();
            app.refresh_bar(Some(topic));
        }
        Topic::State => {
            app.launcher_inputs();
            // The picker's Style previews show the slots.
            app.launcher_picker();
            theme_inputs(app);
            lights::record(app.store.state.data.lights.clone());
            app.refresh_bar(Some(topic));
        }
        Topic::Plugins => {
            app.apply_config();
            app.launcher_inputs();
            app.launcher_plugins();
        }
        Topic::PluginOutput => {
            app.launcher_plugins();
            app.refresh_bar(Some(topic));
        }
        Topic::Theme => {
            app.set_bar_theme();
            app.restyle_cards();
            app.toasts_changed();
            lights_palette(app);
            // A sky step the engine published moves the desktop's source.
            theme_inputs(app);
        }
        Topic::Bluetooth
        | Topic::Network
        | Topic::Audio
        | Topic::Power
        | Topic::Earbuds
        | Topic::PadLights
        | Topic::Profiles
        | Topic::Revealed
        | Topic::Logind => {
            if app.battery_watch.check(&mut app.store) {
                changed(app, Topic::Notifications);
            }
            app.osd_audio();
            app.headset_devices();
            app.launcher_devices();
            app.refresh_bar(Some(topic));
        }
        Topic::Capture => capture::events(app),
        Topic::Launch => {}
        Topic::Tray => {
            app.launcher_tray();
            app.tray_changed();
            app.refresh_bar(Some(topic));
        }
        Topic::Notifications => {
            app.toasts_changed();
            app.launcher_store_changed();
            app.refresh_bar(Some(topic));
        }
        Topic::Clipboard => {
            app.field_paste();
            app.launcher_clipboard();
            app.launcher_inputs();
        }
        Topic::Localsend => {
            app.launcher_inputs();
            app.launcher_store_changed();
        }
        Topic::Activity => {
            if packages::land(&mut app.store) || app.launcher.level.as_deref() == Some(fs_menu::providers::INSTALL_ROUTE) {
                app.launcher_store_changed();
            }
            app.toasts_changed();
            app.refresh_bar(Some(topic));
        }
        Topic::Mirror => app.launcher_mirror(),
        Topic::Picker => app.launcher_picker(),
        Topic::Weather
        | Topic::Github
        | Topic::Usage
        | Topic::SystemUpdate
        | Topic::Tailscale
        | Topic::Monitor
        | Topic::Iphone
        | Topic::Calendar
        | Topic::Procs
        | Topic::PowerFlow
        | Topic::About => {
            if app.launcher.open && app.launcher.level.as_deref() == Some(crate::surfaces::launcher::MONITOR_ROUTE) {
                app.launcher.monitor_sample(&app.store);
                app.launcher_store_changed();
            }
            app.refresh_bar(Some(topic));
        }
        Topic::Clipssh => {
            app.launcher_clipssh();
            app.refresh_bar(Some(topic));
        }
        Topic::Lights | Topic::NightLight | Topic::Ipad => {
            app.launcher_inputs();
            app.launcher_store_changed();
            app.refresh_bar(Some(topic));
        }
        Topic::Media => {
            app.launcher_devices();
            app.saver_update();
            app.refresh_bar(Some(topic));
        }
        Topic::Display => {
            let scales = &app.store.display.scales;
            crate::services::menu::ask(crate::services::menu::Ask::Scale(scales.primary()));
            crate::services::tray::scale(scales.largest());
            crate::services::notifications::scale(scales.largest());
            app.refresh_bar(Some(topic));
        }
        _ => app.refresh_bar(Some(topic)),
    }
}

/// The theme's primary as the keyboard lights' wallpaper colour.
fn lights_palette(app: &App) {
    let c = app.store.theme.theme.colors.get("primary");
    lights::palette(fs_system::lights::hex_from_rgb(f64::from(c.r) * 255.0, f64::from(c.g) * 255.0, f64::from(c.b) * 255.0));
}
