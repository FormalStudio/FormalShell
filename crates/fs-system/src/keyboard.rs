//! Keyboard layout, normalized for the bar cell. The compositor backend reads
//! it and applies its layout switches; this is the shape it lands in and what
//! the cell derives from it.
//!
//! `available: false` means the compositor could not be asked at all (the query
//! has not answered, or it failed). The widget then renders NO LAYOUT rather
//! than guessing. `current_idx` is -1 whenever the position of the active
//! layout inside `names` is not known; 0 would be a valid index and
//! indistinguishable from "the first layout is active".

use fs_js as js;

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

    fn names(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    fn layout(items: &[&str], current: &str) -> Layout {
        Layout { available: true, names: names(items), current_idx: -1, current: current.into() }
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
    fn has_choice_gates_the_cell_on_more_than_one_layout() {
        assert!(has_choice(&layout(&["us", "de"], "German")));
        assert!(!has_choice(&layout(&["us"], "English (US)")));
        assert!(!has_choice(&unavailable()));
    }

    #[test]
    fn tooltip_lists_every_layout_with_the_active_one_first() {
        let l = Layout { available: true, names: names(&["English (US)", "German"]), current_idx: 1, current: "German".into() };
        assert_eq!(tooltip_text(Some(&l)), "GERMAN / ENGLISH (US)");
    }

    #[test]
    fn tooltip_without_a_known_index_leads_with_the_active_keymap() {
        assert_eq!(tooltip_text(Some(&layout(&["us", "de"], "German"))), "GERMAN / US / DE");
    }

    #[test]
    fn tooltip_of_an_unavailable_backend_is_no_layout() {
        assert_eq!(tooltip_text(Some(&unavailable())), "NO LAYOUT");
        assert_eq!(tooltip_text(None), "NO LAYOUT");
    }
}
