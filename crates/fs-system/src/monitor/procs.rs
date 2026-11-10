//! Process-table parsers for the launcher's process route, the same
//! pure-function split `sysinfo` draws for the fixed /proc files.
//!
//! The service reads the process table on its own tick rather than as a
//! section of `collect`: a 400-process machine pays for two extra passes
//! over /proc per tick, and the bar cell and the compact panel have no
//! process list to draw. It runs only while something is subscribed, which
//! in practice means only while the process route is open.
//!
//! Each section is the files' text as the kernel prints it. The cmdlines
//! come as `/proc/<pid>/cmdline:<text>` rows, NUL separators turned into
//! spaces, because a NUL inside a string is a byte nothing downstream
//! treats as a separator.
//!
//! Every fraction below is 0..1, and CPU is measured against the WHOLE
//! machine, not one core: a process pinning one core of six reads 0.167
//! here, where htop would say 100%. That keeps a process row directly
//! comparable with the monitor view's own CPU TOTAL row.

use fs_js as js;
use regex::Regex;
use std::collections::HashMap;
use std::sync::LazyLock;

fn lines(text: &str) -> impl Iterator<Item = &str> {
    text.split('\n').filter(|l| !l.is_empty())
}

// ---- sections -----------------------------------------------------------

static CPU_LINE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^cpu\s+(.*)$").unwrap());

/// Total jiffies across all CPUs off the aggregate `cpu` line, which is what
/// a per-process delta is measured against. `None` when the section is
/// missing or unparsable, never 0: 0 would divide into a fabricated 100%.
pub fn total_jiffies(text: &str) -> Option<f64> {
    for line in lines(text) {
        let Some(m) = CPU_LINE.captures(line) else { continue };
        let fields: Vec<f64> = js::split_ws(&m[1]).into_iter().map(js::parse_number).collect();
        if fields.len() < 4 {
            return None;
        }
        // user+nice+system+idle+iowait+irq+softirq+steal, dropping
        // guest/guest_nice for the same reason `parse_stat` does: the kernel
        // has already folded them into user/nice.
        return Some(fields.iter().take(8).map(|f| if f.is_nan() { 0.0 } else { *f }).sum());
    }
    None
}

static MEM_TOTAL: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"MemTotal:\s+(\d+)\s*kB").unwrap());

pub fn mem_total_bytes(text: &str) -> Option<f64> {
    MEM_TOTAL.captures(text).map(|m| js::parse_number(&m[1]) * 1024.0)
}

/// The page size, because RSS in /proc/PID/stat is counted in pages and
/// aarch64 kernels are free to use 16K ones. A missing or absurd answer
/// falls back to 4096 rather than reporting every process at 0 bytes.
pub fn page_size(text: &str) -> f64 {
    let value = js::parse_number(text);
    if value.is_finite() && value >= 1024.0 { value } else { 4096.0 }
}

// ---- /proc/PID/stat -----------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub struct ProcRecord {
    pub pid: u64,
    pub name: String,
    pub state: String,
    pub ppid: f64,
    pub jiffies: f64,
    pub threads: f64,
    /// Boot-relative, so it never changes for a live process: the one field
    /// that tells a recycled pid apart from the same process still running.
    pub started_at: f64,
    pub rss_pages: f64,
}

/// comm sits in parens as field 2 and may itself contain spaces AND parens
/// ("(sd-pam)", a process that prctl'd its name), so it is cut between the
/// FIRST "(" and the LAST ")" rather than split on whitespace. Field offsets
/// after that are counted from state (index 0 below = stat field 3).
pub fn parse_procs(text: &str) -> Vec<ProcRecord> {
    let mut records = Vec::new();
    for line in lines(text) {
        let (Some(open), Some(close)) = (line.find('('), line.rfind(')')) else { continue };
        if open < 1 || close < open {
            continue;
        }
        let pid = js::parse_number(&line[..open]);
        if !pid.is_finite() || pid <= 0.0 || pid.fract() != 0.0 {
            continue;
        }
        let rest = js::split_ws(&line[close + 1..]);
        if rest.len() < 22 {
            continue;
        }
        let n = |i: usize| js::number_or_zero(rest[i]);
        records.push(ProcRecord {
            pid: pid as u64,
            name: line[open + 1..close].to_string(),
            state: rest[0].to_string(),
            ppid: n(1),
            jiffies: n(11) + n(12),
            threads: n(17),
            started_at: n(19),
            rss_pages: n(21),
        });
    }
    records
}

