//! The keyboard layout off `j/devices` at boot and on `configreloaded`, and
//! `activelayout` events applied in place.
//!
//! UNVERIFIED: the `keyboards[].{main,layout,active_keymap}` field names come
//! from the Hyprland wiki, not from its source (src/devices/IKeyboard.cpp would
//! settle it). Until then a wrong guess falls into `unavailable()` and the
//! widget shows NO LAYOUT, which is the honest outcome. Hyprland also reports
//! the two halves in different vocabularies (`layout` is a comma-separated list
//! of xkb codes, "us,de", while `active_keymap` is a human name, "English
//! (US)"), so there is no reliable index mapping between them and
//! `current_idx` stays -1.

use fs_js as js;
use fs_system::keyboard::{Layout, unavailable};
use serde_json::Value;

/// `j/devices`, verbatim. Reads the keyboard flagged `main`, falling back to
/// the first one listed. Hyprland applies layout switches to the main
/// keyboard, and a session with no keyboard at all has no layout to report,
/// which is `unavailable()` rather than a fabricated one.
pub fn parse(text: &str) -> Layout {
    let raw = js::trim(text);
    if raw.is_empty() {
        return unavailable();
    }
    let Ok(data) = serde_json::from_str::<Value>(raw) else {
        return unavailable();
    };
    let Some(keyboards) = data.get("keyboards").and_then(Value::as_array).filter(|k| !k.is_empty()) else {
        return unavailable();
    };

    let empty = Value::Object(serde_json::Map::new());
    let main = keyboards
        .iter()
        .find(|k| k.get("main") == Some(&Value::Bool(true)))
        .unwrap_or_else(|| if keyboards[0].is_null() { &empty } else { &keyboards[0] });

    let names: Vec<String> = js::str_or_empty(main.get("layout"))
        .split(',')
        .map(|s| js::trim(s).to_string())
        .filter(|s| !s.is_empty())
        .collect();
    let current = js::str_or_empty(main.get("active_keymap"));
    if names.is_empty() && current.is_empty() {
        return unavailable();
    }
    Layout { available: true, names, current_idx: -1, current }
}

/// Applies an `activelayout` event (`data` = "KEYBOARDNAME,LAYOUTNAME") to an
/// already-parsed layout, returning a new value with `current` set from the
/// text after the first comma. The keyboard name itself never contains a comma,
/// so the first one is the split point. `names`/`current_idx` pass through
/// unchanged (see the module docs on why the two vocabularies don't map). A
/// layout that isn't `available`, or data with no comma, is returned as-is:
/// there's nothing parsed yet to update.
pub fn apply_active_layout(layout: &Layout, data: &str) -> Layout {
    if !layout.available {
        return layout.clone();
    }
    match data.find(',') {
        None => layout.clone(),
        Some(comma) => Layout { current: data[comma + 1..].to_string(), ..layout.clone() },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fs_system::keyboard::{has_choice, tooltip_text};
    use serde_json::json;

    fn fixture() -> String {
        json!({
            "mice": [],
            "keyboards": [
                { "name": "virtual-keyboard", "layout": "us", "active_keymap": "English (US)", "main": false },
                { "name": "at-translated-set-2-keyboard", "layout": "us,de", "active_keymap": "German", "main": true }
            ],
            "tablets": [], "touch": [], "switches": []
        })
        .to_string()
    }

    #[test]
    fn reads_the_main_keyboard_not_the_first_listed() {
        let l = parse(&fixture());
        assert!(l.available);
        assert_eq!(l.names, ["us", "de"]);
        assert_eq!(l.current, "German");
    }

    #[test]
    fn reports_no_index_because_codes_and_keymap_names_do_not_map() {
        assert_eq!(parse(&fixture()).current_idx, -1);
    }

    #[test]
    fn falls_back_to_the_first_keyboard_when_none_is_main() {
        let text = json!({ "keyboards": [{ "name": "kb", "layout": "fr", "active_keymap": "French", "main": false }] }).to_string();
        let l = parse(&text);
        assert!(l.available);
        assert_eq!(l.current, "French");
    }

    #[test]
    fn no_keyboards_or_bad_output_is_unavailable() {
        assert!(!parse(&json!({ "keyboards": [] }).to_string()).available);
        assert!(!parse("").available);
        assert!(!parse("Invalid command").available);
    }

    #[test]
    fn the_cell_reads_the_parsed_layout() {
        assert!(has_choice(&parse(&fixture())));
        let single = json!({ "keyboards": [{ "name": "kb", "layout": "us", "active_keymap": "English (US)", "main": true }] }).to_string();
        assert!(!has_choice(&parse(&single)));
        assert_eq!(tooltip_text(Some(&parse(&fixture()))), "GERMAN / US / DE");
    }

    #[test]
    fn apply_active_layout_sets_current_from_the_event_data() {
        let l = parse(&fixture());
        let updated = apply_active_layout(&l, "at-translated-set-2-keyboard,English (US)");
        assert!(updated.available);
        assert_eq!(updated.current, "English (US)");
        assert_eq!(updated.names, l.names);
    }

    #[test]
    fn apply_active_layout_leaves_an_unavailable_layout_untouched() {
        let l = unavailable();
        assert_eq!(apply_active_layout(&l, "kb,German"), l);
    }

    #[test]
    fn apply_active_layout_with_no_comma_leaves_the_layout_unchanged() {
        let l = parse(&fixture());
        assert_eq!(apply_active_layout(&l, "nocommahere"), l);
    }
}
