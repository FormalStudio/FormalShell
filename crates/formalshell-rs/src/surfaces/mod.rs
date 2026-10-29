//! Each surface reads its slices of the store; [`changed`] routes a store
//! change to the surfaces that read that slice.

pub mod atlas;
pub mod bar;
pub mod battery;
pub mod capture;
pub mod card;
pub mod headset;
pub mod launcher;
pub mod lock;
pub mod login;
pub mod modal;
pub mod osd;
pub mod panel;
pub mod popup;
pub mod shoulders;
pub mod switcher;
pub mod tray_menu;
pub mod toasts;
pub mod tooltip;

use crate::services::{lights, theme};
use crate::store::Topic;
use crate::wayland::{App, theme_inputs};

pub fn changed(app: &mut App, topic: Topic) {
    app.lock_changed(topic);
    app.desktop_clock_changed(topic);
    if matches!(topic, Topic::Compositor | Topic::State | Topic::Display | Topic::Config) {
        crate::services::display::reconcile(&app.store);
    }
    app.switcher_changed(topic);
    app.atlas_changed(topic);
    app.overlay_changed(topic);
    app.preview_changed(topic);
    match topic {
        Topic::Menu => app.launcher_store_changed(),
        Topic::Config => {
            app.launcher_inputs();
            crate::services::picker::command(crate::services::picker::Cmd::Boot(app.store.config.str("picker.directory").unwrap_or("").to_owned()));
            let c = &app.store.config;
            crate::services::localsend::command(crate::services::localsend::Cmd::Config {
                receive: c.bool("localsend.receive").unwrap_or(false),
                alias: c.str("localsend.alias").unwrap_or("").to_owned(),
                dir: c.str("localsend.dir").unwrap_or("").to_owned(),
            });
            crate::services::depth::command(crate::services::depth::Cmd::Config {
                enabled: c.bool("wallpaper.depthGenerate").unwrap_or(false),
                dir: c.str("picker.directory").unwrap_or("").to_owned(),
            });
            let names = match c.get("compositor.workspaceNames") {
                Some(serde_json::Value::Array(items)) => items.iter().map(|v| v.as_str().unwrap_or_default().to_owned()).collect(),
                _ => Vec::new(),
            };
            crate::services::compositor::send(crate::services::compositor::Command::WorkspaceNames(names));
            let settings = app.store.config.settings().clone();
            if app.store.theme.apply(theme::Diff::Settings(settings)) {
                changed(app, Topic::Theme);
            }
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
            app.launcher_store_changed();
            theme_inputs(app);
            lights::record(app.store.state.data.lights.clone());
            app.refresh_bar(Some(topic));
        }
        Topic::Plugins => app.apply_config(),
        Topic::Theme => {
            app.set_bar_theme();
            app.restyle_cards();
            lights_palette(app);
        }
        Topic::Devices => {
            if app.battery_watch.check(&mut app.store) {
                changed(app, Topic::Notifications);
            }
            app.osd_audio();
            app.headset_devices();
            app.launcher_devices();
            app.refresh_bar(Some(topic));
        }
        Topic::Capture => capture::events(app),
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
        Topic::Localsend => app.launcher_inputs(),
        // Read over IPC alone until its cell and toast land.
        Topic::Activity => {}
        Topic::Mirror => app.launcher_mirror(),
        Topic::Picker => app.launcher_picker(),
        Topic::Info => {
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
        Topic::Lights | Topic::NightLight => {
            app.launcher_inputs();
            app.launcher_store_changed();
            app.refresh_bar(Some(topic));
        }
        Topic::Media => {
            app.launcher_devices();
            app.saver_update();
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