static CMDLINE_LINE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^/proc/(\d+)/cmdline:(.*)$").unwrap());

/// `/proc/<pid>/cmdline:<text>` rows, one per process that has one.
/// A kernel thread's cmdline is genuinely empty, so it produces no row at all
/// and the pid is simply absent from the map.
pub fn parse_cmdlines(text: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for line in lines(text) {
        let Some(m) = CMDLINE_LINE.captures(line) else { continue };
        let cmd = js::trim_end(&m[2]);
        if !cmd.is_empty() {
            map.insert(m[1].to_string(), cmd.to_string());
        }
    }
    map
}

// ---- rows ---------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub struct ProcRow {
    pub pid: u64,
    pub ppid: f64,
    pub name: String,
    /// A kernel thread has no argv to show, which is a fact about the process
    /// rather than a gap in the reading, so it renders in the brackets ps
    /// uses for exactly this.
    pub cmd: String,
    pub kernel: bool,
    pub state: String,
    pub threads: f64,
    pub cpu_fraction: Option<f64>,
    pub mem_bytes: f64,
    pub mem_fraction: Option<f64>,
}

#[derive(Debug, Clone, Default)]
pub struct DeltaOptions {
    pub page_size: Option<f64>,
    pub mem_total_bytes: Option<f64>,
    pub cmdlines: HashMap<String, String>,
}

/// One display row per process in `next`. `total_delta` is the jiffie delta
/// between the two samples.
///
/// `cpu_fraction` is `None` (a dash on screen, never a 0%) for the first
/// sample after a subscribe, for a process this poll saw for the first time,
/// and for a pid whose `started_at` moved, which means the number was
/// recycled onto a different process between ticks and the old counter is not
/// comparable.
pub fn proc_delta(prev: Option<&[ProcRecord]>, next: &[ProcRecord], total_delta: f64, opts: &DeltaOptions) -> Vec<ProcRow> {
    let bytes_per_page = opts.page_size.filter(|p| *p != 0.0 && !p.is_nan()).unwrap_or(4096.0);
    let mem_total = opts.mem_total_bytes.filter(|m| *m != 0.0 && !m.is_nan());
    let by_pid: HashMap<u64, &ProcRecord> = prev.unwrap_or(&[]).iter().map(|p| (p.pid, p)).collect();
    let usable = total_delta > 0.0;

    next.iter()
        .map(|rec| {
            let was = by_pid.get(&rec.pid);
            let cpu_fraction = match was {
                Some(was) if usable && was.started_at == rec.started_at => {
                    let busy = (rec.jiffies - was.jiffies).max(0.0);
                    Some((busy / total_delta).min(1.0))
                }
                _ => None,
            };
            let bytes = rec.rss_pages * bytes_per_page;
            let cmd = opts.cmdlines.get(&rec.pid.to_string()).cloned().unwrap_or_default();
            ProcRow {
                pid: rec.pid,
                ppid: rec.ppid,
                name: rec.name.clone(),
                kernel: cmd.is_empty(),
                cmd,
                state: rec.state.clone(),
                threads: rec.threads,
                cpu_fraction,
                mem_bytes: bytes,
                mem_fraction: mem_total.map(|t| (bytes / t).min(1.0)),
            }
        })
        .collect()
}

// ---- filter and sort ----------------------------------------------------

/// Case-insensitive substring over the name and the argv, plus an exact pid
/// match so a pid pasted in from a log finds its row. Matching the argv is
/// what makes "python" find a script the kernel named after its interpreter
/// and "electron" find every app built on one, which comm alone (15 bytes,
/// truncated by the kernel) cannot do.
pub fn filter_rows(rows: &[ProcRow], query: &str) -> Vec<ProcRow> {
    let q = js::trim(query).to_lowercase();
    if q.is_empty() {
        return rows.to_vec();
    }
    rows.iter()
        .filter(|r| r.pid.to_string() == q || r.name.to_lowercase().contains(&q) || r.cmd.to_lowercase().contains(&q))
        .cloned()
        .collect()
}

