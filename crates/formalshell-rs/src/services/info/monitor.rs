//! System monitor data: the
//! /proc and /sys reads of one tick happen on the blocking pool, the
//! fs-system parsers turn them into numbers, and CPU and network figures
//! are deltas against the tick before, so the first tick after a start
//! answers `null` rather than a made-up zero. `collect` files each read
//! under the section name its fs-system parser takes.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

#[cfg(test)]
use fs_system::monitor::collect::split_sections;
use fs_system::monitor::gpu::{self, GfxMode, GpuRecord};
use fs_system::monitor::sysinfo::{self, CoreLoad, CpuRecord, Disk, Fan, Load, Mem, NetRate, NetRow, Temp, Uptime};
use futures_lite::future;
use serde_json::{Value, json};

use super::idle;
use crate::runtime::Ctx;
use crate::services::config;
use crate::services::proc::{self, on_path};
use crate::store;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Cpu {
    pub available: bool,
    pub aggregate: Option<f64>,
    pub cores: Vec<CoreLoad>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Card {
    pub record: GpuRecord,
    pub name: String,
    pub discrete: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct State {
    /// Unix milliseconds of the last tick, none before the first lands.
    pub sampled_ms: Option<u64>,
    pub cpu: Cpu,
    pub mem: Option<Mem>,
    pub load: Option<Load>,
    pub uptime: Option<Uptime>,
    pub temps: Vec<Temp>,
    pub fans: Vec<Fan>,
    pub net_available: bool,
    pub net: Vec<NetRate>,
    pub disk: Vec<Disk>,
    pub cards: Vec<Card>,
    pub gfx: Option<GfxMode>,
    /// nvidia-offload and prime-run on PATH.
    pub tools: (bool, bool),
}

impl State {
    /// The busy fraction of the first card that reports one (the cell's G).
    pub fn gpu_busy(&self) -> Option<f64> {
        self.cards.iter().find_map(|c| c.record.metrics.busy.filter(|_| c.record.metrics.available))
    }

    /// The IPC `status` reply.
    pub fn status(&self, now_ms: u64) -> Value {
        let num = |v: Option<f64>| v.map_or(Value::Null, |n| json!(n));
        json!({
            "sampledAtMs": self.sampled_ms,
            "ageMs": self.sampled_ms.map(|t| now_ms.saturating_sub(t)),
            "cpu": {
                "available": self.cpu.available,
                "aggregate": num(self.cpu.aggregate),
                "cores": self.cpu.cores.iter().map(|c| json!({"label": c.label, "fraction": c.fraction})).collect::<Vec<_>>(),
            },
            "mem": match &self.mem {
                Some(m) => json!({
                    "available": true, "totalBytes": m.total_bytes, "availableBytes": m.available_bytes,
                    "freeBytes": m.free_bytes, "swapTotalBytes": m.swap_total_bytes,
                    "swapFreeBytes": m.swap_free_bytes, "usedFraction": m.used_fraction,
                }),
                None => json!({"available": false}),
            },
            "load": match &self.load {
                Some(l) => json!({
                    "available": true, "load1": l.load1, "load5": l.load5, "load15": l.load15,
                    "runningProcs": l.running_procs, "totalProcs": l.total_procs, "lastPid": l.last_pid,
                }),
                None => json!({"available": false}),
            },
            "uptime": match &self.uptime {
                Some(u) => json!({"available": true, "uptimeSeconds": u.uptime_seconds, "idleSeconds": u.idle_seconds}),
                None => json!({"available": false}),
            },
            "temps": {
                "available": !self.temps.is_empty(),
                "rows": self.temps.iter().map(|t| json!({"chip": t.chip, "id": t.id, "label": t.label, "celsius": t.celsius})).collect::<Vec<_>>(),
            },
            "fans": {
                "available": !self.fans.is_empty(),
                "rows": self.fans.iter().map(|f| json!({"chip": f.chip, "id": f.id, "label": f.label, "rpm": f.rpm})).collect::<Vec<_>>(),
            },
            "net": {
                "available": self.net_available,
                "rows": self.net.iter().map(|n| json!({
                    "iface": n.iface, "rxBytesPerSec": n.rx_bytes_per_sec, "txBytesPerSec": n.tx_bytes_per_sec,
                })).collect::<Vec<_>>(),
            },
            "disk": {
                "available": !self.disk.is_empty(),
                "rows": self.disk.iter().map(|d| json!({
                    "source": d.source, "mount": d.mount, "size": d.size, "used": d.used, "fraction": d.fraction,
                })).collect::<Vec<_>>(),
            },
        })
    }

    /// The IPC `gpu` reply.
    pub fn gpu(&self, now_ms: u64) -> Value {
        let cards: Vec<Value> = self.cards.iter().map(card_json).collect();
        json!({
            "sampledAtMs": self.sampled_ms,
            "ageMs": self.sampled_ms.map(|t| now_ms.saturating_sub(t)),
            "available": !cards.is_empty(),
            "cards": cards,
            "gfxMode": match &self.gfx {
                Some(g) => json!({"supported": g.supported, "mode": g.mode}),
                None => json!({"supported": false, "mode": ""}),
            },
            "tools": {"nvidiaOffload": self.tools.0, "primeRun": self.tools.1},
        })
    }
}

fn card_json(c: &Card) -> Value {
    let r = &c.record;
    let m = &r.metrics;
    let num = |v: Option<f64>| v.map_or(Value::Null, |n| json!(n));
    json!({
        "card": r.card.card,
        "driver": r.card.driver,
        "vendorId": r.card.vendor_id,
        "deviceId": r.card.device_id,
        "bootVga": r.card.boot_vga,
        "pci": r.card.pci,
        "label": r.card.label,
        "outputs": r.card.outputs.iter().map(|o| json!({"name": o.name, "connected": o.connected})).collect::<Vec<_>>(),
        "metrics": {
            "available": m.available, "busy": num(m.busy), "tempC": num(m.temp_c), "vramUsed": num(m.vram_used),
            "vramTotal": num(m.vram_total), "powerW": num(m.power_w), "fanPercent": num(m.fan_percent),
            "fanRpm": num(m.fan_rpm), "clockMhz": num(m.clock_mhz), "clockMaxMhz": num(m.clock_max_mhz),
        },
        "name": c.name,
        "discrete": c.discrete,
    })
}

/// Whether `name` is an executable on PATH, for the offload tools the
/// GPU cell asks about.
/// What a tick compares itself to.
#[derive(Default)]
struct Prev {
    stat: Option<Vec<CpuRecord>>,
    net: Option<(Vec<NetRow>, Instant)>,
}

fn section<'a>(sections: &'a HashMap<String, String>, name: &str) -> &'a str {
    sections.get(name).map_or("", String::as_str)
}

