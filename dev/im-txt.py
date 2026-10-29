#!/usr/bin/env python3
"""ImageMagick's `txt:-` pixel enumeration, for a host whose policy blocks
the TXT coder (openSUSE's ImageMagick-config-7-SUSE refuses it, and
sparse-color with it). dev/smoke.sh points convert_bin here; any other call
goes straight to the real binary.

  im-txt.py <real convert> <convert args...> txt:-

The output matches IM7's own: a header, then one `x,y: (v,...)  #HEX
colour(v,...)` line per pixel, gray images as one value, alpha as a fourth.
"""
import os
import subprocess
import sys

real, args = sys.argv[1], sys.argv[2:]
if not args or args[-1] != "txt:-":
    os.execv(real, [real] + args)

src = args[:-1]
info = subprocess.run([real, *src, "-format", "%w %h %z %[colorspace] %A\n", "info:"],
                      capture_output=True, text=True)
if info.returncode != 0:
    sys.stderr.write(info.stderr)
    sys.exit(info.returncode)
frames = [line.split() for line in info.stdout.splitlines() if line.strip()]
if not frames:
    sys.exit(1)

out = sys.stdout
for n, (w, h, depth, space, alpha) in enumerate(frames):
    w, h, depth = int(w), int(h), 16 if int(depth) > 8 else 8
    gray = space.lower() in ("gray", "linear-gray", "lineargray")
    has_alpha = alpha.lower() in ("true", "blend")
    coder = ("gray" if gray else "rgb") + ("a" if has_alpha else "")
    raw = subprocess.run([real, *src, "-depth", str(depth), "%s:-" % coder], capture_output=True).stdout
    per = ((1 if gray else 3) + has_alpha) * (depth // 8)
    size = w * h * per
    frame = raw[n * size:(n + 1) * size]
    top = (1 << depth) - 1
    name = ("gray" if gray else "srgb") + ("a" if has_alpha else "")
    out.write("# ImageMagick pixel enumeration: %d,%d,0,%d,%s\n" % (w, h, top, name))
    digits = depth // 4
    for i in range(min(w * h, len(frame) // per)):
        px = frame[i * per:(i + 1) * per]
        vals = [int.from_bytes(px[j:j + depth // 8], "big") for j in range(0, per, depth // 8)]
        colour = vals[:1] * 3 if gray else vals[:3]
        hexed = "#" + "".join("%0*X" % (digits, v) for v in colour + (vals[-1:] if has_alpha else []))
        shown = [str(v) for v in vals]
        shown_c = shown[:-1] + ["%g" % round(vals[-1] / top, 4)] if has_alpha else shown
        out.write("%d,%d: (%s)  %s  %s(%s)\n" % (i % w, i // w, ",".join(shown), hexed, name, ",".join(shown_c)))
