#!/usr/bin/env python3
"""A dark card's rim read off a frame at its own pixels.

usage: card-corner.py <png> <x> <y> [name]

(x, y) is a pixel inside the card's fill, left of its right edge and below
its top edge, over a light flat wallpaper. From there the top edge is found
straight up and the right edge straight across, and each is read along its
normal: the border has to be one pixel at full depth with the wallpaper
side of it clearly lighter (a line on a part pixel spreads over two at half
depth, which reads as a soft doubled rim). Then the top-right arc: nothing lit on it brighter than the top
edge's own line, which is the brightest one the card draws.

Prints one line of numbers and exits non-zero naming the claim that missed.
"""
import sys

from PIL import Image

png, x, y = sys.argv[1], int(sys.argv[2]), int(sys.argv[3])
name = sys.argv[4] if len(sys.argv) > 4 else "card"
img = Image.open(png).convert("L")
w, h = img.size
px = img.load()

# The darkest pixel a border of black at 0.75 over the dark fill gives,
# with room for a light theme's lighter fill.
DEEP = 40
# A part pixel of the border over the wallpaper sits between the two; the
# pixel past a crisp one is the wallpaper under the card's cast.
CLEAR = 40

fill = px[x, y]
fails = []


def profile(points):
    return [px[p] for p in points if 0 <= p[0] < w and 0 <= p[1] < h]


def edge(points, label):
    """The rim along `points`, walking out of the fill to the wallpaper."""
    values = profile(points)
    out = next((i for i, v in enumerate(values) if v > fill + 60), None)
    if out is None or out < 2:
        fails.append(f"{label}: no wallpaper past the card ({values[:12]})")
        return None
    border, lip, outside = values[out - 1], values[out - 2], values[out]
    if border > DEEP:
        fails.append(f"{label}: the border pixel is {border}, not a whole one (spread over two)")
    if outside - border < CLEAR:
        fails.append(f"{label}: the pixel past the border is {outside}, a half-drawn border beside {border}")
    return {"border": border, "lip": lip, "outside": outside, "at": out}


top = edge([(x, y - i) for i in range(h)], f"{name} top")
right = edge([(x + i, y) for i in range(w)], f"{name} right")
arc = None
if top and right:
    # The corner's own circle, its radius where the top border row and the
    # right border column each stop being straight. Only the band just
    # inside the border is read, where the lit lines run: the arc's
    # antialiased pixels over the wallpaper lie outside it and the card's
    # content well inside.
    x1 = x + right["at"] - 1
    y0 = y - top["at"] + 1
    across = x1 - min((i for i in range(x, x1 + 1) if px[i, y0] > DEEP), default=x1)
    down = min((j for j in range(y0, y + 1) if px[x1, j] <= DEEP), default=y0) - y0
    radius = (across + down) / 2
    cx, cy = x1 + 1 - radius, y0 + radius
    band = [
        px[i, j]
        for i in range(int(cx), x1 + 1)
        for j in range(y0, int(cy) + 1)
        if radius - 4.5 < ((i + 0.5 - cx) ** 2 + (j + 0.5 - cy) ** 2) ** 0.5 < radius - 1.2
    ]
    arc = max(band) if band else None
    if radius < 3 or arc is None:
        fails.append(f"{name} arc: no corner found off the border (radius {radius})")
    elif arc > top["lip"] + 6:
        fails.append(f"{name} arc: lit to {arc}, past the top edge's line {top['lip']}")

print(f"{name}: fill={fill} top={top} right={right} arc={arc}")
if fails:
    print("FAIL " + "; ".join(fails))
    sys.exit(1)
