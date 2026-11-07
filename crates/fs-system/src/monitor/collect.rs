//! The one-shot `sh -c` collector for the system monitor: every /proc and
//! /sys read in one process per poll tick. Section markers (`@stat`, `@mem`,
//! ...) let one pass split the blob before `sysinfo` and `gpu` touch it.
//!
//! The `@drm`, `@nvidia` and `@gfx` section names and their row shapes are
//! what `gpu` parses, so they must stay byte-identical to the script below.
//!
//! `@fan` carries the same `chip|file|label|value` row shape as `@temp`:
//! hwmon exposes tachometers under fan*_input beside the temp*_input
//! sensors, and on a laptop those are separate chips from the thermal ones.
//!
//! gt_act_freq_mhz/gt_max_freq_mhz sit on the CARD directory, not on
//! `$c/device` where the amdgpu counters live, which is why they need a loop
//! of their own. They are the only unprivileged load signal an i915/xe card
//! has: those drivers expose no busy counter at all.

use std::collections::HashMap;

const LINES: [&str; 12] = [
    r##"echo "@stat"; grep -E '^cpu' /proc/stat"##,
    r##"echo "@mem"; grep -E '^(MemTotal|MemAvailable|MemFree|SwapTotal|SwapFree):' /proc/meminfo"##,
    r##"echo "@load"; cat /proc/loadavg"##,
    r##"echo "@uptime"; cat /proc/uptime"##,
    r##"echo "@net"; tail -n +3 /proc/net/dev"##,
    r##"echo "@temp"; for h in /sys/class/hwmon/hwmon*; do [ -d "$h" ] || continue; n=$(cat "$h/name" 2>/dev/null); for t in "$h"/temp*_input; do [ -r "$t" ] || continue; b=$(basename "$t"); l="${t%_input}_label"; echo "$n|$b|$(cat "$l" 2>/dev/null)|$(cat "$t" 2>/dev/null)"; done; done"##,
    r##"echo "@fan"; for h in /sys/class/hwmon/hwmon*; do [ -d "$h" ] || continue; n=$(cat "$h/name" 2>/dev/null); for t in "$h"/fan*_input; do [ -r "$t" ] || continue; b=$(basename "$t"); l="${t%_input}_label"; echo "$n|$b|$(cat "$l" 2>/dev/null)|$(cat "$t" 2>/dev/null)"; done; done"##,
    r##"echo "@disk"; df -B1 -x tmpfs -x devtmpfs -x efivarfs --output=source,target,size,used 2>/dev/null | tail -n +2"##,
    r##"echo "@drm"; for c in /sys/class/drm/card*; do case "$(basename "$c")" in card[0-9]|card[0-9][0-9]) ;; *) continue ;; esac; d="$c/device"; echo "card|$(basename "$c")|$(basename "$(readlink -f "$d/driver" 2>/dev/null)")|$(cat "$d/vendor" 2>/dev/null)|$(cat "$d/device" 2>/dev/null)|$(cat "$d/boot_vga" 2>/dev/null)|$(basename "$(readlink -f "$d")")|$(cat "$d/label" 2>/dev/null)"; for f in gpu_busy_percent mem_info_vram_used mem_info_vram_total mem_busy_percent; do [ -r "$d/$f" ] && echo "metric|$(basename "$c")|$f|$(cat "$d/$f")"; done; for f in gt_act_freq_mhz gt_max_freq_mhz; do [ -r "$c/$f" ] && echo "metric|$(basename "$c")|$f|$(cat "$c/$f")"; done; for hw in "$d"/hwmon/hwmon*; do [ -d "$hw" ] || continue; for t in "$hw"/temp1_input "$hw"/power1_average "$hw"/fan1_input; do [ -r "$t" ] && echo "metric|$(basename "$c")|$(basename "$t")|$(cat "$t")"; done; done; for k in "$c"-*; do [ -d "$k" ] || continue; echo "conn|$(basename "$c")|$(basename "$k" | sed "s/^card[0-9]*-//")|$(cat "$k/status" 2>/dev/null)"; done; done"##,
    r##"echo "@nvidia"; command -v nvidia-smi >/dev/null 2>&1 && nvidia-smi --query-gpu=index,name,utilization.gpu,temperature.gpu,memory.used,memory.total,power.draw,fan.speed --format=csv,noheader,nounits 2>/dev/null"##,
    r##"echo "@gfx"; command -v supergfxctl >/dev/null 2>&1 && timeout 1 supergfxctl -g 2>/dev/null"##,
    r##"echo "@end""##,
];

pub(crate) fn collector_script() -> String {
    LINES.join("\n")
}

/// argv for the collector: it never takes arguments.
pub fn collect_command() -> Vec<String> {
    vec!["sh".into(), "-c".into(), collector_script()]
}

/// Splits one collector run's stdout into sections keyed by marker name with
/// the leading `@` stripped, each body joined back with `\n`. A section with
/// nothing between its marker and the next one comes back as `""`, not an
/// absent key, so callers can always index the result.
pub fn split_sections(blob: &str) -> HashMap<String, String> {
    let mut sections = HashMap::new();
    let mut current: Option<String> = None;
    let mut buffer: Vec<&str> = Vec::new();

    for line in blob.split('\n') {
        if let Some(name) = line.strip_prefix('@') {
            if let Some(c) = current.take() {
                sections.insert(c, buffer.join("\n"));
            }
            current = Some(name.to_string());
            buffer.clear();
        } else if current.is_some() {
            buffer.push(line);
        }
    }
    if let Some(c) = current {
        sections.insert(c, buffer.join("\n"));
    }
    sections
}
