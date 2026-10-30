//! The Information panel's static facts, read once per open: the OS off
//! os-release, the host name, the board off DMI and the CPU off cpuinfo.
//! The collector prints each file under a `@<name>` line; a file the
//! machine lacks (a VM's DMI board, an ARM cpuinfo with no `model name`)
//! leaves its field `None` rather than a guess.

use std::collections::HashMap;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct About {
    pub os: Option<String>,
    /// os-release's `ID`, for the footer's mark.
    pub os_id: Option<String>,
    pub host: Option<String>,
    pub board: Option<String>,
    pub cpu: Option<String>,
}

/// The shell script the service runs: one `@section` per file, a missing
/// file printing nothing under its header.
pub fn collect_command() -> Vec<String> {
    let script = "for f in os:/etc/os-release host:/proc/sys/kernel/hostname \
        board:/sys/class/dmi/id/board_name \
        product:/sys/class/dmi/id/product_name cpu:/proc/cpuinfo; do \
        echo \"@${f%%:*}\"; cat \"${f#*:}\" 2>/dev/null; done";
    vec!["sh".into(), "-c".into(), script.into()]
}

fn sections(text: &str) -> HashMap<&str, String> {
    let mut out: HashMap<&str, String> = HashMap::new();
    let mut at = None;
    for line in text.lines() {
        if let Some(name) = line.strip_prefix('@') {
            at = Some(name);
            out.entry(name).or_default();
        } else if let Some(name) = at {
            let s = out.entry(name).or_default();
            s.push_str(line);
            s.push('\n');
        }
    }
    out
}

/// One `KEY=value` from os-release, its quotes taken off.
fn os_release<'a>(text: &'a str, key: &str) -> Option<&'a str> {
    text.lines()
        .filter_map(|l| l.split_once('='))
        .find(|(k, _)| k.trim() == key)
        .map(|(_, v)| v.trim().trim_matches('"').trim_matches('\''))
        .filter(|v| !v.is_empty())
}

/// DMI fills unset strings with OEM placeholders.
fn dmi(text: Option<&String>) -> Option<String> {
    let v = text?.trim();
    let placeholder = ["", "default string", "to be filled by o.e.m.", "not applicable", "none", "system product name"];
    (!placeholder.contains(&v.to_lowercase().as_str())).then(|| v.to_owned())
}

/// The model name with its threads, `AMD Ryzen 7 9800X3D (16 Threads)`;
/// the `(R)`, `(TM)` and `CPU @ ...` noise Intel writes goes.
pub fn cpu_label(cpuinfo: &str) -> Option<String> {
    let field = |l: &str, key: &str| l.split_once(':').filter(|(k, _)| k.trim() == key).map(|(_, v)| v.trim().to_owned());
    let model = cpuinfo.lines().find_map(|l| field(l, "model name").or_else(|| field(l, "Model")))?;
    let threads = cpuinfo.lines().filter(|l| l.split_once(':').is_some_and(|(k, _)| k.trim() == "processor")).count();
    let mut name = model.replace("(R)", "").replace("(TM)", "").replace("(tm)", "");
    if let Some(at) = name.find(" @ ") {
        name.truncate(at);
    }
    let name = name.trim_end_matches(" CPU").split_whitespace().collect::<Vec<_>>().join(" ");
    if name.is_empty() {
        return None;
    }
    Some(match threads {
        0 => name,
        1 => format!("{name} (1 Thread)"),
        n => format!("{name} ({n} Threads)"),
    })
}

pub fn parse(text: &str) -> About {
    let s = sections(text);
    let os = s.get("os").map(String::as_str).unwrap_or("");
    let board = dmi(s.get("board")).or_else(|| dmi(s.get("product")));
    About {
        os: os_release(os, "PRETTY_NAME").or_else(|| os_release(os, "NAME")).map(str::to_owned),
        os_id: os_release(os, "ID").map(str::to_owned),
        host: s.get("host").map(|h| h.trim().to_owned()).filter(|h| !h.is_empty()),
        board,
        cpu: s.get("cpu").and_then(|c| cpu_label(c)),
    }
}

/// `1.2 GB / 30.5 GB`, decimal gigabytes the way Pond and GNOME's About
/// page print them.
pub fn used_of(used_bytes: f64, total_bytes: f64) -> String {
    let gb = |b: f64| b / 1e9;
    format!("{:.1} GB / {:.1} GB", gb(used_bytes), gb(total_bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "@os\nNAME=\"openSUSE Tumbleweed\"\nID=\"opensuse-tumbleweed\"\nPRETTY_NAME=\"openSUSE Tumbleweed\"\n@host\ng815\n@board\nG815LP\n@product\nROG Strix G16\n@cpu\nprocessor\t: 0\nmodel name\t: Intel(R) Core(TM) Ultra 9 275HX\nprocessor\t: 1\nmodel name\t: Intel(R) Core(TM) Ultra 9 275HX\n";

    #[test]
    fn reads_every_section() {
        let a = parse(SAMPLE);
        assert_eq!(a.os.as_deref(), Some("openSUSE Tumbleweed"));
        assert_eq!(a.os_id.as_deref(), Some("opensuse-tumbleweed"));
        assert_eq!(a.host.as_deref(), Some("g815"));
        assert_eq!(a.board.as_deref(), Some("G815LP"));
        assert_eq!(a.cpu.as_deref(), Some("Intel Core Ultra 9 275HX (2 Threads)"));
    }

    #[test]
    fn the_product_stands_in_for_a_blank_board() {
        let a = parse("@board\nDefault string\n@product\nROG Strix G16\n");
        assert_eq!(a.board.as_deref(), Some("ROG Strix G16"));
    }

    #[test]
    fn missing_and_placeholder_files_stay_none() {
        let a = parse("@os\n@host\n@board\nTo be filled by O.E.M.\n@product\nNone\n@cpu\nprocessor : 0\n");
        assert_eq!(a, About::default());
    }

    #[test]
    fn intel_frequency_suffix_goes() {
        let c = cpu_label("processor : 0\nmodel name : Intel(R) Core(TM) i7-8550U CPU @ 1.80GHz\n");
        assert_eq!(c.as_deref(), Some("Intel Core i7-8550U (1 Thread)"));
    }

    #[test]
    fn used_of_prints_decimal_gigabytes() {
        assert_eq!(used_of(1.2e9, 30.5e9), "1.2 GB / 30.5 GB");
    }
}
