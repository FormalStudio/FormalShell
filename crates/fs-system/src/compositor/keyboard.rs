//! Keyboard layout, normalized for the bar cell. One `hyprctl devices -j` at
//! boot and on `configreloaded`, `activelayout` events applied in place through
//! `apply_active_layout`.
//!
//! `available: false` means the compositor could not be asked at all (the query
//! has not answered, or it failed). The widget then renders NO LAYOUT rather
//! than guessing. `current_idx` is -1 whenever the position of the active
//! layout inside `names` is not known; 0 would be a valid index and
//! indistinguishable from "the first layout is active".
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
use serde_json::Value;

#[derive(Debug, Clone, PartialEq)]
pub struct Layout {
    pub available: bool,
    pub names: Vec<String>,
    pub current_idx: i64,
    pub current: String,
}

/// The shared honest-empty value: the widget's initial state and every parse
/// failure land on the same value, so no call site invents its own.
pub fn unavailable() -> Layout {
    Layout { available: false, names: Vec::new(), current_idx: -1, current: String::new() }
}

/// The bar cell's text: the parenthetical region code, uppercased ("English
/// (US)" -> "US", "English (US, intl.)" -> "US"), falling back to the first two
/// letters for a name with no parenthetical ("German" -> "GE", and an xkb code
/// "de" -> "DE"). The parenthetical is never truncated: "English (Dvorak)"
/// reads DVORAK, because "DV" would be ambiguous.
pub fn short_label(name: &str) -> String {
    let text = js::trim(name);
    if text.is_empty() {
        return String::new();
    }
    if let Some(open) = text.find('(') {
        let after = &text[open + 1..];
        let inner = after.find(')').map_or(after, |close| &after[..close]);
        let first = js::trim(inner.split(',').next().unwrap_or(""));
        if !first.is_empty() {
            return first.to_uppercase();
        }
    }
    text.chars().take(2).collect::<String>().to_uppercase()
}

/// stdout of `hyprctl devices -j`, verbatim. Reads the keyboard flagged `main`,
/// falling back to the first one listed. Hyprland applies layout switches to
/// the main keyboard, and a session with no keyboard at all has no layout to
/// report, which is `unavailable()` rather than a fabricated one.
pub fn parse_hyprland_layouts(text: &str) -> Layout {
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

/// The bar cell's `shown` gate. A single-layout session has nothing to report
/// and a permanently static cell is noise, the same judgement the battery cell
/// makes with no battery.
pub fn has_choice(layout: &Layout) -> bool {
    layout.available && layout.names.len() >= 2
}

/// One tooltip string, active layout first, joined " / ". Uppercased to sit in
/// the meta band with every other tooltip.
pub fn tooltip_text(layout: Option<&Layout>) -> String {
    let Some(l) = layout.filter(|l| l.available) else {
        return "NO LAYOUT".into();
    };
    let mut names = l.names.clone();
    match usize::try_from(l.current_idx).ok().filter(|i| *i < names.len()) {
        Some(i) => {
            let head = names.remove(i);
            names.insert(0, head);
        }
        None if !l.current.is_empty() => names.insert(0, l.current.clone()),
        None => {}
    }
    if names.is_empty() {
        return "NO LAYOUT".into();
    }
    names.iter().map(|n| n.to_uppercase()).collect::<Vec<_>>().join(" / ")
}

#[cfg(test)]
mod tests {
    use super::*;
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

    fn names(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn short_label_extracts_the_parenthetical_region_uppercased() {
        assert_eq!(short_label("English (US)"), "US");
        assert_eq!(short_label("English (US, intl., with dead keys)"), "US");
    }

    #[test]
    fn short_label_falls_back_to_first_two_letters_when_no_parenthetical() {
        assert_eq!(short_label("German"), "GE");
        assert_eq!(short_label("de"), "DE");
    }

    #[test]
    fn short_label_of_empty_is_empty() {
        assert_eq!(short_label(""), "");
    }

    #[test]
    fn unavailable_is_the_shared_honest_empty_shape() {
        let l = unavailable();
        assert!(!l.available);
        assert_eq!(l.names.len(), 0);
        assert_eq!(l.current_idx, -1);
        assert_eq!(l.current, "");
    }

    #[test]
    fn hyprland_reads_the_main_keyboard_not_the_first_listed() {
        let l = parse_hyprland_layouts(&fixture());
        assert!(l.available);
        assert_eq!(l.names, names(&["us", "de"]));
        assert_eq!(l.current, "German");
    }

    #[test]
    fn hyprland_reports_no_index_because_codes_and_keymap_names_do_not_map() {
        assert_eq!(parse_hyprland_layouts(&fixture()).current_idx, -1);
    }

    #[test]
    fn hyprland_falls_back_to_the_first_keyboard_when_none_is_main() {
        let text = json!({ "keyboards": [{ "name": "kb", "layout": "fr", "active_keymap": "French", "main": false }] }).to_string();
        let l = parse_hyprland_layouts(&text);
        assert!(l.available);
        assert_eq!(l.current, "French");
    }

    #[test]
    fn hyprland_with_no_keyboards_or_bad_output_is_unavailable() {
        assert!(!parse_hyprland_layouts(&json!({ "keyboards": [] }).to_string()).available);
        assert!(!parse_hyprland_layouts("").available);
        assert!(!parse_hyprland_layouts("Invalid command").available);
    }

    #[test]
    fn has_choice_gates_the_cell_on_more_than_one_layout() {
        assert!(has_choice(&parse_hyprland_layouts(&fixture())));
        let single = json!({ "keyboards": [{ "name": "kb", "layout": "us", "active_keymap": "English (US)", "main": true }] }).to_string();
        assert!(!has_choice(&parse_hyprland_layouts(&single)));
        assert!(!has_choice(&unavailable()));
    }

    #[test]
    fn tooltip_lists_every_layout_with_the_active_one_first() {
        let l = Layout { available: true, names: names(&["English (US)", "German"]), current_idx: 1, current: "German".into() };
        assert_eq!(tooltip_text(Some(&l)), "GERMAN / ENGLISH (US)");
    }

    #[test]
    fn tooltip_without_a_known_index_leads_with_the_active_keymap() {
        assert_eq!(tooltip_text(Some(&parse_hyprland_layouts(&fixture()))), "GERMAN / US / DE");
    }

    #[test]
    fn tooltip_of_an_unavailable_backend_is_no_layout() {
        assert_eq!(tooltip_text(Some(&unavailable())), "NO LAYOUT");
        assert_eq!(tooltip_text(None), "NO LAYOUT");
    }

    #[test]
    fn apply_active_layout_sets_current_from_the_event_data() {
        let l = parse_hyprland_layouts(&fixture());
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
        let l = parse_hyprland_layouts(&fixture());
        assert_eq!(apply_active_layout(&l, "nocommahere"), l);
    }
}
