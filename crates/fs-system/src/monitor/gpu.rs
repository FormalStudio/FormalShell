//! GPU parsing for the system monitor and dGPU-offload launch. The collector's
//! `@drm`/`@nvidia`/`@gfx` sections go in, records come out.
//!
//! Card ids (`card0`, `card1`, ...) are opaque strings end to end: the
//! collector's own numbering does not imply which GPU is primary. `boot_vga`
//! is the only signal for that (see `is_discrete`). The owner's g815 fixture
//! pins this: the dGPU enumerates as card0 with boot_vga=0, the iGPU as card1
//! with boot_vga=1.

use fs_js as js;
use std::collections::HashMap;

/// Desktop-entry Exec field codes, per the freedesktop spec. `%%` is a
/// literal percent and is not one of them.
const FIELD_CODES: &str = "fFuUickdDnNvm";

// ---- @drm section --------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub struct Output {
    pub name: String,
    pub connected: bool,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Card {
    pub card: String,
    pub driver: String,
    pub vendor_id: String,
    pub device_id: String,
    pub boot_vga: String,
    pub pci: String,
    pub label: String,
    pub outputs: Vec<Output>,
}

fn part<'a>(parts: &[&'a str], i: usize) -> &'a str {
    parts.get(i).copied().unwrap_or("")
}

/// `card|<card>|<driver>|<vendor>|<device>|<boot_vga>|<pci>|<label>` and
/// `conn|<card>|<connector>|<status>` rows, in collector emission order.
/// `metric|` rows are `parse_metrics`'s concern.
pub fn parse_cards(drm_text: &str) -> Vec<Card> {
    let mut cards: Vec<Card> = Vec::new();
    let mut by_id: HashMap<String, usize> = HashMap::new();

    for line in drm_text.split('\n') {
        if line.is_empty() {
            continue;
        }
        let parts: Vec<&str> = line.split('|').collect();
        match parts[0] {
            "card" => {
                let card = Card {
                    card: part(&parts, 1).into(),
                    driver: part(&parts, 2).into(),
                    vendor_id: part(&parts, 3).into(),
                    device_id: part(&parts, 4).into(),
                    boot_vga: part(&parts, 5).into(),
                    pci: part(&parts, 6).into(),
                    label: part(&parts, 7).into(),
                    outputs: Vec::new(),
                };
                by_id.insert(card.card.clone(), cards.len());
                cards.push(card);
            }
            "conn" => {
                if let Some(&i) = by_id.get(part(&parts, 1)) {
                    cards[i].outputs.push(Output {
                        name: part(&parts, 2).into(),
                        connected: part(&parts, 3) == "connected",
                    });
                }
            }
            _ => {}
        }
    }
    cards
}

/// `{ "<card>": { "<key>": <number>, ... } }` from `metric|<card>|<key>|<value>`
/// rows. A row whose value doesn't parse as a number is dropped rather than
/// stored as a fabricated 0.
pub(crate) type CardMetrics = HashMap<String, HashMap<String, f64>>;

pub fn parse_metrics(drm_text: &str) -> CardMetrics {
    let mut metrics: CardMetrics = HashMap::new();
    for line in drm_text.split('\n') {
        if line.is_empty() {
            continue;
        }
        let parts: Vec<&str> = line.split('|').collect();
        if parts[0] != "metric" {
            continue;
        }
        let n = parts.get(3).map_or(f64::NAN, |p| js::parse_number(p));
        if !n.is_finite() {
            continue;
        }
        metrics
            .entry(part(&parts, 1).to_string())
            .or_default()
            .insert(part(&parts, 2).to_string(), n);
    }
    metrics
}

// ---- @nvidia section -------------------------------------------------------

fn num_or_none(field: &str) -> Option<f64> {
    if field == "[N/A]" || field == "N/A" {
        return None;
    }
    Some(js::parse_number(field)).filter(|n| n.is_finite())
}

