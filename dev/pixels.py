"""Pixel reads for the rig's image helpers (dissolve-cells.py,
settled-frames.py), on PIL, which dev/smoke.sh checks for before a leg that
needs it."""

from PIL import Image, ImageChops


def load(path):
    """The picture as RGB, alpha dropped."""
    return Image.open(path).convert("RGB")


def crop(img, box, scale):
    """The box ({"x","y","width","height"} in logical pixels) at `scale`."""
    x, y = round(box["x"] * scale), round(box["y"] * scale)
    w, h = round(box["width"] * scale), round(box["height"] * scale)
    return img.crop((x, y, x + w, y + h))


def differing(a, b, fuzz):
    """The share of pixels whose gray difference passes `fuzz` (0..255)."""
    gray = ImageChops.difference(a, b).convert("L")
    hist = gray.histogram()
    return sum(hist[int(fuzz) + 1:]) / max(1, a.width * a.height)
