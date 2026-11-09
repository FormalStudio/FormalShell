//! The section framing the system monitor's fixtures and parsers share:
//! the service reads /proc and /sys and files each read under a marker
//! (`stat`, `mem`, ...) before `sysinfo` and `gpu` touch it.
//!
//! The `drm`, `nvidia` and `gfx` section names and their row shapes are
//! what `gpu` parses, so the service's collector must keep them.
//!
//! `fan` carries the same `chip|file|label|value` row shape as `temp`:
//! hwmon exposes tachometers under fan*_input beside the temp*_input
//! sensors, and on a laptop those are separate chips from the thermal ones.
//!
//! gt_act_freq_mhz/gt_max_freq_mhz sit on the CARD directory, not on
//! `$c/device` where the amdgpu counters live. They are the only
//! unprivileged load signal an i915/xe card has: those drivers expose no
//! busy counter at all.

use std::collections::HashMap;

/// Splits framed text (`@name` over each section's lines) into sections
/// keyed by marker name with the leading `@` stripped, each body joined back with `\n`. A section with
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
