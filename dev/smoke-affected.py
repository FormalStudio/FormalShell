#!/usr/bin/env python3
"""Picks the smoke runs a change needs, off dev/smoke-map.toml.

The change is every file that differs from the merge base of HEAD and
<base> (origin/main by default), committed or not, plus untracked files.
Prints each file's reason and the runs chosen, and writes the runs, one per
line, to --out for dev/remote.sh.

  dev/smoke-affected.py [base] [--out <file>]
"""
import argparse
import os
import re
import subprocess
import sys
import tomllib

root = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))


def git(*args):
    return subprocess.run(["git", "-C", root, *args], check=True, capture_output=True, text=True).stdout


def glob_re(pattern):
    out, i = "", 0
    while i < len(pattern):
        if pattern.startswith("**/", i):
            out, i = out + "(?:.*/)?", i + 3
        elif pattern.startswith("**", i):
            out, i = out + ".*", i + 2
        elif pattern[i] == "*":
            out, i = out + "[^/]*", i + 1
        elif pattern[i] == "?":
            out, i = out + "[^/]", i + 1
        else:
            out, i = out + re.escape(pattern[i]), i + 1
    return re.compile(out + r"\Z")


def matches(path, patterns):
    return any(glob_re(p).match(path) for p in patterns)


def legs():
    """Each leg's flag and whether it needs the VM rig, off dev/smoke.d."""
    out = {}
    d = os.path.join(root, "dev", "smoke.d")
    for name in sorted(os.listdir(d)):
        if not name.endswith(".sh"):
            continue
        leg = name[:-3]
        text = open(os.path.join(d, name)).read()
        flag = re.search(r'^leg_%s_flag="(--[a-z0-9-]+)' % leg, text, re.M)
        if flag:
            vm = re.search(r"^leg_%s_vm_only=" % leg, text, re.M) is not None
            out[leg] = (flag.group(1), vm)
    return out


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("base", nargs="?", default="origin/main")
    ap.add_argument("--out")
    args = ap.parse_args()

    smoke_map = tomllib.load(open(os.path.join(root, "dev", "smoke-map.toml"), "rb"))
    by_leg = legs()
    flags = {flag: (leg, vm) for leg, (flag, vm) in by_leg.items()}
    listed = [l.strip() for l in open(os.path.join(root, "dev", "smoke-all.list")) if l.strip() and not l.startswith("#")]

    def check(run, where):
        for word in run.split():
            if word.startswith("--") and word not in flags:
                sys.exit("smoke-affected: %s names %s, which no leg in dev/smoke.d declares" % (where, word))

    core = smoke_map["core"]
    for run in core:
        check(run, "core")
    rules = smoke_map.get("rule", [])
    for n, rule in enumerate(rules):
        for run in rule["runs"]:
            check(run, "rule %d" % (n + 1))

    base = git("merge-base", "HEAD", args.base).strip()
    changed = set(git("diff", "--name-only", base).split())
    changed |= set(git("ls-files", "--others", "--exclude-standard").split())
    changed = sorted(p for p in changed if not p.startswith(".claude/"))

    chosen = {}  # run -> reasons

    def take(run, why):
        chosen.setdefault(run, [])
        if why not in chosen[run]:
            chosen[run].append(why)

    print("smoke-affected: %d changed files against %s (%s)" % (len(changed), args.base, base[:8]))
    for path in changed:
        if matches(path, smoke_map["none"]["paths"]):
            print("  %-60s no legs" % path)
            continue
        runs, why = [], []
        for rule in rules:
            if matches(path, rule["paths"]):
                runs += rule["runs"]
        if runs:
            why.append("mapped")
        leg_file = re.fullmatch(r"dev/smoke\.d/([a-z0-9_]+)\.sh", path)
        if leg_file and leg_file.group(1) in by_leg:
            flag = by_leg[leg_file.group(1)][0]
            own = [r for r in listed if flag in r.split()]
            if own:
                runs += own
                why.append("its own leg")
            else:
                print("  %-60s not in dev/smoke-all.list, run %s by hand" % (path, flag))
                continue
        if matches(path, smoke_map["shared"]["paths"]):
            runs += core
            why.append("shared, core set")
        if not runs:
            runs = core
            why.append("unmapped, core set")
        print("  %-60s %s" % (path, ", ".join(why)))
        for run in runs:
            take(run, path)

    picked, skipped = [], []
    for run, reasons in chosen.items():
        vm = [w for w in run.split() if w in flags and flags[w][1]]
        (skipped if vm else picked).append((run, reasons))
    if skipped:
        print("smoke-affected: needs the VM rig, not run here: %s" % ", ".join(r for r, _ in skipped))
    if not picked:
        print("smoke-affected: no legs to run")
    else:
        print("smoke-affected: %d runs" % len(picked))
        for run, reasons in picked:
            more = " and %d more" % (len(reasons) - 1) if len(reasons) > 1 else ""
            print("  %-36s %s%s" % (run, reasons[0], more))
    if args.out:
        with open(args.out, "w") as f:
            f.writelines(run + "\n" for run, _ in picked)


main()
