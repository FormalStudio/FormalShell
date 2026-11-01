#!/usr/bin/env python3
"""Reads a burst of frames taken while a list changed under real input
against the frames it settles on, and the row boxes `menu status` reported
through it.

    settled-frames.py frames <convert> <scale> <box.json> <settled.png>... -- <burst.png>...
    settled-frames.py rows <status.json>...

`frames` crops every picture to the box (`{"x","y","width","height"}` in
the output's logical pixels, read at `scale`) and prints, per burst frame,
the settled frame it matches and the share of pixels that differ from it. It exits
1 when a burst frame matches none: a frame that is neither the list before
a key nor the list after it is a transition, an overlap or a stale row.

`rows` exits 1 when two of a status's `rowBoxes` intersect, or a row is cut
by anything but the body's scroll viewport: every row inside the body's
width, and inside its height unless it crosses the body's top or bottom
edge.
"""
import json
import subprocess
import sys

FUZZ = "3%"
MAX_DIFF = 0.0005


def crop(convert, path, box, scale):
    x, y = round(box["x"] * scale), round(box["y"] * scale)
    w, h = round(box["width"] * scale), round(box["height"] * scale)
    out = path[: -len(".png")] + ".crop.png"
    subprocess.run([convert, path, "-crop", f"{w}x{h}+{x}+{y}", "+repage", "-alpha", "off", out], check=True)
    return out


def differing(convert, a, b):
    """The share of the box's pixels that differ past FUZZ."""
    out = subprocess.run([convert, a, b, "-compose", "difference", "-composite", "-colorspace", "Gray", "-threshold", FUZZ, "-format", "%[fx:mean]", "info:"], capture_output=True, text=True)
    try:
        return float(out.stdout.strip())
    except ValueError:
        return 1.0


def frames(args):
    convert, scale, box_path = args[0], float(args[1]), args[2]
    split = args.index("--")
    settled, burst = args[3:split], args[split + 1:]
    box = json.load(open(box_path))
    ref = [(p, crop(convert, p, box, scale)) for p in settled]
    bad = 0
    for p in burst:
        c = crop(convert, p, box, scale)
        best = min(((differing(convert, c, r), name) for name, r in ref), key=lambda t: t[0])
        ok = best[0] <= MAX_DIFF
        bad += not ok
        print(f"{'ok' if ok else 'NONE'} {p.rsplit('/', 1)[-1]} {best[1].rsplit('/', 1)[-1]} {best[0]:.5f}")
    return 1 if bad else 0


def rows(args):
    bad = 0
    for path in args:
        s = json.load(open(path))
        body, boxes = s.get("body"), s.get("rowBoxes", [])
        name = path.rsplit("/", 1)[-1]
        if not body:
            continue
        bx0, by0 = body["x"], body["y"]
        bx1, by1 = bx0 + body["width"], by0 + body["height"]
        for i, a in enumerate(boxes):
            ax0, ay0, ax1, ay1 = a["x"], a["y"], a["x"] + a["width"], a["y"] + a["height"]
            if ax0 < bx0 - 1 or ax1 > bx1 + 1:
                print(f"{name}: row {a['index']} runs past the body's sides: {a} in {body}")
                bad += 1
            inside = ay0 >= by0 - 1 and ay1 <= by1 + 1
            crosses = ay0 < by0 < ay1 or ay0 < by1 < ay1
            if not inside and not crosses:
                print(f"{name}: row {a['index']} is drawn outside the body: {a} in {body}")
                bad += 1
            for b in boxes[i + 1:]:
                bx_0, by_0, bx_1, by_1 = b["x"], b["y"], b["x"] + b["width"], b["y"] + b["height"]
                if min(ax1, bx_1) - max(ax0, bx_0) > 0.5 and min(ay1, by_1) - max(ay0, by_0) > 0.5:
                    print(f"{name}: rows {a['index']} and {b['index']} intersect: {a} {b}")
                    bad += 1
    print(f"rows: {len(args)} statuses, {bad} faults")
    return 1 if bad else 0


def main():
    if len(sys.argv) < 2 or sys.argv[1] not in ("frames", "rows"):
        print(__doc__, file=sys.stderr)
        return 2
    return frames(sys.argv[2:]) if sys.argv[1] == "frames" else rows(sys.argv[2:])


if __name__ == "__main__":
    sys.exit(main())
