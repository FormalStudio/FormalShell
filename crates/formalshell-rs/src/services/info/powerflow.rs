//! What the power panel samples while it is open: the CPU package draw
//! the host's poller leaves in `/run/formalshell/rapl`, and one sysfs pass
//! over the supplies and USB-C ports. A missing poller leaves `cpu_w` empty
//! rather than 0.

use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::time::Duration;

use async_io::Timer;
use fs_system::power::flow::{self, PORT_KEYS, SUPPLY_KEYS, Snapshot};
use fs_system::power::model::parse_rapl_mw;

use crate::runtime::Ctx;
use crate::store;

const RAPL: &str = "/run/formalshell/rapl";
const UCSI: &str = "/run/formalshell/ucsi";
const SAMPLE: Duration = Duration::from_millis(3000);

#[derive(Clone, Debug, Default, PartialEq)]
pub struct State {
    pub cpu_w: Option<f64>,
    pub snapshot: Snapshot,
}

pub async fn run(ctx: Ctx) {
    loop {
        let read = || {
            let cpu_w = std::fs::read_to_string(RAPL).ok().and_then(|t| parse_rapl_mw(&t));
            State { cpu_w, snapshot: flow::parse_snapshot(&read_sysfs()) }
        };
        if let Some(state) = ctx.pool().run(read).await {
            ctx.publish(store::Diff::PowerFlow(state));
        }
        Timer::after(SAMPLE).await;
    }
}

/// The supplies, the USB-C ports and the host's UCSI dump as `@section`
/// lines of `key=value`. An attribute the kernel refuses to read (a battery
/// between samples answers EIO) is listed with no value.
fn read_sysfs() -> String {
    let mut text = String::new();
    for dir in entries(Path::new("/sys/class/power_supply")).into_iter().filter(|d| d.is_dir()) {
        text.push_str(&format!("@supply {}\n", name(&dir)));
        for key in SUPPLY_KEYS {
            field(&mut text, &dir, key);
        }
    }
    let port = |d: &PathBuf| {
        let n = name(d);
        n.strip_prefix("port").is_some_and(|rest| rest.starts_with(|c: char| c.is_ascii_digit())) && !n.ends_with("-partner") && d.is_dir()
    };
    for dir in entries(Path::new("/sys/class/typec")).into_iter().filter(port) {
        text.push_str(&format!("@typec {}\n", name(&dir)));
        for key in PORT_KEYS {
            field(&mut text, &dir, key);
        }
        let partner = PathBuf::from(format!("{}-partner", dir.display()));
        if partner.exists() {
            text.push_str("partner=1\n");
            for alt in entries(&partner) {
                field(&mut text, &alt, "product");
            }
        }
    }
    text.push_str("@ucsi\n");
    text.push_str(&std::fs::read_to_string(UCSI).unwrap_or_default());
    text
}

/// A directory's entries in name order, hidden ones left out.
fn entries(dir: &Path) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> =
        std::fs::read_dir(dir).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| !name(p).starts_with('.')).collect();
    out.sort();
    out
}

fn name(path: &Path) -> String {
    path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

fn field(text: &mut String, dir: &Path, key: &str) {
    match std::fs::read_to_string(dir.join(key)) {
        Ok(value) => text.push_str(&format!("{key}={}\n", value.trim_end_matches('\n'))),
        Err(e) if matches!(e.kind(), ErrorKind::NotFound | ErrorKind::PermissionDenied | ErrorKind::NotADirectory) => {}
        Err(_) => text.push_str(&format!("{key}=\n")),
    }
}
