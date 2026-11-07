//! Which output the shell treats as the main one, for every surface that has
//! to pick a single screen: the screensaver's animated head, and anything
//! downstream that wants to name the main monitor rather than guess at it.
//!
//! `display.outputPriority` names it, in preference order:
//!
//! ```json
//! "display": { "outputPriority": ["HDMI", "internal"] }
//! ```
//!
//! First entry with a connected output wins, so that list reads "the desk
//! monitor when it's plugged in, the laptop panel when it isn't". An entry
//! matches a connector by exact name ("HDMI-A-1"), by the port it hangs off
//! ("HDMI", "DP-2"), or by one of the two aliases below. Unset, the default
//! falls through to the focused output, which on a single-head session is the
//! only output there is.

use fs_js as js;
use serde_json::Value;

/// Connectors the panel built into the machine shows up as.
const INTERNAL_PREFIXES: [&str; 3] = ["edp", "lvds", "dsi"];

/// A configured priority wins outright, and is re-applied on every screen
/// change: plugging the main monitor back in hands the title straight to it,
/// which is the whole point of naming it first.
///
/// `current` only holds the line for an unconfigured session, and only for
/// callers that pass one. There the answer comes from focus, which says
/// nothing new once the session is already idle, so a screen arriving must not
/// drag a running screensaver off the output it started on: restarting ttfx
/// there would replay the effect from frame 0 on a screen already past it. An
/// unplug still moves it: the name is gone from `names`, so the fallbacks
/// below pick up. Empty only when there are no outputs at all.
pub fn resolve_main_output(names: &[String], priority: &[String], focused: &str, current: &str) -> String {
    let preferred = match_priority(names, priority);
    if !preferred.is_empty() {
        return preferred;
    }
    if contains(names, current) {
        return current.to_string();
    }
    if contains(names, focused) {
        return focused.to_string();
    }
    names.first().cloned().unwrap_or_default()
}

/// The first entry in the priority list with a connected output, or "", which
/// is also how a list of nothing but typos and unplugged monitors answers, so
/// the caller can say so once rather than silently falling back somewhere
/// else.
pub(crate) fn match_priority(names: &[String], priority: &[String]) -> String {
    priority
        .iter()
        .map(|entry| match_entry(names, entry))
        .find(|hit| !hit.is_empty())
        .unwrap_or_default()
}

/// One entry to the connector it names, or "".
///
/// Exact before prefix: an "HDMI-A-1" in the list is answered by that
/// connector even on a machine where "HDMI-A-2" sorts first.
pub(crate) fn match_entry(names: &[String], entry: &str) -> String {
    let wanted = entry.to_lowercase();
    if wanted.is_empty() {
        return String::new();
    }
    if let Some(n) = names.iter().find(|n| n.to_lowercase() == wanted) {
        return n.clone();
    }
    if wanted == "internal" || wanted == "external" {
        let want_internal = wanted == "internal";
        return names.iter().find(|n| is_internal(n) == want_internal).cloned().unwrap_or_default();
    }
    // Anchored, so "DP" names the DisplayPort outputs and not the "eDP" the
    // laptop panel hangs off.
    names.iter().find(|n| n.to_lowercase().starts_with(&wanted)).cloned().unwrap_or_default()
}

pub(crate) fn is_internal(name: &str) -> bool {
    let lower = name.to_lowercase();
    INTERNAL_PREFIXES.iter().any(|p| lower.starts_with(p))
}

/// settings.json is hand-written, so a single output named as a bare string is
/// a likelier mistake than a considered one. Read it as a one-entry list
/// instead of ignoring the whole key. Entries that are not strings are
/// stringified the way a JS `String(entry || "")` would.
pub fn priority_list(value: &Value) -> Vec<String> {
    match value {
        Value::String(s) if !s.is_empty() => vec![s.clone()],
        Value::Array(entries) => entries
            .iter()
            .map(|e| match e {
                Value::String(s) => s.clone(),
                Value::Number(n) => {
                    let f = n.as_f64().unwrap_or(0.0);
                    if f == 0.0 { String::new() } else { js::num_str(f) }
                }
                Value::Bool(true) => "true".to_string(),
                Value::Object(_) => "[object Object]".to_string(),
                _ => String::new(),
            })
            .collect(),
        _ => Vec::new(),
    }
}

