//! The wall clock, published once a minute on the minute.

use std::time::Duration;

use async_io::Timer;
use chrono::{Local, NaiveDateTime, Timelike};

use crate::runtime::Ctx;
use crate::store;

pub struct State {
    pub text: String,
    /// The wall clock when the minute last changed, for the formats a cell
    /// renders itself.
    pub now: NaiveDateTime,
}

impl Default for State {
    fn default() -> Self {
        Self { text: String::new(), now: Local::now().naive_local() }
    }
}

pub enum Diff {
    Text(String),
}

impl State {
    pub fn apply(&mut self, diff: Diff) -> bool {
        let Diff::Text(text) = diff;
        if self.text == text {
            return false;
        }
        self.text = text;
        self.now = Local::now().naive_local();
        true
    }
}

/// Time left until the next wall-clock minute starts.
fn until_next_minute() -> Duration {
    let now = Local::now();
    let into = Duration::new(now.second() as u64, now.nanosecond() % 1_000_000_000);
    Duration::from_secs(60) - into
}

pub fn start(ctx: &Ctx) {
    ctx.spawn(run(ctx.clone()));
}

async fn run(ctx: Ctx) {
    loop {
        ctx.publish(store::Diff::Clock(Diff::Text(Local::now().format("%H:%M").to_string())));
        Timer::after(until_next_minute()).await;
    }
}
