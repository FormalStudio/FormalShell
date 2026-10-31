#!/usr/bin/env python3
"""Reads one frame of the pixel dissolve (surfaces/dissolve.rs) on its 32 px
grid: how many whole 32 px cells are one colour, and of the cells where the
two end pictures differ at the cell's centre, how many show the first
picture's centre pixel and how many the second's.

    dissolve-cells.py <convert> <frame.png> <from.png> <to.png>

prints `cells=N uniform=U from=S to=D neither=X`. The middle third of a
dissolve holds every cell at 32 px, so a frame from it reads nearly all
uniform with both S and D above zero; neither picture alone has that.
"""
import subprocess
import sys

GRID = 32


def raw(convert, path):
    size = subprocess.run([convert, path, "-format", "%w %h", "info:"], capture_output=True, text=True, check=True).stdout.split()
    w, h = int(size[0]), int(size[1])
    data = subprocess.run([convert, path, "-alpha", "off", "-depth", "8", "rgb:-"], capture_output=True, check=True).stdout
    return w, h, data


def near(a, b, tol):
    return all(abs(x - y) <= tol for x, y in zip(a, b))


def main():
    convert, frame, src, dst = sys.argv[1:5]
    w, h, f = raw(convert, frame)
    pictures = [raw(convert, p) for p in (src, dst)]
    if any((pw, ph) != (w, h) for pw, ph, _ in pictures):
        print("cells=0 uniform=0 from=0 to=0 neither=0 size-mismatch")
        return
    a, b = pictures[0][2], pictures[1][2]
    stride = w * 3
    cells = uniform = n_from = n_to = neither = 0
    for cy in range(0, h - GRID + 1, GRID):
        for cx in range(0, w - GRID + 1, GRID):
            cells += 1
            first = f[cy * stride + cx * 3: cy * stride + cx * 3 + 3]
            line = first * GRID
            whole = all(f[y * stride + cx * 3: y * stride + (cx + GRID) * 3] == line for y in range(cy, cy + GRID))
            if not whole:
                continue
            uniform += 1
            c = (cy + GRID // 2) * stride + (cx + GRID // 2) * 3
            pa, pb = a[c:c + 3], b[c:c + 3]
            if near(pa, pb, 24):
                continue
            if near(first, pa, 6):
                n_from += 1
            elif near(first, pb, 6):
                n_to += 1
            else:
                neither += 1
    print(f"cells={cells} uniform={uniform} from={n_from} to={n_to} neither={neither}")


if __name__ == "__main__":
    main()
