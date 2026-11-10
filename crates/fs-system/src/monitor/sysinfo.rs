//! Pure parsers for the system monitor: each function takes one
//! `collect` section's text and returns a plain value.
//!
//! Every fraction returned is 0..1, never 0..100. A missing or empty section
//! is an empty vector or `None`, never a panic: the collector runs on
//! machines with no hwmon, no interfaces beyond loopback, or a swapless VM,
//! and all of that is a normal state to render.
//!
//! Delta functions (`cpu_delta`, `net_delta`) need two samples to mean
//! anything. Called with no previous sample they return `None`, never a
//! fabricated 0, which would read as "measured zero load" instead of "no
//! measurement yet".

use fs_js as js;
use regex::Regex;
use std::collections::HashMap;
use std::sync::LazyLock;

fn lines(text: &str) -> impl Iterator<Item = &str> {
    text.split('\n').filter(|l| !l.is_empty())
}

/// One record per `cpu`/`cpuN` line. `total` follows the common
/// user+nice+system+idle+iowait+irq+softirq+steal formula (guest/guest_nice
/// are already folded into user/nice by the kernel, so adding them again
/// would double-count); `idle_all` is idle+iowait.
#[derive(Debug, Clone, PartialEq)]
pub struct CpuRecord {
    pub label: String,
    pub user: f64,
    pub nice: f64,
    pub system: f64,
    pub idle: f64,
    pub iowait: f64,
    pub irq: f64,
    pub softirq: f64,
    pub steal: f64,
    pub guest: f64,
    pub guest_nice: f64,
    pub idle_all: f64,
    pub total: f64,
}

static STAT_LINE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^(cpu\d*)\s+(.*)$").unwrap());

pub fn parse_stat(text: &str) -> Vec<CpuRecord> {
    let mut records = Vec::new();
    for line in lines(text) {
        let Some(m) = STAT_LINE.captures(line) else { continue };
        let fields: Vec<f64> = js::split_ws(&m[2]).into_iter().map(js::parse_number).collect();
        if fields.len() < 4 {
            continue;
        }
        let f = |i: usize| fields.get(i).copied().filter(|v| !v.is_nan()).unwrap_or(0.0);
        let (user, nice, system, idle) = (f(0), f(1), f(2), f(3));
        let (iowait, irq, softirq, steal) = (f(4), f(5), f(6), f(7));
        records.push(CpuRecord {
            label: m[1].to_string(),
            user,
            nice,
            system,
            idle,
            iowait,
            irq,
            softirq,
            steal,
            guest: f(8),
            guest_nice: f(9),
            idle_all: idle + iowait,
            total: user + nice + system + idle + iowait + irq + softirq + steal,
        });
    }
    records
}

/// A total delta of zero or less (no time elapsed, or a counter reset) has
/// nothing to divide by, so it means "unusable" rather than "0% busy".
fn busy_fraction(prev: &CpuRecord, next: &CpuRecord) -> Option<f64> {
    let total_delta = next.total - prev.total;
    if !total_delta.is_finite() || total_delta <= 0.0 {
        return None;
    }
    let busy_delta = total_delta - (next.idle_all - prev.idle_all);
    Some((busy_delta / total_delta).clamp(0.0, 1.0))
}