fn contains(names: &[String], name: &str) -> bool {
    !name.is_empty() && names.iter().any(|n| n == name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn v(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    fn both() -> Vec<String> {
        v(&["eDP-1", "HDMI-A-1"])
    }

    // ---- the priority list

    #[test]
    fn first_connected_entry_wins() {
        assert_eq!(resolve_main_output(&both(), &v(&["HDMI-A-1", "eDP-1"]), "eDP-1", ""), "HDMI-A-1");
        assert_eq!(resolve_main_output(&both(), &v(&["eDP-1", "HDMI-A-1"]), "HDMI-A-1", ""), "eDP-1");
    }

    #[test]
    fn entry_matches_a_port_without_its_index() {
        assert_eq!(match_entry(&both(), "HDMI"), "HDMI-A-1");
        assert_eq!(match_entry(&v(&["DP-2", "eDP-1"]), "DP"), "DP-2");
        // Anchored: "DP" is DisplayPort, never the "eDP" the panel is on.
        assert_eq!(match_entry(&v(&["eDP-1"]), "DP"), "");
    }

    #[test]
    fn exact_name_beats_a_port_prefix() {
        assert_eq!(match_entry(&v(&["HDMI-A-2", "HDMI-A-1"]), "HDMI-A-1"), "HDMI-A-1");
    }

    #[test]
    fn internal_and_external_aliases() {
        assert_eq!(match_entry(&both(), "internal"), "eDP-1");
        assert_eq!(match_entry(&both(), "external"), "HDMI-A-1");
        assert_eq!(match_entry(&v(&["LVDS-1", "DP-3"]), "internal"), "LVDS-1");
        assert_eq!(match_entry(&v(&["DSI-1"]), "external"), "");
        assert!(is_internal("eDP-1"));
        assert!(!is_internal("HDMI-A-1"));
    }

    #[test]
    fn priority_is_case_insensitive() {
        assert_eq!(match_entry(&both(), "hdmi-a-1"), "HDMI-A-1");
        assert_eq!(match_entry(&both(), "Internal"), "eDP-1");
    }

    #[test]
    fn a_single_output_named_as_a_bare_string_still_reads() {
        assert_eq!(priority_list(&json!("HDMI")), v(&["HDMI"]));
        assert_eq!(priority_list(&json!("")), Vec::<String>::new());
        assert_eq!(priority_list(&Value::Null), Vec::<String>::new());
        assert_eq!(resolve_main_output(&both(), &priority_list(&json!("HDMI")), "eDP-1", ""), "HDMI-A-1");
    }

    // ---- fallbacks

    #[test]
    fn a_list_matching_nothing_falls_through_to_focus() {
        assert_eq!(match_priority(&both(), &v(&["DP-4", "nonsense"])), "");
        assert_eq!(resolve_main_output(&both(), &v(&["DP-4"]), "HDMI-A-1", ""), "HDMI-A-1");
    }

    #[test]
    fn no_list_configured_animates_the_focused_output() {
        assert_eq!(resolve_main_output(&both(), &[], "HDMI-A-1", ""), "HDMI-A-1");
        assert_eq!(resolve_main_output(&both(), &[], "eDP-1", ""), "eDP-1");
    }

    #[test]
    fn single_output_animates_whatever_focus_says() {
        assert_eq!(resolve_main_output(&v(&["winit"]), &[], "", ""), "winit");
        // A backend reporting an output that isn't connected (or isn't
        // reporting at all) still has to leave something animating.
        assert_eq!(resolve_main_output(&v(&["winit"]), &[], "DP-3", ""), "winit");
        assert_eq!(resolve_main_output(&v(&["winit"]), &v(&["HDMI", "internal"]), "", ""), "winit");
    }

    #[test]
    fn no_outputs_resolves_to_nothing() {
        assert_eq!(resolve_main_output(&[], &v(&["HDMI"]), "eDP-1", ""), "");
        assert_eq!(resolve_main_output(&[], &[], "", "eDP-1"), "");
    }

    // ---- hotplug

    #[test]
    fn the_main_monitor_takes_the_run_back_when_it_returns() {
        assert_eq!(resolve_main_output(&both(), &v(&["HDMI", "internal"]), "eDP-1", "eDP-1"), "HDMI-A-1");
    }

    #[test]
    fn unplugging_the_main_monitor_falls_back_down_the_list() {
        assert_eq!(resolve_main_output(&v(&["eDP-1"]), &v(&["HDMI", "internal"]), "eDP-1", "HDMI-A-1"), "eDP-1");
        // Nothing left in the list either, so focus takes over.
        assert_eq!(resolve_main_output(&v(&["DP-1"]), &v(&["HDMI", "internal"]), "DP-1", "HDMI-A-1"), "DP-1");
    }

    #[test]
    fn an_unconfigured_run_stays_on_the_screen_it_started_on() {
        // No list, so a monitor arriving must not restart ttfx elsewhere.
        assert_eq!(resolve_main_output(&both(), &[], "HDMI-A-1", "eDP-1"), "eDP-1");
        // Unless it was the one that left.
        assert_eq!(resolve_main_output(&v(&["eDP-1"]), &[], "eDP-1", "HDMI-A-1"), "eDP-1");
    }

}
