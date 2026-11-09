//! Flake inputs behind their upstream: one
//! shared pass over `<systemUpdate.flakeDir>/flake.lock`, however many
//! cells and panels read it. Stage one reads the lock off the pool, stage
//! two probes each input's forge one at a time. The lock changing on disk
//! starts a new pass.

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;

use fs_info::system_update::{self, Counts, FlakeInput, PollState};

use super::{idle, kicked};
use crate::runtime::Ctx;
use crate::services::config;
use crate::services::http;
use crate::services::proc;
use crate::services::wants::Source;
use crate::services::watch::Watch;
use crate::store;

#[derive(Clone, Debug, PartialEq)]
pub struct State {
    pub poll: PollState,
    pub inputs: Vec<FlakeInput>,
    pub heads: HashMap<String, String>,
}

impl Default for State {
    fn default() -> Self {
        Self { poll: PollState::Checking, inputs: Vec::new(), heads: HashMap::new() }
    }
}

impl State {
    pub fn counts(&self) -> Counts {
        system_update::count_behind(&self.inputs, &self.heads)
    }

    pub fn summary(&self) -> String {
        system_update::summary_label(self.poll, &self.counts())
    }
}

#[derive(Clone, Debug, PartialEq)]
struct Cfg {
    dir: String,
    interval: Duration,
}

fn read() -> Cfg {
    let s = config::current();
    Cfg { dir: s.str("systemUpdate.flakeDir").unwrap_or("").to_owned(), interval: s.interval("systemUpdate.intervalMs", 10_800_000) }
}

fn publish(ctx: &Ctx, state: State) {
    ctx.publish(store::Diff::SystemUpdate(state));
}

/// One pass: the lock, then every probe. Answers the state it ended on.
async fn pass(ctx: &Ctx, dir: &str) {
    if dir.is_empty() {
        return publish(ctx, State { poll: PollState::NoFlake, ..State::default() });
    }
    let path = PathBuf::from(dir).join("flake.lock");
    let text = ctx.pool().run(move || std::fs::read_to_string(path)).await;
    let lock = match text {
        Some(Ok(text)) => system_update::parse_lock(&text),
        _ => Default::default(),
    };
    if !lock.ok {
        return publish(ctx, State { poll: PollState::NoLock, ..State::default() });
    }
    let mut state = State { poll: PollState::Checking, inputs: lock.inputs, heads: HashMap::new() };
    publish(ctx, state.clone());
    let (mut attempted, mut resolved) = (0, 0);
    for input in state.inputs.clone() {
        let probe = system_update::probe_command(&input);
        if probe.kind == system_update::ProbeKind::None {
            continue;
        }
        attempted += 1;
        let (code, stdout) = match probe.kind {
            system_update::ProbeKind::Github | system_update::ProbeKind::Gitlab => {
                let mut req = http::Request::get(&probe.url, 15);
                for h in &probe.headers {
                    req = req.header(h.as_str());
                }
                match http::send(&req).await {
                    Ok(r) if r.ok() => (0, r.text()),
                    _ => (22, String::new()),
                }
            }
            _ => {
                let done = proc::capture(&probe.argv, Duration::from_secs(60)).await;
                (done.code, done.stdout)
            }
        };
        let rev = system_update::parse_probe(probe.kind, code, &stdout);
        if !rev.is_empty() {
            resolved += 1;
        }
        state.heads.insert(input.name, rev);
        publish(ctx, state.clone());
    }
    state.poll = if attempted > 0 && resolved == 0 { PollState::Offline } else { PollState::Ok };
    publish(ctx, state);
}

pub async fn run(ctx: Ctx) {
    let rx = config::subscribe();
    let kick = kicked(Source::SystemUpdate);
    while kick.try_recv().is_ok() {}
    loop {
        let cfg = read();
        pass(&ctx, &cfg.dir).await;
        let mut lock = (!cfg.dir.is_empty()).then(|| Watch::new(PathBuf::from(&cfg.dir).join("flake.lock")).ok()).flatten();
        let moved = async {
            match lock.as_mut() {
                Some(watch) => watch.changed().await,
                None => std::future::pending().await,
            }
        };
        let asked = async {
            let _ = kick.recv().await;
        };
        futures_lite::future::or(asked, futures_lite::future::or(idle(cfg.interval, &rx, &cfg, read), moved)).await;
    }
}
