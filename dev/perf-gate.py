#!/usr/bin/env python3
"""The one-look spec's performance gate (docs/superpowers/specs/
2026-10-29-one-look.md section 5) against docs/perf/baseline.json.

Reads the stdout of `dev/remote.sh smoke --idle`, `--r0-measure 300` and
`--menu-budget` runs saved one per file, plain or under FS_CPU_QUOTA (told
apart by the `shell cpu quota:` line smoke.sh prints). A run that failed is
refused, never counted. Each backend has a baseline of its own, the CPU's
docs/perf/baseline.json and the GPU's docs/perf/baseline-gpu.json, told
apart by the `SMOKE_BACKEND` line `--backend` prints (a run without one
drew on the CPU); a run asking for the GPU that fell back is refused.

  dev/perf-gate.py run [--backend cpu|gpu] [--legs idle,r0,menu] [--runs 3]
      the matrix on the rig, each leg --runs times plain and under quota
      (--idle at least five times) on that backend (cpu unless asked),
      outputs kept in artifacts/perf/<stamp>/, then `check` over them
  dev/perf-gate.py check <run-output>...
      the median of the given runs per number against the baseline; exits 1
      when any number is over its gate
  dev/perf-gate.py extract <run-output>...
      the numbers each file carries, as JSON
  dev/perf-gate.py baseline --commit <sha> [--note <text>] <run-output>...
      writes docs/perf/baseline.json from three runs per leg and mode

The gate, per number: the median of the new runs at or under the baseline
median plus the baseline's spread (max minus min of its three runs). RSS
gets 2 MB over the median instead, the idle commit count and the shell's
engine time stay exactly 0, the compositor's engine time stays at or under
its median, and the DRM fd count is reported only.

Each run carries g815's ACPI platform profile, read just before it, and a
check refuses runs read on another profile than the baseline's: every CPU
number moves with it (the baseline's own commit reads its launcher first
commit at 15 to 25 ms on balanced against 8 to 9 on performance), and the
shell's Overnight mode drops performance to balanced until it is turned off.
`run` stops before a run on the wrong one.

The compositor's engine time is read on the median of five --idle runs:
over a window the shell commits once in, it is one or two of the nested
Hyprland's frames, and the same build reads 595k to 705k ns from run to run
on g815 (the iGPU's power state), so three runs leave the median to luck.
"""
import json
import os
import re
import subprocess
import sys
import time

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
BASELINES = {"cpu": os.path.join(REPO, "docs/perf/baseline.json"), "gpu": os.path.join(REPO, "docs/perf/baseline-gpu.json")}
QUOTA_ENV = {"FS_CPU_QUOTA": "10%", "FS_CPU_QUOTA_PERIOD": "2ms"}
LEGS = {"idle": ["--idle"], "r0": ["--r0-measure", "300"], "menu": ["--menu-budget"]}
MIN_RUNS = {"idle": 5}

# number -> (leg flag, rule)
NUMBERS = {
    "cold_start_ms": ("--idle", "spread"),
    "idle_ticks": ("--idle", "spread"),
    "idle_wakeups": ("--idle", "spread"),
    "idle_commits": ("--idle", "zero"),
    "shell_engine_ns": ("--idle", "zero"),
    "shell_drm_fds": ("--idle", "report"),
    "compositor_engine_ns": ("--idle", "median"),
    "idle_cpu_pct": ("--r0-measure 300", "spread"),
    "spinner_cpu_pct": ("--r0-measure 300", "spread"),
    "rss_kb": ("--r0-measure 300", "rss"),
    "threads": ("--r0-measure 300", "spread"),
    "panel_first_commit_ms_p50": ("--r0-measure 300", "spread"),
    "panel_first_commit_ms_max": ("--r0-measure 300", "spread"),
    "panel_render_us_max": ("--r0-measure 300", "spread"),
    "scrim_first_commit_ms_p50": ("--r0-measure 300", "spread"),
    "scrim_first_commit_ms_max": ("--r0-measure 300", "spread"),
    "scrim_render_us_max": ("--r0-measure 300", "spread"),
    "launcher_first_commit_ms_p50": ("--menu-budget", "spread"),
    "launcher_first_commit_ms_max": ("--menu-budget", "spread"),
    "bar_gap_ms_max": ("--menu-budget", "spread"),
}
RSS_ALLOWANCE_KB = 2048
PROFILE_LINE = "host platform_profile: "


