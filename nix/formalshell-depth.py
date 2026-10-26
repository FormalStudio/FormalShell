"""Writes a depth map beside each wallpaper.

    formalshell-depth DIR
    formalshell-depth --file PICTURE

DIR is walked for png, jpg, jpeg and webp pictures, dot directories and dot
files skipped. Each picture gets DIR/.depth/<file name>.png: 8-bit
grayscale, 255 nearest, the picture's aspect with the long edge capped at
MAX_EDGE, edges pulled onto the picture's own by a guided filter. A map is
current when it exists and is not older than its picture; its mtime is set to
the picture's, so a copy that keeps mtimes stays current. Maps whose picture
is gone are removed (DIR mode only).

Exit 0 when every picture is done or current, 1 when any failed, 2 on usage.
FORMALSHELL_DEPTH_MODEL points at the ONNX model (Depth Anything V2 Small,
preprocessed as its preprocessor_config.json says: long edge 518, sides
multiples of 14, bicubic, ImageNet mean and std).
"""

import os
import sys

import numpy as np
import onnxruntime as ort
from PIL import Image, ImageOps

EXTENSIONS = (".png", ".jpg", ".jpeg", ".webp")
MODEL_EDGE = 518
MULTIPLE = 14
MAX_EDGE = 2560
MEAN = np.array([0.485, 0.456, 0.406], dtype=np.float32)
STD = np.array([0.229, 0.224, 0.225], dtype=np.float32)
GUIDE_RADIUS = 0.01
GUIDE_EPS = 1e-3

Image.MAX_IMAGE_PIXELS = None


def scaled(size, edge):
    w, h = size
    k = edge / max(w, h)
    return max(1, round(w * k)), max(1, round(h * k))


def model_size(size):
    w, h = scaled(size, MODEL_EDGE)
    return max(MULTIPLE, round(w / MULTIPLE) * MULTIPLE), max(MULTIPLE, round(h / MULTIPLE) * MULTIPLE)


def box(a, r):
    """Mean over a (2r+1) square window, clamped at the borders."""
    h, w = a.shape
    c = np.cumsum(np.pad(a.astype(np.float64), ((1, 0), (1, 0))).cumsum(0), 1)
    y0 = np.clip(np.arange(h) - r, 0, h)
    y1 = np.clip(np.arange(h) + r + 1, 0, h)
    x0 = np.clip(np.arange(w) - r, 0, w)
    x1 = np.clip(np.arange(w) + r + 1, 0, w)
    s = c[y1][:, x1] - c[y0][:, x1] - c[y1][:, x0] + c[y0][:, x0]
    return s / ((y1 - y0)[:, None] * (x1 - x0)[None, :])


def guided(guide, src, r, eps):
    mi, mp = box(guide, r), box(src, r)
    var = box(guide * guide, r) - mi * mi
    cov = box(guide * src, r) - mi * mp
    a = cov / (var + eps)
    b = mp - a * mi
    return box(a, r) * guide + box(b, r)


class Depth:
    def __init__(self, model):
        self.session = ort.InferenceSession(model, providers=["CPUExecutionProvider"])
        self.input = self.session.get_inputs()[0].name

    def map(self, path):
        with Image.open(path) as opened:
            img = ImageOps.exif_transpose(opened).convert("RGB")
        mw, mh = model_size(img.size)
        x = np.asarray(img.resize((mw, mh), Image.BICUBIC), dtype=np.float32) / 255.0
        x = ((x - MEAN) / STD).transpose(2, 0, 1)[None]
        d = self.session.run(None, {self.input: x})[0][0].astype(np.float32)
        lo, hi = float(d.min()), float(d.max())
        d = (d - lo) / (hi - lo) if hi > lo else np.zeros_like(d)
        ow, oh = scaled(img.size, min(max(img.size), MAX_EDGE))
        d = np.asarray(Image.fromarray(d, "F").resize((ow, oh), Image.BICUBIC), dtype=np.float32)
        luma = np.asarray(img.convert("L").resize((ow, oh), Image.BICUBIC), dtype=np.float32) / 255.0
        r = max(2, round(max(ow, oh) * GUIDE_RADIUS))
        d = guided(luma.astype(np.float64), d.astype(np.float64), r, GUIDE_EPS)
        return Image.fromarray(np.clip(d * 255.0 + 0.5, 0, 255).astype(np.uint8), "L")


def map_path(picture):
    folder, name = os.path.split(picture)
    return os.path.join(folder, ".depth", name + ".png")


def current(picture, out):
    try:
        return os.stat(out).st_mtime_ns >= os.stat(picture).st_mtime_ns
    except FileNotFoundError:
        return False


def process(depth, picture):
    out = map_path(picture)
    if current(picture, out):
        print(f"fresh {picture}")
        return True
    tmp = out + ".tmp"
    try:
        os.makedirs(os.path.dirname(out), exist_ok=True)
        stamp = os.stat(picture).st_mtime_ns
        depth.map(picture).save(tmp, "PNG")
        os.utime(tmp, ns=(stamp, stamp))
        os.replace(tmp, out)
    except Exception as e:
        try:
            os.unlink(tmp)
        except OSError:
            pass
        print(f"failed {picture}: {e}")
        return False
    print(f"wrote {picture}")
    return True


def prune(folder):
    store = os.path.join(folder, ".depth")
    if not os.path.isdir(store):
        return
    for name in sorted(os.listdir(store)):
        if name.endswith(".png") and not os.path.exists(os.path.join(folder, name[:-4])):
            path = os.path.join(store, name)
            os.unlink(path)
            print(f"pruned {path}")


def pictures(root):
    for folder, dirs, files in os.walk(root):
        dirs[:] = sorted(d for d in dirs if not d.startswith("."))
        prune(folder)
        for name in sorted(files):
            if not name.startswith(".") and name.lower().endswith(EXTENSIONS):
                yield os.path.join(folder, name)


def main(argv):
    if len(argv) == 1 and not argv[0].startswith("-"):
        targets = pictures(argv[0]) if os.path.isdir(argv[0]) else None
    elif len(argv) == 2 and argv[0] == "--file":
        targets = [argv[1]] if os.path.isfile(argv[1]) else None
    else:
        targets = None
    if targets is None:
        print("usage: formalshell-depth DIR | formalshell-depth --file PICTURE", file=sys.stderr)
        return 2
    depth = None
    ok = True
    for picture in targets:
        if depth is None and not current(picture, map_path(picture)):
            depth = Depth(os.environ["FORMALSHELL_DEPTH_MODEL"])
        ok = process(depth, picture) and ok
    return 0 if ok else 1


sys.exit(main(sys.argv[1:]))
