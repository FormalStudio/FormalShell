//! The Information panel's static facts, read once per open: the OS off
//! os-release, the host name, the board off DMI and the CPU off cpuinfo.
//! The service reads each file (`Files`); one the machine lacks (a VM's DMI board,
//! an ARM cpuinfo with no `model name`) is empty text and leaves its field
//! `None` rather than a guess.

#[derive(Clone, Debug, Default, PartialEq)]
pub struct About {
    pub os: Option<String>,
    /// os-release's `ID`, for the footer's mark.
    pub os_id: Option<String>,
    /// os-release's `NAME`, the footer's word beside the mark.
    pub os_name: Option<String>,
    pub host: Option<String>,
    pub board: Option<String>,
    pub cpu: Option<String>,
}

/// The text of each file `parse` reads, empty for one the machine lacks.
#[derive(Clone, Debug, Default)]
pub struct Files {
    /// /etc/os-release
    pub os: String,
    /// /proc/sys/kernel/hostname
    pub host: String,
    /// /sys/class/dmi/id/board_name
    pub board: String,
    /// /sys/class/dmi/id/product_name
    pub product: String,
    /// /proc/cpuinfo
    pub cpu: String,
}

/// One `KEY=value` from os-release, unquoted the way its shell-style
/// grammar reads: one pair of delimiters off, and inside double quotes a
/// backslash escaping `"`, `\\`, `$` or a backtick.
fn os_release(text: &str, key: &str) -> Option<String> {
    let raw = text.lines().filter_map(|l| l.split_once('=')).find(|(k, _)| k.trim() == key)?.1.trim();
    let value = if let Some(inner) = raw.strip_prefix('\'').and_then(|r| r.strip_suffix('\'')) {
        inner.to_owned()
    } else if let Some(inner) = raw.strip_prefix('"').and_then(|r| r.strip_suffix('"')) {
        let mut out = String::with_capacity(inner.len());
        let mut chars = inner.chars().peekable();
        while let Some(c) = chars.next() {
            match (c, chars.peek()) {
                ('\\', Some('"' | '\\' | '$' | '`')) => out.extend(chars.next()),
                _ => out.push(c),
            }
        }
        out
    } else {
        raw.to_owned()
    };
    (!value.is_empty()).then_some(value)
}

/// This machine's os-release `ID`, which the launcher's logo is keyed by.
pub fn os_id() -> Option<String> {
    os_release(&std::fs::read_to_string("/etc/os-release").ok()?, "ID")
}

/// DMI fills unset strings with OEM placeholders.
fn dmi(text: &str) -> Option<String> {
    let v = text.trim();
    let placeholder = ["", "default string", "to be filled by o.e.m.", "not applicable", "none", "not specified", "system product name", "system version"];
    (!placeholder.contains(&v.to_lowercase().as_str())).then(|| v.to_owned())
}

/// The model name with its threads, `AMD Ryzen 7 9800X3D (16 Threads)`;
/// the `(R)`, `(TM)` and `CPU @ ...` noise Intel writes goes. ARM
/// cpuinfo carries no `model name`, and its `Model` names the board, so
/// it reads as none.
pub(crate) fn cpu_label(cpuinfo: &str) -> Option<String> {
    let field = |l: &str, key: &str| l.split_once(':').filter(|(k, _)| k.trim() == key).map(|(_, v)| v.trim().to_owned());
    let model = cpuinfo.lines().find_map(|l| field(l, "model name"))?;
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

pub fn parse(files: &Files) -> About {
    let os = files.os.as_str();
    let board = dmi(&files.board).or_else(|| dmi(&files.product));
    About {
        os: os_release(os, "PRETTY_NAME").or_else(|| os_release(os, "NAME")),
        os_id: os_release(os, "ID"),
        os_name: os_release(os, "NAME"),
        host: Some(files.host.trim().to_owned()).filter(|h| !h.is_empty()),
        board,
        cpu: cpu_label(&files.cpu),
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

    /// A fixture written as `@name` headers over each file's text.
    fn parse(text: &str) -> About {
        let mut files = Files::default();
        let mut at = "";
        for line in text.lines() {
            if let Some(name) = line.strip_prefix('@') {
                at = name;
                continue;
            }
            let dest = match at {
                "os" => &mut files.os,
                "host" => &mut files.host,
                "board" => &mut files.board,
                "product" => &mut files.product,
                _ => &mut files.cpu,
            };
            dest.push_str(line);
            dest.push('\n');
        }
        super::parse(&files)
    }

    const SAMPLE: &str = "@os\nNAME=\"openSUSE Tumbleweed\"\nID=\"opensuse-tumbleweed\"\nPRETTY_NAME=\"openSUSE Tumbleweed\"\n@host\ng815\n@board\nG815LP\n@product\nROG Strix G16\n@cpu\nprocessor\t: 0\nmodel name\t: Intel(R) Core(TM) Ultra 9 275HX\nprocessor\t: 1\nmodel name\t: Intel(R) Core(TM) Ultra 9 275HX\n";

    #[test]
    fn reads_every_section() {
        let a = parse(SAMPLE);
        assert_eq!(a.os.as_deref(), Some("openSUSE Tumbleweed"));
        assert_eq!(a.os_id.as_deref(), Some("opensuse-tumbleweed"));
        assert_eq!(a.os_name.as_deref(), Some("openSUSE Tumbleweed"));
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
    fn os_release_unquotes_its_escapes() {
        let a = parse("@os\nPRETTY_NAME=\"Say \\\"hi\\\" \\\\ OS\"\nID='arch'\n");
        assert_eq!(a.os.as_deref(), Some("Say \"hi\" \\ OS"));
        assert_eq!(a.os_id.as_deref(), Some("arch"));
    }

    #[test]
    fn not_specified_falls_back_to_the_product() {
        let a = parse("@board\nNot Specified\n@product\nThinkPad X1\n");
        assert_eq!(a.board.as_deref(), Some("ThinkPad X1"));
    }

    #[test]
    fn an_arm_board_model_is_not_a_cpu() {
        assert_eq!(cpu_label("processor : 0\nModel : Raspberry Pi 4 Model B Rev 1.4\n"), None);
    }

    #[test]
    fn used_of_prints_decimal_gigabytes() {
        assert_eq!(used_of(1.2e9, 30.5e9), "1.2 GB / 30.5 GB");
    }
}