fn sample(prev: &mut Prev, sections: &HashMap<String, String>, at: Instant) -> State {
    let stat = sysinfo::parse_stat(section(sections, "stat"));
    let delta = sysinfo::cpu_delta(prev.stat.as_deref(), Some(&stat));
    let cpu = Cpu {
        available: !stat.is_empty(),
        aggregate: delta.as_ref().map(|d| d.aggregate),
        cores: delta.map_or_else(Vec::new, |d| d.cores),
    };
    prev.stat = Some(stat);

    let raw_net = sysinfo::parse_net(section(sections, "net"));
    let elapsed = prev.net.as_ref().map_or(0.0, |(_, t)| at.saturating_duration_since(*t).as_secs_f64() * 1000.0);
    let net = sysinfo::net_delta(prev.net.as_ref().map(|(r, _)| r.as_slice()), Some(&raw_net), elapsed).unwrap_or_default();
    let net_available = !raw_net.is_empty();
    prev.net = Some((raw_net, at));

    let drm = section(sections, "drm");
    let nvidia = gpu::parse_nvidia(section(sections, "nvidia"));
    let merged = gpu::merge_gpu(&gpu::parse_cards(drm), &gpu::parse_metrics(drm), &nvidia);
    let mut nvidia_index = 0;
    let cards = merged
        .into_iter()
        .map(|record| {
            let row = (record.card.driver == "nvidia").then(|| {
                nvidia_index += 1;
                nvidia.get(nvidia_index - 1)
            });
            Card {
                name: gpu::display_name(&record.card, row.flatten()),
                discrete: gpu::is_discrete(&record.card),
                record,
            }
        })
        .collect();

    State {
        sampled_ms: Some(crate::runtime::now_ms() as u64),
        cpu,
        mem: sysinfo::parse_mem(section(sections, "mem")),
        load: sysinfo::parse_load(section(sections, "load")),
        uptime: sysinfo::parse_uptime(section(sections, "uptime")),
        temps: sysinfo::parse_temps(section(sections, "temp")),
        fans: sysinfo::parse_fans(section(sections, "fan")),
        net_available,
        net,
        disk: sysinfo::parse_disk(section(sections, "disk")),
        cards,
        gfx: Some(gpu::parse_gfx_mode(section(sections, "gfx"))),
        tools: (false, false),
    }
}

