#!/usr/bin/env python3
"""The panel language (docs/superpowers/specs/2026-10-30-panel-language.md)
read off one frame of one card, for every --panel and --settings-ui shot.

    panel-language.py <png> <rect json> <header yes|no> <inset> <name> <row> <scale>

The rect is the card's own (`debug dump`'s `.panel.rest`, `menu status`'s
`cardBox`), in logical pixels from the output's origin; it and the insets
are multiplied by the output's scale. Four claims, each printed, any
failure exiting 1:

- header: the gap from the card's top edge to the title's ink and from its
  baseline to the hairline agree within 2px (a headerless card skips it).
- column: no ink between the card's edge and the text column, `inset` in,
  on either side below the header; only the 3px of the card's own border
  and the corner radius are left out.
- bottom: no ink in the last `inset` rows, and a body ending on a box (a
  group, a button) ends it `inset` above the edge; one ending on words
  (an empty line, a caption) has them inside the last row's band.
- one inset level: no two box borders standing side by side down the text
  column's left edge, which is what a box inside a group draws.
- even insets: the room from the card's left edge to its first ink equals
  the room after its last ink, and on a headerless card (a strip of chips)
  the room above equals the room below, within 1px; a header card's is
  read when its body closes on a box (words end inside a row's band).

Ink is anything off the card's fill, the fill being the commonest colour
down each gutter, taken per side so a rim leaning one way does not count. The pointer stays off the card in these
runs, so no hover wash counts against the column.
"""
import json
import sys
from collections import Counter

from PIL import Image

RADIUS = 14
INK = 24
LINE = 12
RUN = 18

png, rect, header, name = sys.argv[1], sys.argv[2], sys.argv[3] == "yes", sys.argv[5]
scale = float(sys.argv[7])
inset, band = round(int(sys.argv[4]) * scale), round(int(sys.argv[6]) * scale)
RADIUS = round(RADIUS * scale)
im = Image.open(png).convert("RGB")
px = im.load()
r = json.loads(rect)
x0, y0 = round(r["x"] * scale), round(r["y"] * scale)
x1, y1 = x0 + round(r["width"] * scale) - 1, y0 + round(r["height"] * scale) - 1
x1, y1 = min(x1, im.width - 1), min(y1, im.height - 1)
failed = []


def d(a, b):
    return sum(abs(p - q) for p, q in zip(a, b))


def say(check, ok, detail):
    print(f"panel-language {name}: {check} {'ok' if ok else 'FAIL'}, {detail}")
    if not ok:
        failed.append(check)


def gutter(x):
    return Counter(px[x, y] for y in range(y0 + RADIUS, y1 - RADIUS)).most_common(1)[0][0]


fill = gutter(x0 + 5)
fills = {"left": fill, "right": gutter(x1 - 5)}

