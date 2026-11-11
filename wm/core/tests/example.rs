//! The owner's settings in `wm/example` are the port of `hyprland.lua` and
//! g815's `host.lua`; they have to stay loadable.

use formalwm_core::{
    action::{Action, WorkspaceRef},
    rules::{Facts, resolve},
    settings::{ModeChoice, OutputFacts, Settings},
};

fn example(host: Option<&str>) -> Settings {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../example");
    let main = std::fs::read_to_string(format!("{dir}/settings.json")).unwrap();
    let host =
        host.map(|host| std::fs::read_to_string(format!("{dir}/hosts/{host}/host.json")).unwrap());
    Settings::parse(&main, host.as_deref()).unwrap()
}

#[test]
fn the_shared_file_loads_on_its_own() {
    let settings = example(None);
    assert_eq!(settings.workspaces.len(), 9);
    assert!(settings.workspaces[0].ends_with("web"));
    assert_eq!(settings.binds.len(), 73);
    assert_eq!(settings.rules.len(), 20);
    assert_eq!(settings.layers.len(), 11);
    assert_eq!(settings.opacity, 1.0);
    assert_eq!(settings.input.keyboard.layout.as_deref(), Some("es"));
    let laptop = OutputFacts {
        name: "eDP-1",
        ..OutputFacts::default()
    };
    assert_eq!(settings.output_for(&laptop).unwrap().scale, Some(1.0));
    let other = OutputFacts {
        name: "DP-2",
        ..OutputFacts::default()
    };
    assert!(settings.output_for(&other).is_some());
}

#[test]
fn the_number_row_is_bound_by_keycode() {
    let settings = example(None);
    for number in 1..=9 {
        let chord = format!("SUPER+code:{}", number + 9);
        let bind = settings
            .binds
            .iter()
            .find(|bind| bind.chord.canonical() == chord)
            .unwrap();
        assert_eq!(bind.action, Action::Workspace(WorkspaceRef::Number(number)));
        let chord = format!("SUPER+SHIFT+code:{}", number + 9);
        let bind = settings
            .binds
            .iter()
            .find(|bind| bind.chord.canonical() == chord)
            .unwrap();
        assert_eq!(bind.action, Action::MoveToWorkspace(number));
    }
}

#[test]
fn the_rules_place_the_apps_they_name() {
    let settings = example(None);
    let sum = |app_id: &str, title: &str, floating: bool| {
        resolve(
            &settings.rules,
            &Facts {
                app_id,
                title,
                floating,
                ..Facts::default()
            },
        )
    };
    assert_eq!(sum("helium", "Page", false).workspace, Some(1));
    assert_eq!(sum("helium", "Page", false).pin, None);
    assert_eq!(sum("helium", "Meet", true).pin, Some(true));
    assert_eq!(sum("Beeper", "Beeper", true).float, Some(false));
    assert_eq!(sum("Beeper", "Beeper", true).workspace, Some(4));
    assert_eq!(sum("", "Messages", false).workspace, Some(4));
    assert_eq!(
        sum("dev.formalshell.console", "", false).overlay.as_deref(),
        Some("formalshell-console")
    );
    assert_eq!(
        sum("steam_app_730", "CS2", false).content.as_deref(),
        Some("game")
    );
    assert_eq!(sum("steam_app_730", "CS2", false).idle_pointer, Some(3.0));
    assert_eq!(sum("", "Picture in picture", false).pin, Some(true));
    assert_eq!(sum("", "", false).float, Some(true));
    assert_eq!(sum("ghostty", "~", false).workspace, None);
    assert!(settings.layer_rule("formalshell:menu").unwrap().blur);
    assert!(!settings.layer_rule("formalshell:veil").unwrap().animate);
}

#[test]
fn g815_lays_its_outputs_over_the_shared_file() {
    let settings = example(Some("g815"));
    assert_eq!(settings.opacity, 0.96);
    assert!(!settings.direct_scanout);
    assert_eq!(settings.env["LIBVA_DRIVER_NAME"], "nvidia");
    assert_eq!(
        settings.env["QT_QPA_PLATFORMTHEME"], "qt6ct",
        "the shared env stays"
    );
    assert_eq!(settings.xwayland_primary.as_deref(), Some("HDMI-A-1"));
    let desk = OutputFacts {
        name: "DP-1",
        make: "ASUSTek COMPUTER INC",
        model: "PA278CGV",
        serial: "T1LMQS063802",
        ..OutputFacts::default()
    };
    let portable = OutputFacts {
        name: "DP-2",
        make: "ASUSTek COMPUTER INC",
        model: "MB16QHG",
        serial: "RCLMTF037964",
        ..OutputFacts::default()
    };
    let panel = OutputFacts {
        name: "eDP-1",
        make: "BOE",
        model: "NE180QDM-NZC",
        ..OutputFacts::default()
    };
    assert_eq!(
        settings.output_for(&desk).unwrap().position,
        Some([2560, 0])
    );
    assert_eq!(settings.output_for(&portable).unwrap().scale, Some(1.6));
    let beamer = OutputFacts {
        name: "HDMI-A-2",
        ..OutputFacts::default()
    };
    assert_eq!(
        settings.output_for(&beamer).unwrap().mode(),
        ModeChoice::HighRefresh
    );
    assert_eq!(settings.output_for(&panel).unwrap().vrr, 3);
    assert!(settings.is_lid_output(&panel));
    assert!(!settings.is_lid_output(&desk));
    let specs = settings.workspace_specs(&[desk, portable, panel]);
    let pins: Vec<Option<&str>> = specs.iter().map(|spec| spec.pin.as_deref()).collect();
    assert_eq!(pins[0], Some("DP-1"));
    assert_eq!(pins[3], Some("DP-2"));
    assert_eq!(pins[7], Some("DP-2"));
    assert_eq!(pins[8], Some("DP-1"));
}