/// A sysfs attribute's text, trailing newlines off; none when it cannot be
/// read.
fn cat(path: &Path) -> Option<String> {
    std::fs::read_to_string(path).ok().map(|t| t.trim_end_matches('\n').to_owned())
}

fn lines_of(path: &str, keep: impl Fn(&str) -> bool) -> String {
    std::fs::read_to_string(path).unwrap_or_default().lines().filter(|l| keep(l)).collect::<Vec<_>>().join("\n")
}

/// Directory entries whose name starts with `prefix`, in name order.
fn entries(dir: &str, prefix: &str) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().starts_with(prefix))
        .map(|e| e.path())
        .collect();
    out.sort();
    out
}

fn name_of(path: &Path) -> String {
    path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

/// `chip|file|label|value` for each `<kind>*_input` under every hwmon chip.
fn hwmon_rows(kind: &str) -> String {
    let mut rows = Vec::new();
    for hwmon in entries("/sys/class/hwmon", "hwmon").iter().filter(|h| h.is_dir()) {
        let chip = cat(&hwmon.join("name")).unwrap_or_default();
        for input in entries(&hwmon.to_string_lossy(), "").iter().filter(|p| {
            let n = name_of(p);
            n.starts_with(kind) && n.ends_with("_input") && std::fs::File::open(p).is_ok()
        }) {
            let id = name_of(input);
            let label = cat(&hwmon.join(format!("{}_label", id.trim_end_matches("_input")))).unwrap_or_default();
            rows.push(format!("{chip}|{id}|{label}|{}", cat(input).unwrap_or_default()));
        }
    }
    rows.join("\n")
}

/// `readlink -f`'s last component: a missing leaf still names itself when
/// its parent exists.
fn resolved_name(path: &Path) -> String {
    match std::fs::canonicalize(path) {
        Ok(real) => name_of(&real),
        Err(_) if path.parent().is_some_and(Path::exists) => name_of(path),
        Err(_) => String::new(),
    }
}

/// The `card`, `metric` and `conn` rows `gpu` parses, one set per card.
fn drm_rows() -> String {
    // The smoke rig has no hybrid card to read; `--gpu` points this at the
    // rows of a captured one, in place of /sys/class/drm.
    if let Some(rows) = std::env::var_os("FORMALSHELL_SMOKE_DRM_ROWS").and_then(|p| cat(Path::new(&p))) {
        return rows;
    }
    let mut rows = Vec::new();
    for card in entries("/sys/class/drm", "card") {
        let base = name_of(&card);
        let digits = &base[4..];
        if !(1..=2).contains(&digits.len()) || !digits.bytes().all(|b| b.is_ascii_digit()) {
            continue;
        }
        let dev = card.join("device");
        let attr = |name: &str| cat(&dev.join(name)).unwrap_or_default();
        rows.push(format!(
            "card|{base}|{}|{}|{}|{}|{}|{}",
            resolved_name(&dev.join("driver")),
            attr("vendor"),
            attr("device"),
            attr("boot_vga"),
            resolved_name(&dev),
            attr("label"),
        ));
        for f in ["gpu_busy_percent", "mem_info_vram_used", "mem_info_vram_total", "mem_busy_percent"] {
            if let Some(v) = cat(&dev.join(f)) {
                rows.push(format!("metric|{base}|{f}|{v}"));
            }
        }
        for f in ["gt_act_freq_mhz", "gt_max_freq_mhz"] {
            if let Some(v) = cat(&card.join(f)) {
                rows.push(format!("metric|{base}|{f}|{v}"));
            }
        }
        for hw in entries(&dev.join("hwmon").to_string_lossy(), "hwmon").iter().filter(|h| h.is_dir()) {
            for f in ["temp1_input", "power1_average", "fan1_input"] {
                if let Some(v) = cat(&hw.join(f)) {
                    rows.push(format!("metric|{base}|{f}|{v}"));
                }
            }
        }
        for conn in entries("/sys/class/drm", &format!("{base}-")).iter().filter(|k| k.is_dir()) {
            let output = name_of(conn);
            let output = output.trim_start_matches("card").trim_start_matches(|c: char| c.is_ascii_digit());
            let output = output.strip_prefix('-').unwrap_or(output);
            rows.push(format!("conn|{base}|{output}|{}", cat(&conn.join("status")).unwrap_or_default()));
        }
    }
    rows.join("\n")
}

/// One tick's reads, on a pool thread.
fn collect() -> HashMap<String, String> {
    let net = std::fs::read_to_string("/proc/net/dev").unwrap_or_default().lines().skip(2).collect::<Vec<_>>().join("\n");
    let meminfo = ["MemTotal:", "MemAvailable:", "MemFree:", "SwapTotal:", "SwapFree:"];
    [
        ("stat", lines_of("/proc/stat", |l| l.starts_with("cpu"))),
        ("mem", lines_of("/proc/meminfo", |l| meminfo.iter().any(|k| l.starts_with(k)))),
        ("load", cat(Path::new("/proc/loadavg")).unwrap_or_default()),
        ("uptime", cat(Path::new("/proc/uptime")).unwrap_or_default()),
        ("net", net),
        ("temp", hwmon_rows("temp")),
        ("fan", hwmon_rows("fan")),
        ("drm", drm_rows()),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_owned(), v))
    .collect()
}

