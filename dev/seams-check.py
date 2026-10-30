#!/usr/bin/env python3
"""Reads dev/seams.sh's film: every leg's frames, against
docs/superpowers/specs/2026-10-30-seams.md section 3.

    dev/seams-check.py [artifacts/seams]

Writes <dir>/<leg>-sheet.png (one numbered tile per frame that changed the
picture) and <dir>/report.txt, and exits non-zero on a failure:

  text   tesseract reads console or boot-loader words on a frame (VT1, systemd,
         uwsm, Limine's menu); firmware frames (OVMF's own lines) are listed,
         not judged
  black  black held between two stages past 1.5 s; the firmware's (before the
         first splash, or next to a firmware frame) and the leg's tail excepted

Frame n is n x 100 ms. Needs Pillow and tesseract.
"""
import hashlib
import os
import re
import subprocess
import sys

from PIL import Image, ImageDraw, ImageStat

LEGS = ["boot", "logout", "reboot", "shutdown"]
# A frame is black when nothing on it is brighter than this (the splash averages under 1).
BLACK = 24
HELD = 15
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


def judge(leg, rows):
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
            what = f"black {first:05d} to {run[-1]['n']:05d} ({length * 100} ms)"
            if length > HELD and not before_splash and not tail:
                lines.append(f"FAIL {what}")
                failed = True
            else:
                lines.append(f"  {what}{' (firmware)' if before_splash else ''}{' (to the end)' if tail else ''}")
            run = []
    return lines, failed


def sheet(root, leg, rows):
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
        ImageDraw.Draw(out).text((x + 4, y + 3), f"{r['n']:05d}  {r['n'] / 10:.1f}s  L{r['mean']:.1f}", fill=(220, 220, 220))
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
        lines, bad = judge(leg, rows)
        failed |= bad
        report.append(f"{leg}: {len(rows)} frames, {sum(r['changed'] for r in rows)} changes, {'FAIL' if bad else 'ok'}, {sheet(root, leg, rows)}")
        report += lines
    text = "\n".join(report) + "\n"
    with open(os.path.join(root, "report.txt"), "w") as f:
        f.write(text)
    print(text, end="")
    sys.exit(1 if failed else 0)


if __name__ == "__main__":
    main()