/// One `nvidia-smi --query-gpu=index,name,utilization.gpu,temperature.gpu,
/// memory.used,memory.total,power.draw,fan.speed --format=csv,noheader,
/// nounits` row. `utilization` is a 0..1 fraction; `mem_used`/`mem_total`
/// stay in the wire's MiB.
#[derive(Debug, Clone, PartialEq)]
pub struct NvidiaRow {
    pub index: i64,
    pub name: String,
    pub utilization: Option<f64>,
    pub temperature: Option<f64>,
    pub mem_used: Option<f64>,
    pub mem_total: Option<f64>,
    pub power: Option<f64>,
    pub fan: Option<f64>,
}

/// One GPU per line. `[N/A]` is a value nvidia-smi really emits (laptop GPUs
/// report no fan reading) and parses to `None`, never 0.
pub fn parse_nvidia(nvidia_text: &str) -> Vec<NvidiaRow> {
    let mut rows = Vec::new();
    for line in nvidia_text.split('\n') {
        let line = js::trim(line);
        if line.is_empty() {
            continue;
        }
        let fields: Vec<&str> = line.split(',').map(js::trim).collect();
        if fields.len() < 8 {
            continue;
        }
        let index = js::parse_int(fields[0]);
        rows.push(NvidiaRow {
            index: if index.is_nan() { 0 } else { index as i64 },
            name: fields[1].to_string(),
            utilization: num_or_none(fields[2]).map(|u| u / 100.0),
            temperature: num_or_none(fields[3]),
            mem_used: num_or_none(fields[4]),
            mem_total: num_or_none(fields[5]),
            power: num_or_none(fields[6]),
            fan: num_or_none(fields[7]),
        });
    }
    rows
}

// ---- merge ------------------------------------------------------------

/// Every metrics record carries the same keys whatever the driver, so a
/// consumer reads a field without first working out which branch built the
/// record; a reading this card has no source for is `None`. `available`
/// means "at least one of these is a real measurement": a surface gates its
/// whole metrics block on it, and a card with only a clock reading has
/// something to show.
///
/// `fan_percent` and `fan_rpm` are separate fields because they are separate
/// units: nvidia-smi reports a percent of maximum, the hwmon fan1_input every
/// other driver exposes is RPM off the tachometer.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Metrics {
    pub available: bool,
    pub busy: Option<f64>,
    pub temp_c: Option<f64>,
    pub vram_used: Option<f64>,
    pub vram_total: Option<f64>,
    pub power_w: Option<f64>,
    pub fan_percent: Option<f64>,
    pub fan_rpm: Option<f64>,
    pub clock_mhz: Option<f64>,
    pub clock_max_mhz: Option<f64>,
}

impl Metrics {
    fn from_fields(mut m: Metrics) -> Metrics {
        for slot in [
            &mut m.busy,
            &mut m.temp_c,
            &mut m.vram_used,
            &mut m.vram_total,
            &mut m.power_w,
            &mut m.fan_percent,
            &mut m.fan_rpm,
            &mut m.clock_mhz,
            &mut m.clock_max_mhz,
        ] {
            *slot = slot.filter(|v| v.is_finite());
        }
        m.available = [
            m.busy,
            m.temp_c,
            m.vram_used,
            m.vram_total,
            m.power_w,
            m.fan_percent,
            m.fan_rpm,
            m.clock_mhz,
            m.clock_max_mhz,
        ]
        .iter()
        .any(Option::is_some);
        m
    }
}

/// One `metric|` row's value, divided into the unit the record publishes.
/// An absent key is `None` rather than 0: the collector only emits a row for
/// a file it could actually read.
fn scaled(values: Option<&HashMap<String, f64>>, key: &str, divisor: f64) -> Option<f64> {
    values?.get(key).map(|v| v / divisor)
}

fn nvidia_metrics(row: &NvidiaRow) -> Metrics {
    Metrics::from_fields(Metrics {
        busy: row.utilization,
        temp_c: row.temperature,
        vram_used: row.mem_used.map(|v| v * 1024.0 * 1024.0),
        vram_total: row.mem_total.map(|v| v * 1024.0 * 1024.0),
        power_w: row.power,
        fan_percent: row.fan,
        ..Default::default()
    })
}