pub(crate) const SORTS: [&str; 4] = ["cpu", "mem", "pid", "name"];

/// Every mode breaks its own ties on pid, so the order two idle processes sit
/// in cannot flip between polls: a list that reshuffled under the cursor
/// every two seconds would make the cursor point at a different process than
/// the one the reader aimed at. A `None` cpu fraction (nothing measured yet)
/// sorts as the bottom of the CPU column rather than as zero. An unknown mode
/// sorts by cpu.
pub fn sort_rows(rows: &[ProcRow], mode: &str) -> Vec<ProcRow> {
    let key = if SORTS.contains(&mode) { mode } else { "cpu" };
    let mut out = rows.to_vec();
    out.sort_by(|a, b| {
        let d = match key {
            "cpu" => b.cpu_fraction.unwrap_or(-1.0) - a.cpu_fraction.unwrap_or(-1.0),
            "mem" => b.mem_bytes - a.mem_bytes,
            "name" => {
                // JS compares strings by UTF-16 code unit.
                let (x, y) = (a.name.to_lowercase(), b.name.to_lowercase());
                match x.encode_utf16().cmp(y.encode_utf16()) {
                    std::cmp::Ordering::Less => -1.0,
                    std::cmp::Ordering::Greater => 1.0,
                    std::cmp::Ordering::Equal => 0.0,
                }
            }
            _ => 0.0,
        };
        d.partial_cmp(&0.0).unwrap_or(std::cmp::Ordering::Equal).then(a.pid.cmp(&b.pid))
    });
    out
}

// ---- actions ------------------------------------------------------------

/// The signals a process row can send. TERM asks, KILL takes, HUP is what a
/// daemon reloads on, and INT is what Ctrl-C would have sent had the process
/// been in a terminal. Anything outside this list is refused by name rather
/// than passed through to `kill`, so an IPC caller cannot reach for a signal
/// this surface never meant to offer.
pub const SIGNALS: [&str; 4] = ["TERM", "KILL", "HUP", "INT"];

/// A positive whole number, as a string or a number the way IPC hands it.
pub fn parse_pid(pid: &str) -> Option<u64> {
    let n = js::parse_number(pid);
    (n.is_finite() && n > 0.0 && n.floor() == n && n < u64::MAX as f64).then_some(n as u64)
}

/// `None` is a refused signal. No name at all means TERM.
pub fn normalize_signal(name: Option<&str>) -> Option<&'static str> {
    let upper = js::trim(name.unwrap_or("")).to_uppercase();
    if upper.is_empty() {
        return Some("TERM");
    }
    let s = upper.strip_prefix("SIG").unwrap_or(&upper);
    SIGNALS.iter().find(|k| **k == s).copied()
}

/// `kill` as the shell builtin, not the procps binary: every POSIX sh has it,
/// and this shell ships no dependency on procps anywhere else. The pid is a
/// number, so it is always digits.
pub fn kill_command(pid: u64, signal: &str) -> Vec<String> {
    vec!["sh".into(), "-c".into(), format!("kill -s {signal} {pid}")]
}

/// The exact argv, one arg per line: the poll's own cmdline pass replaced NULs
/// with spaces for display, which loses the boundary between an argument and
/// a space inside one. A restart has to re-exec the real thing, so it re-reads
/// the file with the separator intact.
pub fn argv_command(pid: u64) -> Vec<String> {
    vec!["sh".into(), "-c".into(), format!(r"tr '\0' '\n' < /proc/{pid}/cmdline")]
}

/// Empty for a process that died between the read and the parse (an empty
/// cmdline is also what a kernel thread has, which is why nothing offers a
/// restart on one).
pub fn parse_argv(text: &str) -> Vec<String> {
    lines(text).map(String::from).collect()
}

