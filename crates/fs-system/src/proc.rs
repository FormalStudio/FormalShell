//! The parts of a desktop entry and an action the launcher hands to a spawn.

/// The parts of a desktop entry `app_launch` reads.
#[derive(Debug, Clone, Default)]
pub struct Entry {
    pub id: String,
    pub command: Vec<String>,
    pub run_in_terminal: bool,
}

#[derive(Debug, Clone, Default)]
pub struct Action {
    pub command: Vec<String>,
}