/// One tick's tool reads, empty for a tool that is missing or hangs.
async fn tools_output() -> [(String, String); 3] {
    let stdout = |argv: &[&str], timeout| {
        let argv = proc::argv(argv);
        async move { proc::capture(&argv, timeout).await.stdout.trim_end_matches('\n').to_owned() }
    };
    let df = stdout(&["df", "-B1", "-x", "tmpfs", "-x", "devtmpfs", "-x", "efivarfs", "--output=source,target,size,used"], TOOL_TIMEOUT);
    let nvidia_query = "--query-gpu=index,name,utilization.gpu,temperature.gpu,memory.used,memory.total,power.draw,fan.speed";
    let nvidia = stdout(&["nvidia-smi", nvidia_query, "--format=csv,noheader,nounits"], TOOL_TIMEOUT);
    let gfx = stdout(&["supergfxctl", "-g"], Duration::from_secs(1));
    let (disk, (nvidia, gfx)) = future::zip(df, future::zip(nvidia, gfx)).await;
    let disk = disk.lines().skip(1).collect::<Vec<_>>().join("\n");
    [("disk".into(), disk), ("nvidia".into(), nvidia), ("gfx".into(), gfx)]
}

const TOOL_TIMEOUT: Duration = Duration::from_secs(5);

fn read() -> std::time::Duration {
    let ms = crate::settings::MONITOR_INTERVAL.get(&config::current()).as_millis().max(500);
    std::time::Duration::from_millis(ms as u64)
}

pub async fn run(ctx: Ctx) {
    let rx = config::subscribe();
    let mut prev = Prev::default();
    let tools = ctx.pool().run(|| (on_path("nvidia-offload"), on_path("prime-run"))).await.unwrap_or_default();
    loop {
        let interval = read();
        let mut sections = ctx.pool().run(collect).await.unwrap_or_default();
        sections.extend(tools_output().await);
        let state = State { tools, ..sample(&mut prev, &sections, Instant::now()) };
        ctx.publish(store::Diff::Monitor(Box::new(state)));
        idle(interval, &rx, &interval, read).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sections(stat: &str) -> HashMap<String, String> {
        split_sections(&format!("@stat\n{stat}\n@mem\nMemTotal: 1000 kB\nMemAvailable: 250 kB\n@net\n@drm\n@end"))
    }

    #[test]
    fn the_first_tick_has_no_cpu_figure_and_the_second_does() {
        let mut prev = Prev::default();
        let at = Instant::now();
        let first = sample(&mut prev, &sections("cpu 100 0 100 800 0 0 0 0"), at);
        assert!(first.cpu.available && first.cpu.aggregate.is_none());
        let second = sample(&mut prev, &sections("cpu 200 0 200 1600 0 0 0 0"), at);
        let busy = second.cpu.aggregate.expect("a delta");
        assert!((busy - 0.2).abs() < 1e-9, "{busy}");
        assert!((second.mem.expect("mem").used_fraction - 0.75).abs() < 1e-9);
    }

    #[test]
    fn no_card_is_the_honest_empty_answer() {
        let reply = State::default().gpu(0);
        assert_eq!(reply["available"], json!(false));
        assert_eq!(reply["cards"], json!([]));
    }
}