pub fn alive_command(pid: u64) -> Vec<String> {
    vec!["sh".into(), "-c".into(), format!("test -d /proc/{pid}")]
}

/// Re-runs the argv from the process's own working directory. `exec` so the
/// wrapping shell is replaced rather than left sitting around as the parent,
/// and `cd` only when a cwd was actually readable: /proc/PID/cwd is a symlink
/// only the owner (or root) may follow, so another user's process restarts
/// from the shell's own cwd instead of failing outright.
///
/// This restores the command line, never the environment: a process launched
/// with variables its parent set comes back without them.
pub fn respawn_command(argv: &[String], cwd: &str) -> Vec<String> {
    let mut out: Vec<String> = if cwd.is_empty() {
        vec!["sh".into(), "-c".into(), r#"exec "$@""#.into(), "sh".into()]
    } else {
        vec!["sh".into(), "-c".into(), r#"cd "$1" && shift && exec "$@""#.into(), "sh".into(), cwd.into()]
    };
    out.extend_from_slice(argv);
    out
}

pub fn cwd_command(pid: u64) -> Vec<String> {
    vec!["sh".into(), "-c".into(), format!("readlink /proc/{pid}/cwd 2>/dev/null || true")]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::monitor::collect::split_sections;

    const BUSY_PID: u64 = 36642;
    const BUSY_FRACTION: f64 = 201.0 / 1213.0;

    fn a() -> HashMap<String, String> {
        split_sections(include_str!("../../tests/fixtures/procs-vm-a.txt"))
    }

    fn b() -> HashMap<String, String> {
        split_sections(include_str!("../../tests/fixtures/procs-vm-b.txt"))
    }

    fn row_for(rows: &[ProcRow], pid: u64) -> Option<&ProcRow> {
        rows.iter().find(|r| r.pid == pid)
    }

    fn rows() -> Vec<ProcRow> {
        let (a, b) = (a(), b());
        proc_delta(
            Some(&parse_procs(&a["procs"])),
            &parse_procs(&b["procs"]),
            total_jiffies(&b["stat"]).unwrap() - total_jiffies(&a["stat"]).unwrap(),
            &DeltaOptions {
                page_size: Some(page_size(&b["meta"])),
                mem_total_bytes: mem_total_bytes(&b["mem"]),
                cmdlines: parse_cmdlines(&b["cmdline"]),
            },
        )
    }

    // ---- sections ----

    #[test]
    fn split_sections_finds_every_process_marker() {
        let a = a();
        for name in ["stat", "mem", "meta", "procs", "cmdline", "end"] {
            assert!(a.contains_key(name), "{name} missing");
        }
    }

    #[test]
    fn total_jiffies_sums_the_aggregate_line_only() {
        let (a, b) = (a(), b());
        assert_eq!(total_jiffies(&b["stat"]).unwrap() - total_jiffies(&a["stat"]).unwrap(), 1213.0);
    }

    #[test]
    fn total_jiffies_is_none_without_a_sample() {
        assert_eq!(total_jiffies(""), None);
    }

    #[test]
    fn mem_total_is_bytes_not_kilobytes() {
        assert_eq!(mem_total_bytes(&a()["mem"]), Some(8098064.0 * 1024.0));
    }

    #[test]
    fn page_size_falls_back_rather_than_reporting_zero_bytes() {
        assert_eq!(page_size(&a()["meta"]), 4096.0);
        assert_eq!(page_size(""), 4096.0);
        assert_eq!(page_size("garbage"), 4096.0);
        assert_eq!(page_size("16384\n"), 16384.0);
    }

    // ---- /proc/PID/stat ----

    #[test]
    fn parse_procs_reads_every_row() {
        assert_eq!(parse_procs(&a()["procs"]).len(), 23);
    }

    #[test]
    fn parse_procs_cuts_comm_between_the_outer_parens() {
        let records = parse_procs(&a()["procs"]);
        let rec = records.iter().find(|r| r.pid == 846).unwrap();
        assert_eq!(rec.name, "(sd-pam)");
        assert_eq!(rec.ppid, 842.0);
    }

    #[test]
    fn parse_procs_keeps_the_kernels_truncated_comm_verbatim() {
        let records = parse_procs(&a()["procs"]);
        assert!(records.iter().any(|r| r.name == "power-profiles-"));
    }

    #[test]
    fn parse_procs_reads_the_counters_the_delta_needs() {
        let records = parse_procs(&a()["procs"]);
        let rec = records.iter().find(|r| r.pid == BUSY_PID).unwrap();
        assert_eq!(rec.name, "sh");
        assert_eq!(rec.jiffies, 100.0);
        assert_eq!(rec.threads, 1.0);
        assert_eq!(rec.rss_pages, 886.0);
        assert_eq!(rec.started_at, 5637636.0);
    }

    #[test]
    fn parse_procs_ignores_a_line_it_cannot_place() {
        assert_eq!(parse_procs("garbage\n\n").len(), 0);
    }

    // ---- cmdlines ----

    #[test]
    fn parse_cmdlines_keys_by_pid_from_the_path() {
        let map = parse_cmdlines(&a()["cmdline"]);
        assert_eq!(map["36642"], "sh -c while :; do :; done");
    }

    #[test]
    fn parse_cmdlines_has_no_entry_for_a_kernel_thread() {
        assert_eq!(parse_cmdlines(&a()["cmdline"]).get("103"), None);
    }

    // ---- procDelta ----

    #[test]
    fn first_sample_measures_nothing_rather_than_zero() {
        let rows = proc_delta(None, &parse_procs(&a()["procs"]), 0.0, &DeltaOptions::default());
        assert_eq!(rows.len(), 23);
        assert!(rows.iter().all(|r| r.cpu_fraction.is_none()));
    }

    #[test]
    fn cpu_fraction_is_the_share_of_the_whole_machine() {
        let rows = rows();
        let row = row_for(&rows, BUSY_PID).unwrap();
        assert!((row.cpu_fraction.unwrap() - BUSY_FRACTION).abs() < 0.0001);
    }

    #[test]
    fn an_idle_process_measures_zero_not_none() {
        let rows = rows();
        let row = row_for(&rows, 1000).unwrap();
        assert_eq!(row.name, "dbus-broker");
        assert_eq!(row.cpu_fraction, Some(0.0));
    }

    #[test]
    fn a_recycled_pid_is_unmeasured_rather_than_wrong() {
        let prev: Vec<ProcRecord> = parse_procs(&a()["procs"])
            .into_iter()
            .map(|mut rec| {
                if rec.pid == BUSY_PID {
                    rec.started_at -= 1.0;
                }
                rec
            })
            .collect();
        let rows = proc_delta(Some(&prev), &parse_procs(&b()["procs"]), 1213.0, &DeltaOptions::default());
        assert_eq!(row_for(&rows, BUSY_PID).unwrap().cpu_fraction, None);
    }

    #[test]
    fn memory_is_pages_times_the_page_size() {
        let rows = rows();
        let row = row_for(&rows, BUSY_PID).unwrap();
        assert_eq!(row.mem_bytes, 886.0 * 4096.0);
        assert!((row.mem_fraction.unwrap() - (886.0 * 4096.0) / (8098064.0 * 1024.0)).abs() < 0.0001);
    }

    #[test]
    fn a_process_without_an_argv_is_marked_a_kernel_thread() {
        let rows = rows();
        assert!(row_for(&rows, 103).unwrap().kernel);
        assert_eq!(row_for(&rows, 103).unwrap().cmd, "");
        assert!(!row_for(&rows, BUSY_PID).unwrap().kernel);
    }

    // ---- filter ----

    #[test]
    fn filter_matches_the_process_name() {
        let rows = filter_rows(&rows(), "kworker");
        assert!(rows.len() >= 3);
        assert!(rows.iter().all(|r| r.name.contains("kworker")));
    }

    #[test]
    fn filter_matches_the_command_line_too() {
        let rows = filter_rows(&rows(), "while :;");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].pid, BUSY_PID);
    }

    #[test]
    fn filter_matches_a_pid_exactly() {
        let rows = filter_rows(&rows(), "1000");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].name, "dbus-broker");
    }

    #[test]
    fn an_empty_query_is_the_whole_table() {
        assert_eq!(filter_rows(&rows(), "").len(), 23);
        assert_eq!(filter_rows(&rows(), "   ").len(), 23);
    }

    #[test]
    fn filter_is_case_insensitive() {
        assert_eq!(filter_rows(&rows(), "DBUS").len(), 1);
    }

    // ---- sort ----

    #[test]
    fn cpu_sort_leads_with_the_busiest_process() {
        assert_eq!(sort_rows(&rows(), "cpu")[0].pid, BUSY_PID);
    }

    #[test]
    fn ties_break_on_pid_so_the_order_cannot_flip() {
        let rows = sort_rows(&rows(), "cpu");
        for w in rows.windows(2) {
            if w[0].cpu_fraction == w[1].cpu_fraction {
                assert!(w[1].pid > w[0].pid);
            }
        }
    }

    fn bare(pid: u64, cpu: Option<f64>, name: &str) -> ProcRow {
        ProcRow {
            pid,
            ppid: 0.0,
            name: name.into(),
            cmd: String::new(),
            kernel: true,
            state: "S".into(),
            threads: 1.0,
            cpu_fraction: cpu,
            mem_bytes: 0.0,
            mem_fraction: None,
        }
    }

    #[test]
    fn unmeasured_cpu_sorts_below_a_measured_zero() {
        let rows = sort_rows(&[bare(2, None, "b"), bare(1, Some(0.0), "a")], "cpu");
        assert_eq!(rows[0].pid, 1);
    }

    #[test]
    fn mem_sort_is_descending() {
        let rows = sort_rows(&rows(), "mem");
        assert!(rows.windows(2).all(|w| w[1].mem_bytes <= w[0].mem_bytes));
    }

    #[test]
    fn pid_sort_is_ascending() {
        let rows = sort_rows(&rows(), "pid");
        assert!(rows.windows(2).all(|w| w[1].pid > w[0].pid));
    }

    #[test]
    fn sort_does_not_reorder_its_input() {
        let rows = rows();
        let first = rows[0].pid;
        let _ = sort_rows(&rows, "mem");
        assert_eq!(rows[0].pid, first);
    }

    #[test]
    fn an_unknown_mode_falls_back_to_cpu() {
        assert_eq!(sort_rows(&rows(), "nonsense")[0].pid, BUSY_PID);
    }

    // ---- actions ----

    #[test]
    fn only_the_four_offered_signals_are_accepted() {
        assert_eq!(normalize_signal(Some("term")), Some("TERM"));
        assert_eq!(normalize_signal(Some("SIGKILL")), Some("KILL"));
        assert_eq!(normalize_signal(Some("")), Some("TERM"));
        assert_eq!(normalize_signal(None), Some("TERM"));
        assert_eq!(normalize_signal(Some("STOP")), None);
        assert_eq!(normalize_signal(Some("9")), None);
    }

    #[test]
    fn kill_interpolates_a_number_never_a_string() {
        assert_eq!(kill_command(parse_pid("36642").unwrap(), "TERM"), ["sh", "-c", "kill -s TERM 36642"]);
    }

    #[test]
    fn argv_is_read_back_with_its_separators_intact() {
        assert_eq!(parse_argv("sh\n-c\nwhile :; do :; done\n"), ["sh", "-c", "while :; do :; done"]);
        assert!(parse_argv("").is_empty());
    }

    #[test]
    fn respawn_passes_every_argument_whole() {
        let argv = ["sh", "-c", "echo 'hi'"].map(String::from);
        assert_eq!(
            respawn_command(&argv, "/home/x y"),
            ["sh", "-c", r#"cd "$1" && shift && exec "$@""#, "sh", "/home/x y", "sh", "-c", "echo 'hi'"]
        );
    }

    #[test]
    fn respawn_without_a_readable_cwd_still_runs() {
        assert_eq!(respawn_command(&["mpv".to_string()], ""), ["sh", "-c", r#"exec "$@""#, "sh", "mpv"]);
    }

}
