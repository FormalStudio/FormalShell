//! Calendar events for the panel: local `.ics`
//! files under `calendar.icsDir` and EDS through `formalshell-eds events`,
//! merged by UID. Both refresh every five minutes and when the panel opens.
//! The first failed `formalshell-eds` run switches that backend off for the
//! rest of the process, with one log line and no error cell.

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use chrono::Local;
use fs_info::calendar::ics::{self, Event};
use futures_lite::FutureExt;

use super::{idle, kicked};
use crate::runtime::Ctx;
use crate::services::config;
use crate::services::proc;
use crate::services::wants::Source;
use crate::store;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct State {
    pub events: Vec<Event>,
}

#[derive(Clone, Debug, PartialEq)]
struct Cfg {
    dir: String,
    eds: bool,
}

fn read() -> Cfg {
    let s = config::current();
    Cfg { dir: s.str("calendar.icsDir").unwrap_or_default().to_owned(), eds: s.flag("calendar.eds", true) }
}

static EDS_DOWN: AtomicBool = AtomicBool::new(false);

/// `cat "$dir"/*.ics`, each file followed by a newline.
fn read_dir(dir: &str) -> String {
    let Ok(read) = std::fs::read_dir(Path::new(dir)) else { return String::new() };
    let mut files: Vec<_> = read.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == "ics") && p.is_file()).collect();
    files.sort();
    files.iter().filter_map(|p| std::fs::read_to_string(p).ok()).map(|t| t + "\n").collect()
}

async fn eds() -> Vec<Event> {
    let done = proc::capture(&proc::argv(&["formalshell-eds", "events"]), Duration::from_secs(60)).await;
    if done.code != 0 {
        if !EDS_DOWN.swap(true, Ordering::Relaxed) {
            eprintln!("calendar: formalshell-eds exited {}: EDS events disabled, ics-only from here on", done.code);
        }
        return Vec::new();
    }
    ics::parse_events(&done.stdout, ics::default_window(Local::now()))
}

pub async fn run(ctx: Ctx) {
    let rx = config::subscribe();
    let kick = kicked(Source::Calendar);
    loop {
        let cfg = read();
        let window = ics::default_window(Local::now());
        let from_files = if cfg.dir.is_empty() {
            Vec::new()
        } else {
            let dir = cfg.dir.clone();
            ctx.pool().run(move || ics::parse_events(&read_dir(&dir), window)).await.unwrap_or_default()
        };
        let from_eds = if cfg.eds && !EDS_DOWN.load(Ordering::Relaxed) { eds().await } else { Vec::new() };
        ctx.publish(store::Diff::Calendar(State { events: ics::merge_events(&from_files, &from_eds) }));
        idle(Duration::from_secs(300), &rx, &cfg, read)
            .or(async {
                let _ = kick.recv().await;
            })
            .await;
    }
}