fn amd_metrics(values: Option<&HashMap<String, f64>>) -> Metrics {
    Metrics::from_fields(Metrics {
        busy: scaled(values, "gpu_busy_percent", 100.0),
        temp_c: scaled(values, "temp1_input", 1000.0),
        vram_used: scaled(values, "mem_info_vram_used", 1.0),
        vram_total: scaled(values, "mem_info_vram_total", 1.0),
        power_w: scaled(values, "power1_average", 1_000_000.0),
        fan_rpm: scaled(values, "fan1_input", 1.0),
        ..Default::default()
    })
}

/// Every driver that is neither nvidia nor amdgpu, i915 and xe among them.
/// Those publish no busy counter at all, so `busy` stays `None`; the clock
/// against its own ceiling is the load signal such a card does have, and the
/// hwmon block the collector walks for every card supplies temp/power/fan
/// wherever the driver registers one.
fn generic_metrics(values: Option<&HashMap<String, f64>>) -> Metrics {
    Metrics::from_fields(Metrics {
        temp_c: scaled(values, "temp1_input", 1000.0),
        power_w: scaled(values, "power1_average", 1_000_000.0),
        fan_rpm: scaled(values, "fan1_input", 1.0),
        clock_mhz: scaled(values, "gt_act_freq_mhz", 1.0),
        clock_max_mhz: scaled(values, "gt_max_freq_mhz", 1.0),
        ..Default::default()
    })
}

#[derive(Debug, Clone, PartialEq)]
pub struct GpuRecord {
    pub card: Card,
    pub metrics: Metrics,
}

/// Attaches `metrics` to each card. NVIDIA rows are matched to
/// `nvidia`-driver cards in enumeration order. amdgpu reads gpu_busy_percent
/// (0..100, divided) and mem_info_vram_* (bytes already). Every other driver
/// falls to `generic_metrics`: no unprivileged utilisation counter exists for
/// those, and inventing one would violate the honest-unavailable-state rule.
pub fn merge_gpu(cards: &[Card], metrics: &CardMetrics, nvidia_rows: &[NvidiaRow]) -> Vec<GpuRecord> {
    let mut nvidia_index = 0;
    cards
        .iter()
        .map(|card| {
            let m = match card.driver.as_str() {
                "nvidia" => {
                    let row = nvidia_rows.get(nvidia_index);
                    nvidia_index += 1;
                    row.map_or_else(|| Metrics::from_fields(Metrics::default()), nvidia_metrics)
                }
                "amdgpu" => amd_metrics(metrics.get(&card.card)),
                _ => generic_metrics(metrics.get(&card.card)),
            };
            GpuRecord { card: card.clone(), metrics: m }
        })
        .collect()
}

// ---- naming -------------------------------------------------------------

pub(crate) fn vendor_name(vendor_id: &str) -> &'static str {
    match vendor_id.to_lowercase().as_str() {
        "0x8086" => "Intel",
        "0x1002" | "0x1022" => "AMD",
        "0x10de" => "NVIDIA",
        "0x1af4" => "virtio",
        _ => "",
    }
}

/// nvidia-smi's marketing name when there is one, else the ACPI label
/// ("Onboard - Video" on Intel, empty on NVIDIA), else "<vendor> <deviceId>",
/// else the card id itself.
pub fn display_name(card: &Card, nvidia_row: Option<&NvidiaRow>) -> String {
    if let Some(row) = nvidia_row.filter(|r| !r.name.is_empty()) {
        return row.name.clone();
    }
    if !card.label.is_empty() {
        return card.label.clone();
    }
    let vendor = vendor_name(&card.vendor_id);
    if !vendor.is_empty() {
        return format!("{vendor} {}", card.device_id);
    }
    card.card.clone()
}