body_top = y0 + RADIUS
if header:
    w = x1 - x0 + 1
    line = None
    for y in range(y0 + 8, min(y0 + 120, y1)):
        row = [px[x, y] for x in range(x0 + w // 4, x0 + 3 * w // 4)]
        if all(d(c, fill) > 6 for c in row) and max(c[0] for c in row) - min(c[0] for c in row) < 12:
            line = y
            break
    if line is None:
        say("header", False, "no hairline under the title")
    else:
        counts = [(y, sum(1 for x in range(x0 + 12, x0 + w // 2) if d(px[x, y], fill) > 120)) for y in range(y0 + 1, line)]
        ink = [y for y, n in counts if n]
        if not ink:
            say("header", False, "a hairline but no title ink above it")
        else:
            peak = max(n for _, n in counts)
            base = max(y for y, n in counts if n >= peak * 0.3)
            above, below = ink[0] - y0, line - base - 1
            say("header", abs(above - below) <= 2, f"{above}px above the title, {below}px from its baseline to the hairline")
        body_top = line + 2

# The column: the gutters either side of the text column carry no ink.
bands = {"left": range(x0 + 3, x0 + inset - 1), "right": range(x1 - inset + 2, x1 - 2)}
for side, xs in bands.items():
    hit = None
    for y in range(body_top, y1 - RADIUS):
        for x in xs:
            if d(px[x, y], fills[side]) > INK:
                edge = x - x0 if side == "left" else x1 - x
                if hit is None or edge < hit[0]:
                    hit = (edge, y - y0)
                break
    if hit:
        say(f"column {side}", False, f"ink {hit[0]}px from the {side} edge at y+{hit[1]}, inside the {inset}px column")
    else:
        say(f"column {side}", True, f"nothing inside {inset}px")

# The bottom: nothing in the last inset rows, and a closing box `inset` up.
xs = range(x0 + RADIUS, x1 - RADIUS)
low = None
for y in range(y1 - 2, body_top, -1):
    n = sum(1 for x in xs if d(px[x, y], fill) > INK)
    if n:
        low = (y, n)
        break
if low is None:
    say("bottom", True, "an empty body")
else:
    gap = y1 - low[0]
    if gap < inset - 1:
        say("bottom", False, f"ink {gap}px above the bottom edge, inside {inset}px")
    elif low[1] >= len(xs) // 2:
        say("bottom", gap <= inset + 3, f"the last box ends {gap}px above the edge (want {inset})")
    else:
        say("bottom", gap <= inset + band // 2, f"the last ink {gap}px above the edge (inside {inset + band // 2})")


# One inset level: border lines down the column's left edge, by x.
def line_at(x, y):
    # A border is lighter (dark mode) or darker (light) than both sides,
    # whatever fill each side carries; a chip's edge is a step, not a line.
    p, a, b = sum(px[x, y]), sum(px[x - 2, y]), sum(px[x + 2, y])
    return (p - a > LINE and p - b > LINE) or (a - p > LINE and b - p > LINE)


runs = []
for x in range(x0 + inset - 2, min(x0 + inset + 32, x1 - 2)):
    best, cur, start = (0, 0), 0, 0
    for y in range(body_top, y1 - 2):
        if line_at(x, y):
            if cur == 0:
                start = y
            cur += 1
            if cur > best[0]:
                best = (cur, start)
        else:
            cur = 0
    if best[0] >= RUN:
        runs.append((x, best[1], best[1] + best[0]))
borders = []
for x, a, b in runs:
    if borders and x - borders[-1][0] <= 3:
        borders[-1] = (x, min(a, borders[-1][1]), max(b, borders[-1][2]))
    else:
        borders.append((x, a, b))
nested = [(p, q) for i, p in enumerate(borders) for q in borders[i + 1:] if min(p[2], q[2]) - max(p[1], q[1]) >= RUN]
if nested:
    p, q = nested[0]
    say("one inset level", False, f"box edges {p[0] - x0}px and {q[0] - x0}px in, side by side from y+{max(p[1], q[1]) - y0}")
else:
    say("one inset level", True, f"{len(borders)} box edge(s) down the column")


# Even insets: the ink's own extent inside the card, the border and the
# corners left out.
def ink_in(x, y, side):
    return d(px[x, y], fills[side]) > INK


ys = range(y0 + RADIUS, y1 - RADIUS)
lead = next((x - x0 for x in range(x0 + 3, x1 - 2) if any(ink_in(x, y, "left") for y in ys)), None)
trail = next((x1 - x for x in range(x1 - 3, x0 + 2, -1) if any(ink_in(x, y, "right") for y in ys)), None)
if lead is None or trail is None:
    say("even insets", True, "an empty card")
else:
    say("even insets", abs(lead - trail) <= 1, f"{lead}px before the first ink, {trail}px after the last")
    xs = range(x0 + RADIUS, x1 - RADIUS)
    # A header's top is its title's side: a control on the right of the
    # row stands taller than the words and is centred on the same line.
    head_xs = range(x0 + RADIUS, (x0 + x1) // 2) if header else xs
    top = next((y - y0 for y in range(y0 + 3, y1 - 2) if any(d(px[x, y], fill) > INK for x in head_xs)), None)
    bottom = next((y1 - y for y in range(y1 - 3, y0 + 2, -1) if any(d(px[x, y], fill) > INK for x in xs)), None)
    closing = low is not None and low[1] >= len(xs) // 2
    if top is not None and (not header or closing):
        say("even insets top/bottom", abs(top - bottom) <= 1, f"{top}px above the first ink, {bottom}px under the last")

sys.exit(1 if failed else 0)
