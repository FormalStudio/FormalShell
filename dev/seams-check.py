#!/usr/bin/env python3
"""Reads dev/seams.sh's film: every leg's frames, against
docs/superpowers/specs/2026-10-30-seams.md section 3.

    dev/seams-check.py [artifacts/seams]

Writes <dir>/<leg>-sheet.png (one numbered tile per frame that changed the
picture) and <dir>/report.txt, and exits non-zero on a failure:

  text   tesseract reads console or boot-loader words on a frame (VT1, systemd,
         uwsm, Limine's menu); firmware frames (OVMF's own lines) are listed,
         not judged
  black  black held between two stages past 1.5 s, or on the boot leg past
         100 ms (the locked login, 2026-10-30-locked-login.md); the
         firmware's (before the first splash, or next to a firmware frame)
         and the leg's tail excepted
  cells  the boot leg carries frames part way through both of the shell's
         pixel dissolves (2026-10-30-locked-login.md, "The pixel dissolve"):
         two runs of frames whose picture stands in square cells of two
         colours, the splash into the lock and the lock into the desktop

Frame n is n x 100 ms, or n x <leg>/every-ms where dev/seams.sh filmed a
leg faster. Needs Pillow and tesseract.
"""
import hashlib
import os
import re
import subprocess
import sys

from PIL import Image, ImageChops, ImageDraw, ImageStat

LEGS = ["boot", "logout", "reboot", "shutdown"]
# A frame is black when nothing on it is brighter than this (the splash averages under 1).
BLACK = 24
# The longest black a leg may hold between two stages, in ms.
HELD_MS = {"boot": 100}


def every_ms(root, leg):
    try:
        with open(os.path.join(root, leg, "every-ms")) as f:
            return int(f.read())
    except OSError:
        return 100
SPLASH = re.compile(r"Formal\w{0,2}S")
FIRMWARE = re.compile(r"BdsDxe|TianoCore|Guest has not", re.I)
CONSOLE = re.compile(
    r"systemd|Started|Stopped|Stopping|Reached|Starting|Created|Reloading|uwsm|Command Line|"
    r"Desktop Names|Forked|power off|Kernels|Booting automatic|newest kernel|login:|"
    r"\.service|\.target|\.scope",
    re.I,
)


def ocr(path):
    out = subprocess.run(["tesseract", path, "-", "--psm", "11"], capture_output=True, text=True)
    return " ".join(out.stdout.split())


def frames(d):
    return sorted(f for f in os.listdir(d) if f.endswith(".png"))


def read_leg(root, leg):
    d = os.path.join(root, leg, "frames")
    if not os.path.isdir(d):
        return None
    rows, last, texts = [], None, {}
    for f in frames(d):
        im = Image.open(os.path.join(d, f)).convert("L")
        key = hashlib.md5(im.resize((160, 100)).tobytes()).hexdigest()
        mean = ImageStat.Stat(im).mean[0]
        black = im.getextrema()[1] < BLACK
        if key not in texts:
            texts[key] = "" if black else ocr(os.path.join(d, f))
        rows.append({"n": int(f[:-4]), "file": f, "mean": mean, "black": black, "key": key, "changed": key != last, "text": texts[key]})
        last = key
    return rows


