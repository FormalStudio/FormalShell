//! Pure model for the network panel's wifi rows: sort order. Ported from
//! `shell/Network/model.js`.
//!
//! Signal strength is the toolkit's native 0..1 fraction, not a percentage.

use std::cmp::Ordering;

/// What the sort and the section split read of a row; the panel's own row
/// type carries more.
pub trait WifiRowLike {
    fn connected(&self) -> bool;
    fn known(&self) -> bool;
    fn signal_strength(&self) -> f64;
}

/// `x || 0`: NaN reads as 0.
fn or_zero(x: f64) -> f64 {
    if x.is_nan() { 0.0 } else { x }
}

/// Connected first, then known (saved) networks, then everything else, each
/// tier sorted by signal strength descending. Non-mutating.
pub fn sort_wifi_rows<T: WifiRowLike + Clone>(rows: &[T]) -> Vec<T> {
    let mut list = rows.to_vec();
    list.sort_by(|a, b| {
        if a.connected() != b.connected() {
            return if a.connected() { Ordering::Less } else { Ordering::Greater };
        }
        if a.known() != b.known() {
            return if a.known() { Ordering::Less } else { Ordering::Greater };
        }
        or_zero(b.signal_strength()).partial_cmp(&or_zero(a.signal_strength())).unwrap_or(Ordering::Equal)
    });
    list
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone, Debug, PartialEq)]
    struct WifiRow {
        name: String,
        connected: bool,
        known: bool,
        signal_strength: f64,
    }

    impl WifiRowLike for WifiRow {
        fn connected(&self) -> bool {
            self.connected
        }

        fn known(&self) -> bool {
            self.known
        }

        fn signal_strength(&self) -> f64 {
            self.signal_strength
        }
    }

    fn row(name: &str, connected: bool, known: bool, signal_strength: f64) -> WifiRow {
        WifiRow { name: name.into(), connected, known, signal_strength }
    }

    fn names(rows: &[WifiRow]) -> String {
        rows.iter().map(|r| r.name.as_str()).collect::<Vec<_>>().join(",")
    }

    #[test]
    fn sort_wifi_rows_connected_then_known_then_signal_desc() {
        let sorted = sort_wifi_rows(&[
            row("A", false, true, 0.5),
            row("B", true, true, 0.2),
            row("C", false, false, 0.9),
            row("D", false, true, 0.8),
        ]);
        assert_eq!(names(&sorted), "B,D,A,C");
    }

    #[test]
    fn sort_wifi_rows_does_not_mutate_input() {
        let input = [row("A", false, true, 0.1), row("B", true, true, 0.9)];
        let before = input.clone();
        sort_wifi_rows(&input);
        assert_eq!(input, before);
    }
}
