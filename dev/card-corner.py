#!/usr/bin/env python3
"""A dark card's rim read off a frame at its own pixels.

usage: card-corner.py <png> <name> <x> <y>
       card-corner.py <png> <name> --over <frame without the card>

The card is dark, over a light flat wallpaper. It is named by a pixel inside
its fill, left of its right edge and below its top edge, or found as what
went dark between a frame without it and this one (a panel, the launcher),
when all four edges are read, each from the middle of its own padding.

Each edge is read along its normal: the border has to be one pixel at full
depth with the wallpaper side of it clearly lighter (a line on a part pixel
spreads over two at half depth, which reads as a soft doubled rim). Then the
top-right arc: nothing lit just inside its border, read down the corner's
diagonal, brighter than the top
edge's own line, the brightest one the card draws.

Prints one line of numbers and exits non-zero naming the claim that missed.
"""
import sys

from PIL import Image

png, name = sys.argv[1], sys.argv[2]
img = Image.open(png).convert("L")
w, h = img.size
px = img.load()

# The darkest pixel a border of black at 0.75 over the dark fill gives,
# with room for a light theme's lighter fill.
DEEP = 40
# A part pixel of the border over the wallpaper sits between the two; the
# pixel past a crisp one is the wallpaper under the card's cast.
CLEAR = 40
# How much lighter the cast leaves the next pixel out.
SHADE = 24
# Darker than any cast over the wallpaper: the card itself.
CARD = 100

if sys.argv[3] == "--over":
    before = Image.open(sys.argv[4]).convert("L").load()
    dark = [(i, j) for j in range(h) for i in range(w) if px[i, j] < CARD and before[i, j] >= CARD]
    if not dark:
        sys.exit(f"FAIL {name}: nothing went dark between the two frames")
    # The card is the run of rows and columns that went dark across most
    # of its own width: a scrim's darkened specks elsewhere are left out.
    rows, cols = [0] * h, [0] * w
    for i, j in dark:
        rows[j] += 1
        cols[i] += 1
    dense = lambda counts: [k for k, n in enumerate(counts) if n > max(counts) / 2]
    x0, x1 = min(dense(cols)), max(dense(cols))
    y0, y1 = min(dense(rows)), max(dense(rows))
    top_at, right_at = ((x0 + x1) // 2, y0 + 4), (x1 - 4, (y0 + y1) // 2)
    sides = [((x0 + 4, (y0 + y1) // 2), (-1, 0), "left"), (((x0 + x1) // 2, y1 - 4), (0, 1), "bottom")]
else:
    top_at = right_at = (int(sys.argv[3]), int(sys.argv[4]))
    sides = []

fails = []


def edge(start, step, label):
    """The rim walking out of the fill at `start` along `step`."""
    fill = px[start]
    values = []
    x, y = start
    while 0 <= x < w and 0 <= y < h:
        values.append(px[x, y])
        x, y = x + step[0], y + step[1]
    out = next((i for i, v in enumerate(values) if v > fill + 60), None)
    if out is None or out < 2:
        fails.append(f"{label}: no wallpaper past the card ({values[:12]})")
        return None
    border, lip, outside = values[out - 1], values[out - 2], values[out]
    if border > DEEP:
        fails.append(f"{label}: the border pixel is {border}, not a whole one (spread over two)")
    if outside - border < CLEAR:
        fails.append(f"{label}: the pixel past the border is {outside}, a half-drawn border beside {border}")
    # Past a whole border the wallpaper starts at once, give or take the
    # cast's own falloff; a border on a part pixel leaves one in between.
    beyond = values[out + 1] if out + 1 < len(values) else outside
    if beyond - outside > SHADE:
        fails.append(f"{label}: the pixel past the border is {outside} before the wallpaper's {beyond}, part of the border")
    at = (start[0] + step[0] * (out - 1), start[1] + step[1] * (out - 1))
    return {"fill": fill, "border": border, "lip": lip, "outside": outside, "at": at}


top = edge(top_at, (0, -1), f"{name} top")
right = edge(right_at, (1, 0), f"{name} right")
others = {label: edge(at, step, f"{name} {label}") for at, step, label in sides}
arc = None
if top and right:
    # Down the corner's diagonal from the outside in, and the two beside
    # it: past the antialiased arc over the wallpaper, the darkest pixel is
    # the border and the two after it are what lights the arc.
    x1, y0 = right["at"][0], top["at"][1]
    lit = []
    for shift in (-1, 0, 1):
        line = [px[x1 - t + shift, y0 + t] for t in range(40) if 0 <= x1 - t + shift < w and y0 + t < h]
        deepest = line.index(min(line[:30]))
        lit += line[deepest + 1 : deepest + 3]
    arc = max(lit) if lit else None
    if arc is None:
        fails.append(f"{name} arc: no corner found off the border")
    elif arc > top["lip"] + 6:
        fails.append(f"{name} arc: lit to {arc}, past the top edge's line {top['lip']}")

print(f"{name}: top={top} right={right} arc={arc}" + "".join(f" {k}={v}" for k, v in others.items()))
if fails:
    print("FAIL " + "; ".join(fails))
    sys.exit(1)
