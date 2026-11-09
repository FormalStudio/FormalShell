//! System monitor data: the
//! /proc and /sys reads of one tick happen on the blocking pool, the
//! fs-system parsers turn them into numbers, and CPU and network figures
//! are deltas against the tick before, so the first tick after a start
//! answers `null` rather than a made-up zero. The collector is the one
//! script fs-system ships, so the sections the parsers read are its own.

use std::collections::HashMap;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use fs_system::monitor::collect::{collect_command, split_sections};
use fs_system::monitor::gpu::{self, GfxMode, GpuRecord};
use fs_system::monitor::sysinfo::{self, CoreLoad, CpuRecord, Disk, Fan, Load, Mem, NetRate, NetRow, Temp, Uptime};
use serde_json::{Value, json};

use super::{changed, idle, settings};
use crate::runtime::Ctx;
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

pub fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64)
}

/// Whether `name` is an executable on PATH, for the offload tools gpu.js
/// asks about.
pub fn on_path(name: &str) -> bool {
    std::env::var_os("PATH").is_some_and(|paths| std::env::split_paths(&paths).any(|dir| dir.join(name).is_file()))
}

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
        sampled_ms: Some(now_ms()),
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

/// One collector run, on a pool thread: it walks /proc, /sys and `df`.
fn collect() -> HashMap<String, String> {
    let argv = collect_command();
    let out = crate::services::proc::std_command(&argv[0])
        .args(&argv[1..])
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output();
    out.map_or_else(|_| HashMap::new(), |o| split_sections(&String::from_utf8_lossy(&o.stdout)))
}

fn read() -> std::time::Duration {
    let ms = settings().interval("monitor.intervalMs", 2000).as_millis().max(500);
    std::time::Duration::from_millis(ms as u64)
}

pub async fn run(ctx: Ctx) {
    let rx = changed();
    let mut prev = Prev::default();
    let tools = ctx.pool().run(|| (on_path("nvidia-offload"), on_path("prime-run"))).await.unwrap_or_default();
    loop {
        let interval = read();
        let sections = ctx.pool().run(collect).await.unwrap_or_default();
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
