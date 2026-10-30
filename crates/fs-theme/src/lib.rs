//! Theme resolution shared by every surface: the chrome table in
//! `formalshell.json`, the tokens, the palette and the pure halves of the
//! matugen pipeline. No IO and no processes; `services::theme` in
//! formalshell-rs runs those.

use std::sync::LazyLock;

use serde_json::Value;

pub mod barpaint;
pub mod chrome;
pub mod color;
pub mod flexoki;
pub mod gtk;
pub mod icons;
pub mod matugen;
pub mod palette;
pub mod sky;
pub mod style;
pub mod sun;
pub mod theme;
pub mod tokens;
pub mod zenbones;

static TABLE: LazyLock<Value> =
    LazyLock::new(|| serde_json::from_str(include_str!("../formalshell.json")).expect("formalshell.json is valid JSON"));

/// The chrome table, elementary OS 8's material. `style` documents the
/// schema and resolves it.
pub fn table() -> &'static Value {
    &TABLE
}