/// boot_vga is the only signal for which card is the integrated/primary one:
/// never the card number (the g815 fixture has the dGPU at card0).
pub fn is_discrete(card: &Card) -> bool {
    card.boot_vga != "1"
}

/// The card record driving `connector_name` (compositor output names, e.g.
/// "eDP-1", "HDMI-A-1", match connector names verbatim).
pub(crate) fn output_card<'a>(connector_name: &str, cards: &'a [Card]) -> Option<&'a Card> {
    cards.iter().find(|c| c.outputs.iter().any(|o| o.name == connector_name))
}

// ---- launch-on-dGPU ----------------------------------------------------------

/// Exec field codes (%f %F %u %U %i %c %k %d %D %n %N %v %m) removed and
/// whitespace collapsed. `%%` is a literal percent. One pass, so a literal
/// `%%` can never be re-read as the start of a field code.
pub(crate) fn strip_field_codes(exec_string: &str) -> String {
    let mut out = String::new();
    let mut chars = exec_string.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '%' {
            out.push(c);
            continue;
        }
        // `.` in the JS pattern skips line terminators.
        match chars.peek().copied() {
            Some(n) if !matches!(n, '\n' | '\r' | '\u{2028}' | '\u{2029}') => {
                chars.next();
                if n == '%' {
                    out.push('%');
                } else if !FIELD_CODES.contains(n) {
                    out.push('%');
                    out.push(n);
                }
            }
            _ => out.push('%'),
        }
    }
    js::split_ws(&out)
        .into_iter()
        .filter(|p| !p.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

/// Which offload helper is on PATH.
#[derive(Debug, Clone, Copy, Default)]
pub struct OffloadTools {
    pub nvidia_offload: bool,
    pub prime_run: bool,
}

/// The argv to spawn an app on `target`. A desktop entry's `execute()` cannot
/// carry an environment, so this builds the argv by hand.
///
/// NVIDIA: nvidia-offload (NixOS) if present, else prime-run (Arch), else the
/// exact four env vars NixOS's own nvidia-offload wrapper exports. Non-NVIDIA:
/// DRI_PRIME set to the card's PCI slot in Mesa's `pci-0000_02_00_0` form,
/// never the positional `DRI_PRIME=1`, ambiguous on a box with more than two
/// GPUs.
pub fn offload_argv(exec_string: &str, target: Option<&Card>, tools: OffloadTools) -> Vec<String> {
    let tail = ["sh".to_string(), "-c".to_string(), strip_field_codes(exec_string)];

    let mut head: Vec<String> = if target.is_some_and(|t| t.driver == "nvidia") {
        if tools.nvidia_offload {
            vec!["nvidia-offload".into()]
        } else if tools.prime_run {
            vec!["prime-run".into()]
        } else {
            [
                "env",
                "__NV_PRIME_RENDER_OFFLOAD=1",
                "__NV_PRIME_RENDER_OFFLOAD_PROVIDER=NVIDIA-G0",
                "__GLX_VENDOR_LIBRARY_NAME=nvidia",
                "__VK_LAYER_NV_optimus=NVIDIA_only",
            ]
            .map(String::from)
            .to_vec()
        }
    } else {
        let pci = target.map_or("", |t| t.pci.as_str()).replace([':', '.'], "_");
        vec!["env".into(), format!("DRI_PRIME=pci-{pci}")]
    };
    head.extend(tail);
    head
}

// ---- @gfx section -----------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub struct GfxMode {
    pub supported: bool,
    pub mode: String,
}

