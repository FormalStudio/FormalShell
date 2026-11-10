use super::*;

/// One reading per monitor tick: CPU, memory, GPU, and the network's
/// receive and send rates. A reading nobody could take is left out.
#[derive(Default)]
pub(super) struct MonitorHistory {
    pub(super) sampled: Option<u64>,
    pub(super) cpu: std::collections::VecDeque<f64>,
    pub(super) mem: std::collections::VecDeque<f64>,
    pub(super) gpu: std::collections::VecDeque<f64>,
    pub(super) rx: std::collections::VecDeque<f64>,
    pub(super) tx: std::collections::VecDeque<f64>,
}

pub(super) const HISTORY: usize = 60;

pub(super) fn push(q: &mut std::collections::VecDeque<f64>, v: Option<f64>) {
    if let Some(v) = v.filter(|v| v.is_finite()) {
        if q.len() == HISTORY {
            q.pop_front();
        }
        q.push_back(v);
    }
}

pub const MONITOR_ROUTE: &str = "monitor";

/// The process table's rows: the filter, busiest first.
pub(super) fn monitor_rows(store: &Store, q: &str) -> Vec<Node> {
    use fs_system::monitor::procs;
    let p = &store.procs;
    if !p.available {
        let note = if p.sampled_ms.is_none() { "Reading processes" } else { "No process table" };
        return vec![Node { dim: Some(true), ..Node::note("monitor.empty", note) }];
    }
    let rows = procs::sort_rows(&procs::filter_rows(&p.rows, q), "cpu");
    if rows.is_empty() {
        return vec![Node { dim: Some(true), ..Node::note("monitor.empty", format!("No process matches \u{201C}{q}\u{201D}")) }];
    }
    rows.iter()
        .take(200)
        .map(|r| Node {
            desc: Some(r.cmd.clone()),
            actions: vec![
                ActionEntry { keep_open: Some(true), ..ActionEntry::new("end", "End", Intent::EndProcess(r.pid)) },
                ActionEntry { keep_open: Some(true), ..ActionEntry::new("kill", "Kill", IpcCall::new("monitor", "kill").arg(r.pid.to_string()).arg("KILL")).destructive() },
            ],
            ..Node::new(format!("proc.{}", r.pid), r.name.clone(), NodeKind::Action)
        })
        .collect()
}

/// The monitor's strip: CPU, Memory, GPU, Disk and Network, each a
/// label over one figure; a reading nobody has taken yet is a dash.
pub(super) fn monitor_strip(store: &Store, theme: &Theme, h: &MonitorHistory) -> El {
    use fs_system::monitor::format;
    let s = &theme.space;
    let m = &store.monitor;
    let gpu = m
        .cards
        .iter()
        .find(|c| c.record.metrics.available && c.record.metrics.busy.is_some())
        .or_else(|| m.cards.first());
    let gpu_figure = match gpu {
        None => "No GPU".to_owned(),
        Some(c) => c.record.metrics.busy.filter(|_| c.record.metrics.available).map_or_else(|| "No metrics".to_owned(), |b| format::pct(Some(b))),
    };
    let disk = m.disk.iter().find(|d| d.mount == "/").or_else(|| m.disk.first());
    let rx: Option<f64> = m.net_available.then(|| m.net.iter().map(|n| n.rx_bytes_per_sec).sum());
    let line = |q: &std::collections::VecDeque<f64>, second: Option<&std::collections::VecDeque<f64>>, ceiling: f64| {
        w::sparkline(q.iter().copied().collect(), second.map_or_else(Vec::new, |s| s.iter().copied().collect()), ceiling, HISTORY)
    };
    let net_ceiling = h.rx.iter().chain(h.tx.iter()).copied().fold(1.0, f64::max);
    // Disk fill has no history worth drawing: a groove instead.
    let tiles = [
        ("CPU", format::pct(m.cpu.aggregate), line(&h.cpu, None, 1.0)),
        ("Memory", format::pct(m.mem.as_ref().map(|x| x.used_fraction)), line(&h.mem, None, 1.0)),
        ("GPU", gpu_figure, line(&h.gpu, None, 1.0)),
        ("Disk", format::pct(disk.map(|d| d.fraction)), w::column(0.0, vec![w::space((s.control_height - s.track_thickness).max(0.0)), w::track(disk.map_or(0.0, |d| d.fraction.clamp(0.0, 1.0)))])),
        ("Network", format::rate(rx), line(&h.rx, Some(&h.tx), net_ceiling)),
    ];
    let cells = tiles
        .into_iter()
        .map(|(label, figure, plot)| {
            w::cell(w::column(
                s.xxs,
                vec![w::section_label(s, label, None, false), w::text(figure).size(Type::Title).mono(), plot],
            ))
            .fill()
        })
        .collect();
    w::row(s.sm, cells).fill().pad(s.sm, 0.0, s.sm, 0.0)
}

/// One process: name and command line, then pid, CPU and memory in mono.
/// An armed row turns destructive under a confirm.
pub(super) fn proc_row(m: &Model, store: &Store, row: &Node, i: usize, selected: bool, theme: &Theme) -> El {
    use fs_system::monitor::format;
    let s = &theme.space;
    let pid = row.id.strip_prefix("proc.").and_then(|p| p.parse::<u64>().ok());
    let armed = pid.is_some_and(|p| m.monitor_armed.is(&p));
    let figures = pid.and_then(|p| store.procs.rows.iter().find(|r| r.pid == p));
    let (pid_s, cpu, mem) = figures.map_or_else(Default::default, |r| (r.pid.to_string(), format::proc_pct(r.cpu_fraction), format::bytes(Some(r.mem_bytes))));
    let name = if armed { format!("End {}? Enter again", row.label) } else { row.label.clone() };
    let mut lead = vec![w::label(name).elide()];
    if row.kind == NodeKind::Note {
        return w::cell(w::text(row.label.clone()).ink(Ink::Dim)).ghost();
    }
    if let Some(cmd) = row.desc.clone().filter(|c| !c.is_empty() && !armed) {
        lead.push(w::text(cmd).size(Type::BodySmall).ink(Ink::Dim).elide());
    }
    // Fixed gutters, so the mono figures line up down the table.
    let figure = |t: &str, px: f64| w::caption(t.to_owned()).mono().ink(Ink::Muted).width(Size::Px(px));
    let trail = vec![figure(&pid_s, 56.0), figure(&cpu, 48.0), figure(&mem, 56.0)];
    let parts = vec![w::row(s.icon_gap, lead).fill(), w::row(s.sm, trail)];
    w::cell(w::row(s.control_padding_x, parts).fill())
        .ghost()
        .selected(selected)
        .interactive()
        .cell_state(|st| st.destructive = armed)
        .on(format!("row:{i}"))
        .stop(format!("r:{}", row.id))
}