#[derive(Debug, Clone, PartialEq)]
pub struct CoreLoad {
    pub label: String,
    pub fraction: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CpuDelta {
    pub aggregate: f64,
    pub cores: Vec<CoreLoad>,
}

/// Aggregate and per-core busy fractions between two `parse_stat` samples.
/// Cores keep `next`'s own line order. A core present in `next` but not
/// `prev` (hot-plugged since the last tick) is left out of `cores` instead
/// of reporting a fabricated fraction for it.
pub fn cpu_delta(prev: Option<&[CpuRecord]>, next: Option<&[CpuRecord]>) -> Option<CpuDelta> {
    let (prev, next) = (prev?, next?);
    if prev.is_empty() || next.is_empty() {
        return None;
    }
    let by_label: HashMap<&str, &CpuRecord> = prev.iter().map(|r| (r.label.as_str(), r)).collect();
    let agg_prev = by_label.get("cpu")?;
    let agg_next = next.iter().find(|r| r.label == "cpu")?;
    let aggregate = busy_fraction(agg_prev, agg_next)?;

    let cores = next
        .iter()
        .filter(|r| r.label != "cpu")
        .filter_map(|rec| {
            let prev_rec = by_label.get(rec.label.as_str())?;
            let fraction = busy_fraction(prev_rec, rec)?;
            Some(CoreLoad { label: rec.label.clone(), fraction })
        })
        .collect();
    Some(CpuDelta { aggregate, cores })
}

/// Bytes throughout (the section carries kB), plus a used fraction derived
/// from MemAvailable, the kernel's own estimate of reclaimable memory,
/// closer to "what a user would call used" than MemTotal-MemFree.
#[derive(Debug, Clone, PartialEq)]
pub struct Mem {
    pub total_bytes: f64,
    pub available_bytes: f64,
    pub free_bytes: f64,
    pub swap_total_bytes: f64,
    pub swap_free_bytes: f64,
    pub used_fraction: f64,
}

static MEM_LINE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^(\w+):\s+(\d+)\s*kB$").unwrap());

/// `None` when the section carries no MemTotal.
pub fn parse_mem(text: &str) -> Option<Mem> {
    let mut kb: HashMap<String, f64> = HashMap::new();
    for line in lines(text) {
        if let Some(m) = MEM_LINE.captures(line) {
            kb.insert(m[1].to_string(), js::parse_number(&m[2]));
        }
    }
    let total = *kb.get("MemTotal")? * 1024.0;
    let get = |k: &str| kb.get(k).copied().unwrap_or(0.0);
    let available = kb.get("MemAvailable").copied().unwrap_or_else(|| get("MemFree")) * 1024.0;
    Some(Mem {
        total_bytes: total,
        available_bytes: available,
        free_bytes: get("MemFree") * 1024.0,
        swap_total_bytes: get("SwapTotal") * 1024.0,
        swap_free_bytes: get("SwapFree") * 1024.0,
        used_fraction: if total > 0.0 { ((total - available) / total).clamp(0.0, 1.0) } else { 0.0 },
    })
}

#[derive(Debug, Clone, PartialEq)]
pub struct Load {
    pub load1: f64,
    pub load5: f64,
    pub load15: f64,
    pub running_procs: f64,
    pub total_procs: f64,
    pub last_pid: f64,
}

static LOAD_LINE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^([\d.]+)\s+([\d.]+)\s+([\d.]+)\s+(\d+)/(\d+)\s+(\d+)").unwrap());

pub fn parse_load(text: &str) -> Option<Load> {
    let line = lines(text).next()?;
    let m = LOAD_LINE.captures(js::trim(line))?;
    let n = |i: usize| js::parse_number(&m[i]);
    Some(Load {
        load1: n(1),
        load5: n(2),
        load15: n(3),
        running_procs: n(4),
        total_procs: n(5),
        last_pid: n(6),
    })
}

#[derive(Debug, Clone, PartialEq)]
pub struct Uptime {
    pub uptime_seconds: f64,
    pub idle_seconds: f64,
}

pub fn parse_uptime(text: &str) -> Option<Uptime> {
    let line = lines(text).next()?;
    let parts = js::split_ws(line);
    if parts.len() < 2 {
        return None;
    }
    let (up, idle) = (js::parse_number(parts[0]), js::parse_number(parts[1]));
    (up.is_finite() && idle.is_finite()).then_some(Uptime { uptime_seconds: up, idle_seconds: idle })
}

/// Rx/tx byte counters per interface; everything else /proc/net/dev carries
/// is dropped at the parse boundary.
#[derive(Debug, Clone, PartialEq)]
pub struct NetRow {
    pub iface: String,
    pub rx_bytes: f64,
    pub tx_bytes: f64,
}

/// One row per interface, `lo` excluded: loopback traffic is never what a
/// system monitor's network row means.
pub fn parse_net(text: &str) -> Vec<NetRow> {
    let mut rows = Vec::new();
    for line in lines(text) {
        let Some(colon) = line.find(':') else { continue };
        let iface = js::trim(&line[..colon]);
        if iface.is_empty() || iface == "lo" {
            continue;
        }
        let fields: Vec<f64> = js::split_ws(&line[colon + 1..]).into_iter().map(js::parse_number).collect();
        if fields.len() < 9 {
            continue;
        }
        let f = |i: usize| if fields[i].is_nan() { 0.0 } else { fields[i] };
        rows.push(NetRow { iface: iface.to_string(), rx_bytes: f(0), tx_bytes: f(8) });
    }
    rows
}

#[derive(Debug, Clone, PartialEq)]
pub struct NetRate {
    pub iface: String,
    pub rx_bytes_per_sec: f64,
    pub tx_bytes_per_sec: f64,
}

/// Bytes/sec per interface between two `parse_net` samples, matched by
/// interface name. An interface missing from `prev` (came up since the last
/// tick) or whose counters went backwards (reset) is left out of the result
/// rather than reporting a rate that was never actually measured.
pub fn net_delta(prev: Option<&[NetRow]>, next: Option<&[NetRow]>, elapsed_ms: f64) -> Option<Vec<NetRate>> {
    let (prev, next) = (prev?, next?);
    if !elapsed_ms.is_finite() || elapsed_ms <= 0.0 {
        return None;
    }
    let by_iface: HashMap<&str, &NetRow> = prev.iter().map(|r| (r.iface.as_str(), r)).collect();
    let seconds = elapsed_ms / 1000.0;
    Some(
        next.iter()
            .filter_map(|n| {
                let p = by_iface.get(n.iface.as_str())?;
                let rx = n.rx_bytes - p.rx_bytes;
                let tx = n.tx_bytes - p.tx_bytes;
                (rx >= 0.0 && tx >= 0.0).then(|| NetRate {
                    iface: n.iface.clone(),
                    rx_bytes_per_sec: rx / seconds,
                    tx_bytes_per_sec: tx / seconds,
                })
            })
            .collect(),
    )
}

#[derive(Debug, Clone, PartialEq)]
pub struct Temp {
    pub chip: String,
    pub id: String,
    pub label: String,
    pub celsius: f64,
}

/// One row per `chip|file|label|millidegrees` collector line. `label` falls
/// back to the chip name when the kernel exposes no *_label file for that
/// sensor; the chip name is kept on the row either way so the UI can group
/// sensors that share it.
pub fn parse_temps(text: &str) -> Vec<Temp> {
    let mut rows = Vec::new();
    for line in lines(text) {
        let fields: Vec<&str> = line.split('|').collect();
        if fields.len() < 4 {
            continue;
        }
        let milli = js::parse_number(fields[3]);
        if !milli.is_finite() {
            continue;
        }
        let chip = fields[0];
        rows.push(Temp {
            chip: chip.to_string(),
            id: fields[1].to_string(),
            label: if fields[2].is_empty() { chip } else { fields[2] }.to_string(),
            celsius: milli / 1000.0,
        });
    }
    rows
}

#[derive(Debug, Clone, PartialEq)]
pub struct Fan {
    pub chip: String,
    pub id: String,
    pub label: String,
    pub rpm: f64,
}

/// One row per `chip|file|label|rpm` collector line, the same four-field
/// shape `parse_temps` reads.
///
/// A reading of 0 is a row, not a dropped one: a fan the firmware has spun
/// down is a fact about the machine, and hiding it would make a stopped fan
/// and an absent one look the same. An empty reading is rejected before the
/// number check, since an empty string parses as 0 and an unreadable
/// tachometer would render as a stopped one.
pub fn parse_fans(text: &str) -> Vec<Fan> {
    let mut rows = Vec::new();
    for line in lines(text) {
        let fields: Vec<&str> = line.split('|').collect();
        if fields.len() < 4 || js::trim(fields[3]).is_empty() {
            continue;
        }
        let rpm = js::parse_number(fields[3]);
        if !rpm.is_finite() {
            continue;
        }
        let chip = fields[0];
        rows.push(Fan {
            chip: chip.to_string(),
            id: fields[1].to_string(),
            label: if fields[2].is_empty() { chip } else { fields[2] }.to_string(),
            rpm,
        });
    }
    rows
}

#[derive(Debug, Clone, PartialEq)]
pub struct Disk {
    pub source: String,
    pub mount: String,
    pub size: f64,
    pub used: f64,
    pub fraction: f64,
}

static DISK_LINE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(\S+)\s+(.+?)\s+(\d+)\s+(\d+)$").unwrap());

/// `df --output=source,target,size,used` rows, already in bytes. `fraction`
/// is used/size, clamped, 0 for a zero-size target.
///
/// Deduplicated by `source`: a NixOS machine's overlay/bind mounts put the
/// same device under several mount points. Identity is the device string,
/// never size, since two distinct filesystems can report the same size (9p
/// mounts sharing a host directory's free space do). The row kept for a
/// repeated source is the one with the shortest mount path, the one a human
/// recognises (`/` over `/nix/store`).
pub fn parse_disk(text: &str) -> Vec<Disk> {
    let mut by_source: HashMap<String, Disk> = HashMap::new();
    let mut order: Vec<String> = Vec::new();
    for line in lines(text) {
        let Some(m) = DISK_LINE.captures(js::trim(line)) else { continue };
        let size = js::parse_number(&m[3]);
        let used = js::parse_number(&m[4]);
        let row = Disk {
            source: m[1].to_string(),
            mount: m[2].to_string(),
            size,
            used,
            fraction: if size > 0.0 { (used / size).clamp(0.0, 1.0) } else { 0.0 },
        };
        match by_source.get(&row.source) {
            None => {
                order.push(row.source.clone());
                by_source.insert(row.source.clone(), row);
            }
            // Length counts UTF-16 units.
            Some(existing) if row.mount.encode_utf16().count() < existing.mount.encode_utf16().count() => {
                by_source.insert(row.source.clone(), row);
            }
            Some(_) => {}
        }
    }
    order.into_iter().filter_map(|s| by_source.remove(&s)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::monitor::collect::split_sections;

    fn g815() -> HashMap<String, String> {
        split_sections(include_str!("../../tests/fixtures/monitor-g815.txt"))
    }

    fn vm() -> HashMap<String, String> {
        split_sections(include_str!("../../tests/fixtures/monitor-vm.txt"))
    }

    fn close(a: f64, b: f64) {
        assert!((a - b).abs() < 0.0001, "{a} vs {b}");
    }

    // split_sections

    #[test]
    fn split_sections_finds_every_marker_on_g815() {
        let g = g815();
        for name in ["stat", "mem", "load", "uptime", "net", "temp", "fan", "disk", "drm", "nvidia", "gfx", "end"] {
            assert!(g.contains_key(name), "{name} missing");
        }
    }

    #[test]
    fn split_sections_leaves_an_absent_tool_section_empty() {
        assert_eq!(g815()["gfx"], "");
    }

    #[test]
    fn split_sections_leaves_every_gpu_and_temp_section_empty_on_the_vm() {
        let v = vm();
        for name in ["temp", "fan", "drm", "nvidia", "gfx"] {
            assert_eq!(v[name], "", "{name}");
        }
    }

    #[test]
    fn split_sections_keeps_bodies_out_of_neighboring_sections() {
        let g = g815();
        assert!(g["stat"].contains("cpu0"));
        assert!(!g["stat"].contains("@mem"));
        assert!(g["mem"].contains("MemTotal"));
    }

    // parse_stat / cpu_delta

    #[test]
    fn parse_stat_reads_the_aggregate_and_per_core_lines() {
        let records = parse_stat(&g815()["stat"]);
        assert_eq!(records.len(), 5);
        assert_eq!(records[0].label, "cpu");
        assert_eq!(records[1].label, "cpu0");
        assert_eq!(records[0].idle, 15406530.0);
    }

    #[test]
    fn parse_stat_on_a_missing_section_is_an_empty_array() {
        assert_eq!(parse_stat("").len(), 0);
        assert_eq!(parse_stat(&vm()["drm"]).len(), 0);
    }

    #[test]
    fn cpu_delta_is_none_with_no_previous_sample() {
        let next = parse_stat(&g815()["stat"]);
        assert_eq!(cpu_delta(None, Some(&next)), None);
        assert_eq!(cpu_delta(Some(&[]), Some(&next)), None);
    }

    #[test]
    fn cpu_delta_aggregate_and_per_core_fractions_are_0_to_1() {
        let prev = parse_stat("cpu 100 0 100 700 0 0 0 0 0 0\ncpu0 50 0 50 350 0 0 0 0 0 0\ncpu1 50 0 50 350 0 0 0 0 0 0");
        let next = parse_stat("cpu 200 0 200 800 0 0 0 0 0 0\ncpu0 120 0 60 370 0 0 0 0 0 0\ncpu1 80 0 140 430 0 0 0 0 0 0");
        let delta = cpu_delta(Some(&prev), Some(&next)).unwrap();
        close(delta.aggregate, 200.0 / 300.0);
        assert_eq!(delta.cores.len(), 2);
        assert_eq!(delta.cores[0].label, "cpu0");
        assert_eq!(delta.cores[0].fraction, 0.8);
        assert_eq!(delta.cores[1].label, "cpu1");
        assert_eq!(delta.cores[1].fraction, 0.6);
        assert!((0.0..=1.0).contains(&delta.aggregate));
        assert!(delta.cores.iter().all(|c| (0.0..=1.0).contains(&c.fraction)));
    }

    #[test]
    fn cpu_delta_is_none_when_the_total_did_not_advance() {
        let same = parse_stat("cpu 100 0 100 700 0 0 0 0 0 0");
        assert_eq!(cpu_delta(Some(&same), Some(&same)), None);
    }

    // parse_mem

    #[test]
    fn parse_mem_g815_real_numbers() {
        let mem = parse_mem(&g815()["mem"]).unwrap();
        assert_eq!(mem.total_bytes, 32191112.0 * 1024.0);
        assert_eq!(mem.available_bytes, 18139792.0 * 1024.0);
        assert_eq!(mem.free_bytes, 9172100.0 * 1024.0);
        assert_eq!(mem.swap_total_bytes, 16095228.0 * 1024.0);
        assert_eq!(mem.swap_free_bytes, 16090556.0 * 1024.0);
        close(mem.used_fraction, (32191112.0 - 18139792.0) / 32191112.0);
    }

    #[test]
    fn parse_mem_vm_is_swapless() {
        let mem = parse_mem(&vm()["mem"]).unwrap();
        assert_eq!(mem.swap_total_bytes, 0.0);
        assert_eq!(mem.swap_free_bytes, 0.0);
        assert!((0.0..=1.0).contains(&mem.used_fraction));
    }

    #[test]
    fn parse_mem_on_a_missing_section_is_unavailable() {
        assert_eq!(parse_mem(""), None);
    }

    // parse_load / parse_uptime

    #[test]
    fn parse_load_g815() {
        let load = parse_load(&g815()["load"]).unwrap();
        assert_eq!(load.load1, 1.50);
        assert_eq!(load.load5, 1.05);
        assert_eq!(load.load15, 1.40);
        assert_eq!(load.running_procs, 4.0);
        assert_eq!(load.total_procs, 2006.0);
        assert_eq!(load.last_pid, 103726.0);
    }

    #[test]
    fn parse_load_on_a_missing_section_is_unavailable() {
        assert_eq!(parse_load(""), None);
    }

    #[test]
    fn parse_uptime_g815() {
        let uptime = parse_uptime(&g815()["uptime"]).unwrap();
        assert_eq!(uptime.uptime_seconds, 6850.34);
        assert_eq!(uptime.idle_seconds, 154065.38);
    }

    #[test]
    fn parse_uptime_on_a_missing_section_is_unavailable() {
        assert_eq!(parse_uptime(""), None);
    }

    // parse_net / net_delta

    #[test]
    fn parse_net_excludes_loopback() {
        let rows = parse_net(&g815()["net"]);
        assert_eq!(rows.len(), 2);
        assert!(rows.iter().all(|r| r.iface != "lo"));
    }

    #[test]
    fn parse_net_reads_rx_tx_bytes() {
        let rows = parse_net(&g815()["net"]);
        let wifi = rows.iter().find(|r| r.iface == "wlp129s0f0").unwrap();
        assert_eq!(wifi.rx_bytes, 3306453649.0);
        assert_eq!(wifi.tx_bytes, 351143584.0);
    }

    #[test]
    fn parse_net_on_a_missing_section_is_an_empty_array() {
        assert_eq!(parse_net("").len(), 0);
        assert_eq!(parse_net(&vm()["drm"]).len(), 0);
    }

    #[test]
    fn net_delta_is_none_with_no_previous_sample_or_no_elapsed_time() {
        let next = parse_net(&g815()["net"]);
        assert_eq!(net_delta(None, Some(&next), 1000.0), None);
        assert_eq!(net_delta(Some(&next), Some(&next), 0.0), None);
    }

    fn row(iface: &str, rx: f64, tx: f64) -> NetRow {
        NetRow { iface: iface.into(), rx_bytes: rx, tx_bytes: tx }
    }

    #[test]
    fn net_delta_computes_bytes_per_second() {
        let delta = net_delta(Some(&[row("eth0", 1000.0, 500.0)]), Some(&[row("eth0", 3000.0, 700.0)]), 2000.0).unwrap();
        assert_eq!(delta.len(), 1);
        assert_eq!(delta[0].iface, "eth0");
        assert_eq!(delta[0].rx_bytes_per_sec, 1000.0);
        assert_eq!(delta[0].tx_bytes_per_sec, 100.0);
    }

    #[test]
    fn net_delta_skips_an_interface_whose_counters_went_backwards() {
        let delta = net_delta(Some(&[row("eth0", 5000.0, 500.0)]), Some(&[row("eth0", 100.0, 700.0)]), 1000.0).unwrap();
        assert_eq!(delta.len(), 0);
    }

    #[test]
    fn net_delta_skips_an_interface_absent_from_the_previous_sample() {
        let delta = net_delta(Some(&[]), Some(&[row("eth0", 100.0, 100.0)]), 1000.0).unwrap();
        assert_eq!(delta.len(), 0);
    }

    // parse_temps

    #[test]
    fn parse_temps_converts_millidegrees_and_keeps_the_chip_name() {
        let rows = parse_temps(&g815()["temp"]);
        let pkg = rows.iter().find(|r| r.chip == "coretemp" && r.id == "temp1_input").unwrap();
        assert_eq!(pkg.celsius, 62.0);
        assert_eq!(pkg.label, "Package id 0");
    }

    #[test]
    fn parse_temps_falls_back_to_the_chip_name_when_the_label_is_empty() {
        let rows = parse_temps(&g815()["temp"]);
        let wifi = rows.iter().find(|r| r.chip == "iwlwifi_1_6").unwrap();
        assert_eq!(wifi.label, "iwlwifi_1_6");
        assert_eq!(wifi.celsius, 56.0);
        let acpi = rows.iter().find(|r| r.chip == "acpitz_0").unwrap();
        assert_eq!(acpi.label, "acpitz_0");
    }

    #[test]
    fn parse_temps_on_the_vms_hwmonless_fixture_is_empty() {
        assert_eq!(parse_temps(&vm()["temp"]).len(), 0);
    }

    // parse_fans

    #[test]
    fn parse_fans_reads_rpm_and_keeps_the_chip_name() {
        let rows = parse_fans(&g815()["fan"]);
        assert_eq!(rows.len(), 4);
        let cpu_fan = rows.iter().find(|r| r.chip == "asus" && r.id == "fan1_input").unwrap();
        assert_eq!(cpu_fan.label, "cpu_fan");
        assert_eq!(cpu_fan.rpm, 2500.0);
    }

    #[test]
    fn parse_fans_falls_back_to_the_chip_name_when_the_label_is_empty() {
        let rows = parse_fans(&g815()["fan"]);
        let acpi = rows.iter().find(|r| r.chip == "acpi_fan").unwrap();
        assert_eq!(acpi.label, "acpi_fan");
        assert_eq!(acpi.rpm, 2580.0);
    }

    #[test]
    fn parse_fans_keeps_a_stopped_fan_as_a_zero_row() {
        let rows = parse_fans("asus|fan1_input|cpu_fan|3400\nasus|fan2_input|gpu_fan|0");
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[1].label, "gpu_fan");
        assert_eq!(rows[1].rpm, 0.0);
    }

    #[test]
    fn parse_fans_drops_a_row_whose_reading_is_not_a_number() {
        let rows = parse_fans("asus|fan1_input|cpu_fan|2500\nasus|fan2_input|gpu_fan|\nshort|row");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].rpm, 2500.0);
    }

    #[test]
    fn parse_fans_on_the_vms_hwmonless_fixture_is_empty() {
        assert_eq!(parse_fans(&vm()["fan"]).len(), 0);
    }

    // parse_disk

    #[test]
    fn parse_disk_reads_source_mount_size_used_and_fraction() {
        let rows = parse_disk(&g815()["disk"]);
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].source, "/dev/nvme0n1p5");
        assert_eq!(rows[0].mount, "/");
        assert_eq!(rows[0].size, 269427478528.0);
        assert_eq!(rows[0].used, 164616077312.0);
        close(rows[0].fraction, 164616077312.0 / 269427478528.0);
        assert!((0.0..=1.0).contains(&rows[0].fraction));
        assert_eq!(rows[1].source, "/dev/nvme0n1p1");
        assert_eq!(rows[1].mount, "/boot");
        assert_eq!(rows[2].source, "macbook:");
        assert_eq!(rows[2].mount, "/home/kyandesutter/.macbook");
    }

    #[test]
    fn parse_disk_on_a_missing_section_is_an_empty_array() {
        assert_eq!(parse_disk("").len(), 0);
    }

    #[test]
    fn parse_disk_keeps_every_row_when_all_sources_differ() {
        let rows = parse_disk(&vm()["disk"]);
        assert_eq!(rows.len(), 7);
        let sources: Vec<&str> = rows.iter().map(|r| r.source.as_str()).collect();
        assert_eq!(sources, ["/dev/vda", "certs", "shared", "xchg", "/dev/vdb", "keys", "overlay"]);
    }

    #[test]
    fn parse_disk_dedupes_a_repeated_source_keeping_the_shortest_mount() {
        let rows = parse_disk("/dev/sda1 /mnt/data/nested 1000 500\n/dev/sda1 /data 1000 500\n/dev/sda2 /var 2000 1000");
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].source, "/dev/sda1");
        assert_eq!(rows[0].mount, "/data");
        assert_eq!(rows[1].source, "/dev/sda2");
        assert_eq!(rows[1].mount, "/var");
    }

    #[test]
    fn parse_disk_dedupe_does_not_depend_on_row_order() {
        let rows = parse_disk("/dev/sda1 /data 1000 500\n/dev/sda1 /mnt/data/nested 1000 500");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].mount, "/data");
    }

    #[test]
    fn parse_disk_size_alone_never_merges_distinct_sources() {
        let rows = parse_disk("certs /etc/ssl/certs 1000 500\nshared /tmp/shared 1000 500");
        assert_eq!(rows.len(), 2);
    }

    #[test]
    fn every_parser_survives_the_vms_gpu_and_temp_sections_being_empty() {
        let v = vm();
        assert_eq!(parse_temps(&v["temp"]).len(), 0);
        assert!(!parse_stat(&v["stat"]).is_empty());
        assert!(parse_mem(&v["mem"]).is_some());
        assert!(parse_load(&v["load"]).is_some());
        assert!(parse_uptime(&v["uptime"]).is_some());
        assert!(!parse_net(&v["net"]).is_empty());
        assert!(!parse_disk(&v["disk"]).is_empty());
    }
}