/// supergfxctl's `-g` reply. Empty output (no supergfxctl installed) means
/// unsupported, never a guessed "integrated".
pub fn parse_gfx_mode(gfx_text: &str) -> GfxMode {
    let text = js::trim(gfx_text);
    if text.is_empty() {
        return GfxMode { supported: false, mode: String::new() };
    }
    let text = text.strip_prefix('"').unwrap_or(text);
    let text = text.strip_suffix('"').unwrap_or(text);
    GfxMode { supported: true, mode: text.to_string() }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HYBRID: &str = include_str!("../../tests/fixtures/gpu-hybrid.txt");
    const SINGLE: &str = include_str!("../../tests/fixtures/gpu-single.txt");
    const NONE: &str = include_str!("../../tests/fixtures/gpu-none.txt");
    const AMD: &str = include_str!("../../tests/fixtures/gpu-amd.txt");
    const INTEL: &str = include_str!("../../tests/fixtures/gpu-intel-metrics.txt");

    /// The lines between a `@name` marker and the next `@` line.
    fn section(blob: &str, name: &str) -> String {
        let lines: Vec<&str> = blob.split('\n').collect();
        let marker = format!("@{name}");
        let Some(start) = lines.iter().position(|l| *l == marker) else {
            return String::new();
        };
        lines[start + 1..].iter().take_while(|l| !l.starts_with('@')).copied().collect::<Vec<_>>().join("\n")
    }

    fn merged(blob: &str) -> Vec<GpuRecord> {
        let drm = section(blob, "drm");
        merge_gpu(&parse_cards(&drm), &parse_metrics(&drm), &parse_nvidia(&section(blob, "nvidia")))
    }

    #[test]
    fn hybrid_yields_two_cards_dgpu_first_by_enumeration() {
        let cards = parse_cards(&section(HYBRID, "drm"));
        assert_eq!(cards.len(), 2);
        assert_eq!(cards[0].card, "card0");
        assert_eq!(cards[0].driver, "nvidia");
        assert_eq!(cards[1].card, "card1");
        assert_eq!(cards[1].driver, "i915");
    }

    #[test]
    fn hybrid_card_numbering_is_not_primacy() {
        let cards = parse_cards(&section(HYBRID, "drm"));
        assert!(is_discrete(&cards[0]));
        assert!(!is_discrete(&cards[1]));
    }

    #[test]
    fn hybrid_connectors_fold_onto_their_card() {
        let cards = parse_cards(&section(HYBRID, "drm"));
        assert_eq!(cards[0].outputs.len(), 3);
        assert_eq!(cards[0].outputs[2].name, "HDMI-A-1");
        assert!(cards[0].outputs[2].connected);
        assert_eq!(cards[1].outputs.len(), 3);
        assert_eq!(cards[1].outputs[2].name, "eDP-1");
        assert!(cards[1].outputs[2].connected);
        assert!(!cards[1].outputs[0].connected);
    }

    #[test]
    fn single_fixture_has_no_card0() {
        let cards = parse_cards(&section(SINGLE, "drm"));
        assert_eq!(cards.len(), 1);
        assert_eq!(cards[0].card, "card1");
        assert!(!is_discrete(&cards[0]));
    }

    #[test]
    fn none_fixture_yields_zero_cards_and_does_not_panic() {
        let cards = parse_cards(&section(NONE, "drm"));
        assert_eq!(cards.len(), 0);
        let nvidia = parse_nvidia(&section(NONE, "nvidia"));
        assert_eq!(nvidia.len(), 0);
        assert!(!parse_gfx_mode(&section(NONE, "gfx")).supported);
        let m = merge_gpu(&cards, &parse_metrics(&section(NONE, "drm")), &nvidia);
        assert_eq!(m.len(), 0);
    }

    #[test]
    fn nvidia_row_parses_g815_fixture() {
        let rows = parse_nvidia(&section(HYBRID, "nvidia"));
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].index, 0);
        assert_eq!(rows[0].name, "NVIDIA GeForce RTX 5070 Laptop GPU");
        assert_eq!(rows[0].temperature, Some(50.0));
        assert_eq!(rows[0].power, Some(12.17));
    }

    #[test]
    fn nvidia_utilization_16_becomes_fraction_016() {
        assert_eq!(parse_nvidia(&section(HYBRID, "nvidia"))[0].utilization, Some(0.16));
    }

    #[test]
    fn nvidia_na_fan_is_none_not_zero() {
        assert_eq!(parse_nvidia(&section(HYBRID, "nvidia"))[0].fan, None);
    }

    #[test]
    fn merge_attaches_nvidia_metrics_in_the_right_units() {
        let m = merged(HYBRID);
        assert_eq!(m[0].card.card, "card0");
        let metrics = &m[0].metrics;
        assert!(metrics.available);
        assert_eq!(metrics.busy, Some(0.16));
        assert_eq!(metrics.temp_c, Some(50.0));
        assert_eq!(metrics.power_w, Some(12.17));
        assert_eq!(metrics.fan_percent, None);
        assert_eq!(metrics.vram_used, Some(78.0 * 1024.0 * 1024.0));
        assert_eq!(metrics.vram_total, Some(8151.0 * 1024.0 * 1024.0));
    }

    #[test]
    fn merge_i915_card_with_no_metric_rows_reports_unavailable() {
        let m = merged(HYBRID);
        assert_eq!(m[1].card.card, "card1");
        assert!(!m[1].metrics.available);
        assert_eq!(m[1].metrics.busy, None);
        assert_eq!(m[1].metrics.temp_c, None);
        assert_eq!(m[1].metrics.clock_mhz, None);
    }

    #[test]
    fn merge_i915_card_publishes_its_clock_when_the_collector_read_one() {
        let m = merged(INTEL);
        assert_eq!(m.len(), 1);
        assert_eq!(m[0].card.driver, "i915");
        assert!(m[0].metrics.available);
        assert_eq!(m[0].metrics.busy, None);
        assert_eq!(m[0].metrics.clock_mhz, Some(300.0));
        assert_eq!(m[0].metrics.clock_max_mhz, Some(1250.0));
        assert_eq!(m[0].metrics.vram_total, None);
    }

    #[test]
    fn merge_non_amd_card_reads_its_card_hwmon_rows() {
        let drm = "card|card0|xe|0x8086|0x56a0|1|0000:03:00.0|\n\
                   metric|card0|temp1_input|47000\n\
                   metric|card0|power1_average|23000000\n\
                   metric|card0|fan1_input|1800";
        let m = merge_gpu(&parse_cards(drm), &parse_metrics(drm), &[]);
        assert!(m[0].metrics.available);
        assert_eq!(m[0].metrics.temp_c, Some(47.0));
        assert_eq!(m[0].metrics.power_w, Some(23.0));
        assert_eq!(m[0].metrics.fan_rpm, Some(1800.0));
        assert_eq!(m[0].metrics.fan_percent, None);
        assert_eq!(m[0].metrics.busy, None);
    }

    #[test]
    fn amd_fixture_yields_real_busy_fraction_and_vram_bytes() {
        let m = merged(AMD);
        assert_eq!(m.len(), 1);
        assert_eq!(m[0].card.driver, "amdgpu");
        assert!(is_discrete(&m[0].card));
        assert!(m[0].metrics.available);
        assert_eq!(m[0].metrics.busy, Some(0.37));
        assert_eq!(m[0].metrics.vram_used, Some(4294967296.0));
        assert_eq!(m[0].metrics.vram_total, Some(17179869184.0));
        assert_eq!(m[0].metrics.temp_c, Some(58.0));
        assert_eq!(m[0].metrics.power_w, Some(145.0));
        assert_eq!(m[0].metrics.fan_rpm, None);
        assert_eq!(m[0].metrics.fan_percent, None);
    }

    #[test]
    fn output_card_maps_hdmi_to_the_dgpu_and_edp_to_the_igpu() {
        let cards = parse_cards(&section(HYBRID, "drm"));
        let hdmi = output_card("HDMI-A-1", &cards).unwrap();
        assert_eq!((hdmi.card.as_str(), hdmi.driver.as_str()), ("card0", "nvidia"));
        let edp = output_card("eDP-1", &cards).unwrap();
        assert_eq!((edp.card.as_str(), edp.driver.as_str()), ("card1", "i915"));
    }

    #[test]
    fn output_card_is_none_for_an_unknown_connector() {
        assert_eq!(output_card("DP-99", &parse_cards(&section(HYBRID, "drm"))), None);
    }

    #[test]
    fn offload_argv_uses_the_nvidia_offload_wrapper_when_available() {
        let cards = parse_cards(&section(HYBRID, "drm"));
        let tools = OffloadTools { nvidia_offload: true, ..Default::default() };
        assert_eq!(offload_argv("firefox %u", Some(&cards[0]), tools).join(" "), "nvidia-offload sh -c firefox");
    }

    #[test]
    fn offload_argv_falls_back_to_prime_run() {
        let cards = parse_cards(&section(HYBRID, "drm"));
        let tools = OffloadTools { prime_run: true, ..Default::default() };
        assert_eq!(offload_argv("firefox", Some(&cards[0]), tools).join(" "), "prime-run sh -c firefox");
    }

    #[test]
    fn offload_argv_falls_back_to_the_exact_four_env_vars() {
        let cards = parse_cards(&section(HYBRID, "drm"));
        assert_eq!(
            offload_argv("firefox", Some(&cards[0]), OffloadTools::default()).join(" "),
            "env __NV_PRIME_RENDER_OFFLOAD=1 __NV_PRIME_RENDER_OFFLOAD_PROVIDER=NVIDIA-G0 \
             __GLX_VENDOR_LIBRARY_NAME=nvidia __VK_LAYER_NV_optimus=NVIDIA_only sh -c firefox"
        );
    }

    #[test]
    fn offload_argv_uses_dri_prime_pci_slot_for_a_non_nvidia_card() {
        let cards = parse_cards(&section(HYBRID, "drm"));
        assert_eq!(
            offload_argv("firefox", Some(&cards[1]), OffloadTools::default()).join(" "),
            "env DRI_PRIME=pci-0000_00_02_0 sh -c firefox"
        );
    }

    #[test]
    fn strip_field_codes_removes_a_single_code() {
        assert_eq!(strip_field_codes("firefox %u"), "firefox");
    }

    #[test]
    fn strip_field_codes_collapses_multiple_codes_cleanly() {
        assert_eq!(strip_field_codes("foo %F --bar %i"), "foo --bar");
    }

    #[test]
    fn strip_field_codes_keeps_a_literal_percent_from_double_percent() {
        assert_eq!(strip_field_codes("echo 100%%"), "echo 100%");
    }

    #[test]
    fn gfx_mode_empty_output_is_unsupported() {
        let gfx = parse_gfx_mode(&section(HYBRID, "gfx"));
        assert!(!gfx.supported);
        assert_eq!(gfx.mode, "");
    }

    #[test]
    fn gfx_mode_empty_string_is_unsupported_never_integrated() {
        let gfx = parse_gfx_mode("");
        assert!(!gfx.supported);
        assert_ne!(gfx.mode.to_lowercase(), "integrated");
    }

    #[test]
    fn gfx_mode_strips_quotes_when_supergfxctl_answers() {
        let gfx = parse_gfx_mode("\"Hybrid\"\n");
        assert!(gfx.supported);
        assert_eq!(gfx.mode, "Hybrid");
    }

    #[test]
    fn display_name_prefers_the_marketing_name_then_label_then_vendor() {
        let cards = parse_cards(&section(HYBRID, "drm"));
        let rows = parse_nvidia(&section(HYBRID, "nvidia"));
        assert_eq!(display_name(&cards[0], rows.first()), "NVIDIA GeForce RTX 5070 Laptop GPU");
        let bare = Card { card: "card9".into(), vendor_id: "0x10DE".into(), device_id: "0x1".into(), ..Default::default() };
        assert_eq!(display_name(&bare, None), "NVIDIA 0x1");
        let unknown = Card { card: "card9".into(), ..Default::default() };
        assert_eq!(display_name(&unknown, None), "card9");
    }
}
