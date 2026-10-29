//! The chrome tables, embedded from the JSON files in `themes/`. `style` documents the schema and resolves it.

use std::sync::LazyLock;

use serde_json::Value;

static METAMORPHOSIS: LazyLock<Value> = LazyLock::new(|| {
    serde_json::from_str(include_str!("../themes/metamorphosis.json"))
        .expect("metamorphosis.json is valid JSON")
});

static PANTHEON: LazyLock<Value> = LazyLock::new(|| {
    serde_json::from_str(include_str!("../themes/pantheon.json"))
        .expect("pantheon.json is valid JSON")
});

/// shadcn chrome on Omarchy habits, the shipped look.
pub fn metamorphosis() -> &'static Value {
    &METAMORPHOSIS
}

/// elementary OS 8's material.
pub fn pantheon() -> &'static Value {
    &PANTHEON
}

/// Retro is the metamorphosis table on different preset scalars, the same
/// table rather than a copy of it.
pub fn retro() -> &'static Value {
    &METAMORPHOSIS
}