def host_profile():
    out = subprocess.run(["ssh", "-o", "BatchMode=yes", "-o", "ConnectTimeout=10", "-o", "LogLevel=ERROR", "g815", "cat /sys/firmware/acpi/platform_profile"],
                         capture_output=True, text=True)
    if out.returncode != 0:
        raise SystemExit(f"reading g815's platform profile: {out.stderr.strip()}")
    return out.stdout.strip()


def kv(line):
    return {k: v for k, v in re.findall(r"([A-Za-z_0-9]+):?=(-?[0-9.]+)", line)}


def median(xs):
    s = sorted(xs)
    n = len(s)
    return s[n // 2] if n % 2 else (s[n // 2 - 1] + s[n // 2]) / 2


def extract(path):
    text = open(path, errors="replace").read()
    if "SMOKE_FAIL" in text or "smoke run failed" in text or "SMOKE_OK " not in text:
        raise SystemExit(f"{path}: the run failed, not counted")
    m = re.search(r"^shell cpu quota: (\S+) per (\S+)", text, re.M)
    mode = "quota" if m else "plain"
    if m and (m.group(1), m.group(2)) != (QUOTA_ENV["FS_CPU_QUOTA"], QUOTA_ENV["FS_CPU_QUOTA_PERIOD"]):
        raise SystemExit(f"{path}: quota {m.group(1)} per {m.group(2)}, the baseline's is 10% per 2ms")
    n = {}
    lines = text.splitlines()
    profile = next((l[len(PROFILE_LINE):].strip() for l in lines if l.startswith(PROFILE_LINE)), None)
    backend = "cpu"
    told = next((l for l in lines if l.startswith("SMOKE_BACKEND ")), None)
    if told:
        b = json.loads(told.split(" ", 1)[1])
        if b["backend"] != b["asked"]:
            raise SystemExit(f"{path}: asked for {b['asked']} and drew on {b['backend']} ({b.get('reason')}), not counted")
        backend = b["backend"]

    def pick(prefix):
        return next((l for l in lines if l.startswith(prefix)), None)

    if pick("SMOKE_IDLE_LOG "):
        zero = kv(pick("SMOKE_IDLE launch_to_zero_ms=") or "")
        first = kv(pick("SMOKE_IDLE first_bar_commit_ms=") or "")
        total = kv(pick("SMOKE_IDLE total ticks=") or "")
        commits = re.match(r"SMOKE_IDLE commits (\d+)", pick("SMOKE_IDLE commits ") or "")
        gpu = kv(pick("SMOKE_IDLE gpu ") or "")
        if not (zero and first and total and commits and "compositor_engine_ns" in gpu):
            raise SystemExit(f"{path}: an --idle run missing one of its numbers")
        n["cold_start_ms"] = int(zero["launch_to_zero_ms"]) + int(first["first_bar_commit_ms"])
        n["idle_ticks"] = int(total["ticks"])
        n["idle_wakeups"] = int(total["wakeups"])
        n["idle_commits"] = int(commits.group(1))
        n["shell_engine_ns"] = int(gpu["shell_engine_ns"])
        n["shell_drm_fds"] = int(gpu["shell_drm_fds"])
        n["compositor_engine_ns"] = int(gpu["compositor_engine_ns"])
    if pick("SMOKE_R0_RESULTS "):
        for step in ("idle", "spinner"):
            c = kv(pick(f"cpu {step} ") or "")
            if not c:
                raise SystemExit(f"{path}: no `cpu {step}` line")
            secs = int(c["wall_ns"]) / 1e9
            n[f"{step}_cpu_pct"] = round(int(c["self_ticks"]) / int(c["clk_tck"]) / secs * 100, 3)
        rss = kv(pick("rss ") or "")
        if "VmRSS" not in rss:
            raise SystemExit(f"{path}: no `rss` line")
        n["rss_kb"] = int(rss["VmRSS"])
        n["threads"] = int(rss["Threads"])
        for surface in ("panel", "scrim"):
            s = kv(pick(f"{surface} opens=") or "")
            if not s or s["committed"] != s["opens"] or s["opens"] != "10":
                raise SystemExit(f"{path}: the {surface} step did not commit on all ten opens: {s}")
            n[f"{surface}_first_commit_ms_p50"] = int(s["first_commit_ms_p50"])
            n[f"{surface}_first_commit_ms_max"] = int(s["first_commit_ms_max"])
            n[f"{surface}_render_us_max"] = int(s["render_us_max"])
    if pick("SMOKE_MENU_BUDGET_LOG "):
        opens = [kv(l) for l in lines if re.match(r"SMOKE_MENU_BUDGET open \d+ ", l)]
        worst = kv(pick("SMOKE_MENU_BUDGET worst ") or "")
        if len(opens) != 5 or not worst:
            raise SystemExit(f"{path}: --menu-budget printed {len(opens)} opens, wanted 5")
        n["launcher_first_commit_ms_p50"] = median([int(o["first_commit_ms"]) for o in opens])
        n["launcher_first_commit_ms_max"] = int(worst["first_commit_ms"])
        n["bar_gap_ms_max"] = int(worst["bar_gap_ms"])
    if not n:
        raise SystemExit(f"{path}: carries no --idle, --r0-measure or --menu-budget output")
    return backend, mode, n, profile


def group(paths):
    """The runs per mode and number, the one backend they all drew on, and
    the platform profiles they were read on (None for a run that predates
    the line)."""
    runs, backends, profiles = {"plain": {}, "quota": {}}, set(), set()
    for p in paths:
        backend, mode, n, profile = extract(p)
        backends.add(backend)
        profiles.add(profile)
        for k, v in n.items():
            runs[mode].setdefault(k, []).append(v)
    if len(backends) != 1:
        raise SystemExit(f"runs drawn on {', '.join(sorted(backends))}: one backend at a time")
    return backends.pop(), runs, profiles


def cmd_extract(paths):
    print(json.dumps({p: dict(zip(("backend", "mode", "numbers", "platform_profile"), extract(p))) for p in paths}, indent=2))


def cmd_baseline(args):
    if len(args) < 3 or args[0] != "--commit":
        raise SystemExit("usage: perf-gate.py baseline --commit <sha> [--note <text>] <run-output>...")
    commit, paths, note = args[1], args[2:], None
    if paths[:1] == ["--note"] and len(paths) > 2:
        note, paths = paths[1], paths[2:]
    backend, runs, profiles = group(paths)
    if len(profiles) != 1 or None in profiles:
        raise SystemExit(f"a baseline takes runs read on one platform profile, these were read on {sorted(map(str, profiles))}")
    out = {
        "commit": commit,
        "host": "g815",
        "platform_profile": profiles.pop(),
        "backend": backend,
        "taken": time.strftime("%Y-%m-%d"),
        **({"note": note} if note else {}),
        "modes": {
            "plain": {"quota": None, "numbers": {}},
            "quota": {"quota": QUOTA_ENV, "numbers": {}},
        },
    }
    for mode, numbers in runs.items():
        for k, (leg, rule) in NUMBERS.items():
            xs = numbers.get(k, [])
            if len(xs) != 3:
                raise SystemExit(f"{mode} {k}: {len(xs)} runs, a baseline takes three")
            out["modes"][mode]["numbers"][k] = {
                "leg": leg, "gate": rule, "runs": xs,
                "median": median(xs), "spread": max(xs) - min(xs),
            }
    os.makedirs(os.path.dirname(BASELINES[backend]), exist_ok=True)
    with open(BASELINES[backend], "w") as f:
        json.dump(out, f, indent=2)
        f.write("\n")
    print(f"wrote {BASELINES[backend]}")


def cmd_check(paths):
    backend, runs, profiles = group(paths)
    base = json.load(open(BASELINES[backend]))
    other = sorted(p for p in profiles if p is not None and p != base["platform_profile"])
    if other:
        raise SystemExit(f"runs read on the {', '.join(other)} platform profile, the baseline's is {base['platform_profile']}: the CPU numbers do not compare")
    over = []
    print(f"{backend} baseline {base['commit'][:12]} on {base['host']}")
    for mode, numbers in runs.items():
        for k, xs in sorted(numbers.items(), key=lambda kv: list(NUMBERS).index(kv[0])):
            b = base["modes"][mode]["numbers"][k]
            m = median(xs)
            rule = b["gate"]
            limit = {
                "spread": b["median"] + b["spread"],
                "rss": b["median"] + RSS_ALLOWANCE_KB,
                "zero": 0,
                "median": b["median"],
                "report": None,
            }[rule]
            ok = limit is None or m <= limit
            if not ok:
                over.append(f"{mode} {k}")
            shown = "reported" if limit is None else f"gate {limit:g}"
            print(f"{'ok  ' if ok else 'OVER'} {mode:5} {k:30} median {m:g} of {xs} ({shown}, baseline {b['median']:g})")
            if len(xs) < 3:
                print(f"     {mode} {k}: {len(xs)} runs, the gate is read on three or more")
    if over:
        print("over the gate: " + ", ".join(over))
        sys.exit(1)


def cmd_run(args):
    legs, count, backend = list(LEGS), 3, "cpu"
    while args:
        if args[0] == "--backend" and len(args) > 1 and args[1] in BASELINES:
            backend = args[1]
            args = args[2:]
        elif args[0] == "--legs" and len(args) > 1:
            legs = args[1].split(",")
            args = args[2:]
        elif args[0] == "--runs" and len(args) > 1:
            count = int(args[1])
            args = args[2:]
        else:
            raise SystemExit("usage: perf-gate.py run [--backend cpu|gpu] [--legs idle,r0,menu] [--runs 3]")
    for leg in legs:
        if leg not in LEGS:
            raise SystemExit(f"unknown leg {leg}, one of {', '.join(LEGS)}")
    want = json.load(open(BASELINES[backend]))["platform_profile"]
    out = os.path.join(REPO, "artifacts/perf", time.strftime("%Y%m%d-%H%M%S"))
    os.makedirs(out)
    paths = []
    # Interleaved, so a drift on the host over the hour lands on every leg
    # and both modes rather than on whichever ran last.
    runs = {leg: max(count, MIN_RUNS.get(leg, 0)) for leg in legs}
    for i in range(1, max(runs.values()) + 1):
        for leg in (leg for leg in legs if i <= runs[leg]):
            for mode in ("plain", "quota"):
                # The baseline is g815's; never the mac rig's fallback.
                env = dict(os.environ, FS_RIG="g815")
                for k in QUOTA_ENV:
                    env.pop(k, None)
                if mode == "quota":
                    env.update(QUOTA_ENV)
                path = os.path.join(out, f"{leg}-{backend}-{mode}-{i}.txt")
                profile = host_profile()
                if profile != want:
                    raise SystemExit(f"g815 is on the {profile} platform profile, the baseline's is {want} (Overnight drops performance to balanced): stopped before {leg} {mode} {i}")
                print(f"{time.strftime('%H:%M:%S')} {leg} {mode} {i}", flush=True)
                with open(path, "w") as f:
                    f.write(PROFILE_LINE + profile + "\n")
                    f.flush()
                    status = subprocess.call([os.path.join(REPO, "dev/remote.sh"), "smoke", "--backend", backend, *LEGS[leg]],
                                             stdout=f, stderr=subprocess.STDOUT, env=env, cwd=REPO)
                if status != 0:
                    raise SystemExit(f"{path}: the run failed (exit {status})")
                paths.append(path)
    cmd_check(paths)


def main():
    if len(sys.argv) < 2:
        raise SystemExit(__doc__)
    verb, args = sys.argv[1], sys.argv[2:]
    if verb == "run":
        cmd_run(args)
    elif verb in ("check", "extract") and args:
        {"check": cmd_check, "extract": cmd_extract}[verb](args)
    elif verb == "baseline":
        cmd_baseline(args)
    else:
        raise SystemExit(__doc__)


if __name__ == "__main__":
    main()
