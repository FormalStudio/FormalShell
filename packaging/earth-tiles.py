"""Cuts Blue Marble into the tile pyramid Radio Atlas samples its globe from.

    earth-tiles.py SRC OUT

OUT/tiles/pyramid.txt names the tile size and each level, coarsest first;
level W holds OUT/tiles/W/<column>_<row>.jpg, TILE pixels square except
along the right and bottom edges. Each level halves the one above it, from
SRC's own width down to the first at or under BASE wide, which the shell
decodes whole on open. OUT/earth.jpg is the single picture the shell falls
back to without the tiles. Run at build time by dev/tarball.sh; nothing
here ships.
"""

import os
import sys

from PIL import Image

TILE = 512
BASE = 2700
QUALITY = 88
SINGLE = (4096, 2048)

Image.MAX_IMAGE_PIXELS = None

src, out = sys.argv[1], sys.argv[2]
tiles = os.path.join(out, "tiles")
os.makedirs(tiles, exist_ok=True)

image = Image.open(src).convert("RGB")
image.resize(SINGLE, Image.LANCZOS).save(os.path.join(out, "earth.jpg"), quality=90)

levels = [image]
while levels[-1].width > BASE:
    w, h = levels[-1].size
    levels.append(levels[-1].resize((w // 2, h // 2), Image.LANCZOS))
levels.reverse()

manifest = [f"tile {TILE}"]
for level in levels:
    w, h = level.size
    manifest.append(f"level {w} {h}")
    d = os.path.join(tiles, str(w))
    os.makedirs(d, exist_ok=True)
    for row in range((h + TILE - 1) // TILE):
        for col in range((w + TILE - 1) // TILE):
            box = (col * TILE, row * TILE, min(w, (col + 1) * TILE), min(h, (row + 1) * TILE))
            level.crop(box).save(os.path.join(d, f"{col}_{row}.jpg"), quality=QUALITY)

with open(os.path.join(tiles, "pyramid.txt"), "w") as f:
    f.write("\n".join(manifest) + "\n")