def cells(path):
    """Whether the frame stands in square cells of more than one picture:
    two flat colours each over 5% of it, one not black, and its edges on a
    few columns and rows, as a grid of cells puts them. The guest's output
    reaches the screendump scaled, so the cells are not on the frame's own
    32 px grid. A photograph, a blurred wallpaper and a card with text on it
    have edges everywhere or no second flat colour."""
    im = Image.open(path).convert("RGB")
    w, h = im.size
    colours = im.getcolors(1 << 16) or []
    big = [c for n, c in colours if n * 20 >= w * h]
    if len(big) < 2 or not any(max(c) > 0 for c in big):
        return False
    g = im.convert("L")

    def lined(diff, n, length, along):
        edges = diff.point(lambda v: 255 if v > 3 else 0).resize(along, Image.BOX)
        counts = sorted((v * length / 255 for v in edges.get_flattened_data()), reverse=True)
        total = sum(counts)
        # A boundary scaled by the guest's output spreads over two lines.
        return total >= length and sum(counts[: 2 * (n // 28)]) >= total * 0.8

    dx = ImageChops.difference(g.crop((0, 0, w - 1, h)), g.crop((1, 0, w, h)))
    dy = ImageChops.difference(g.crop((0, 0, w, h - 1)), g.crop((0, 1, w, h)))
    return lined(dx, w, h, (w - 1, 1)) and lined(dy, h, w, (1, h - 1))


def dissolves(root, leg, rows):
    """Runs of frames standing in cells: (first, last, frames)."""
    d = os.path.join(root, leg, "frames")
    runs, run = [], []
    for r in rows:
        hit = cells(os.path.join(d, r["file"])) if r["changed"] else bool(run)
        if hit:
            run.append(r["n"])
        elif run:
            runs.append((run[0], run[-1], len(run)))
            run = []
    if run:
        runs.append((run[0], run[-1], len(run)))
    return runs


def judge(leg, rows, every):
    lines, failed = [], False
    splash = next((r["n"] for r in rows if SPLASH.search(r["text"]) and not CONSOLE.search(r["text"])), None)
    for r in rows:
        if not r["changed"] or not r["text"]:
            continue
        if FIRMWARE.search(r["text"]):
            lines.append(f"  firmware {r['n']:05d}: {r['text'][:90]}")
        elif CONSOLE.search(r["text"]):
            lines.append(f"FAIL text {r['n']:05d}: {r['text'][:90]}")
            failed = True
    firmware = [bool(FIRMWARE.search(r["text"])) for r in rows]
    run = []
    for i, r in enumerate(rows + [None]):
        if r is not None and r["black"]:
            run.append(r)
            continue
        if run:
            first, length = run[0]["n"], len(run)
            # The firmware owns the screen around its own frames, and before the first splash.
            start = i - len(run)
            before_splash = (leg == "boot" and (splash is None or first < splash)) or (start > 0 and firmware[start - 1]) or (r is not None and firmware[i])
            tail = r is None
            what = f"black {first:05d} to {run[-1]['n']:05d} ({length * every} ms)"
            if length * every > HELD_MS.get(leg, 1500) and not before_splash and not tail:
                lines.append(f"FAIL {what}")
                failed = True
            else:
                lines.append(f"  {what}{' (firmware)' if before_splash else ''}{' (to the end)' if tail else ''}")
            run = []
    return lines, failed


def sheet(root, leg, rows, every):
    d = os.path.join(root, leg, "frames")
    tiles = [r for r in rows if r["changed"]]
    if len(tiles) > 80:
        step = len(tiles) / 80
        tiles = [tiles[int(i * step)] for i in range(80)]
    w, h, cols = 320, 200, 8
    out = Image.new("RGB", (cols * w, ((len(tiles) + cols - 1) // cols) * (h + 18)), (24, 24, 24))
    for i, r in enumerate(tiles):
        im = Image.open(os.path.join(d, r["file"])).convert("RGB").resize((w, h))
        x, y = (i % cols) * w, (i // cols) * (h + 18)
        out.paste(im, (x, y + 18))
        ImageDraw.Draw(out).text((x + 4, y + 3), f"{r['n']:05d}  {r['n'] * every / 1000:.2f}s  L{r['mean']:.1f}", fill=(220, 220, 220))
    path = os.path.join(root, f"{leg}-sheet.png")
    out.save(path)
    return path


def main():
    root = sys.argv[1] if len(sys.argv) > 1 else "artifacts/seams"
    report, failed = [], False
    for leg in LEGS:
        rows = read_leg(root, leg)
        if rows is None:
            report.append(f"{leg}: no frames")
            continue
        every = every_ms(root, leg)
        lines, bad = judge(leg, rows, every)
        if leg == "boot":
            runs = dissolves(root, leg, rows)
            for first, last, n in runs:
                lines.append(f"  cells {first:05d} to {last:05d} ({n} frames, {n * every} ms)")
            if len(runs) < 2:
                lines.append(f"FAIL cells: {len(runs)} runs of cell frames, where the splash's and the unlock's dissolves make two")
                bad = True
        failed |= bad
        report.append(f"{leg}: {len(rows)} frames, {sum(r['changed'] for r in rows)} changes, {'FAIL' if bad else 'ok'}, {sheet(root, leg, rows, every)}")
        report += lines
    text = "\n".join(report) + "\n"
    with open(os.path.join(root, "report.txt"), "w") as f:
        f.write(text)
    print(text, end="")
    sys.exit(1 if failed else 0)


if __name__ == "__main__":
    main()
