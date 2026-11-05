"""Turns the pinned originals into the included wallpaper set.

    wallpapers.py CATALOG DOWNLOADS OUT

CATALOG is packaging/wallpapers.json; DOWNLOADS holds each row's original
as wallpaper-<id>.jpg, fetched and verified by dev/tarball.sh. OUT gets
<id>.jpg per row (converted to sRGB through its embedded ICC profile, none
read as sRGB, the long edge capped at LONG_EDGE, an sRGB profile and no
other metadata), catalog.json in the shape fs-menu's `catalog` reads, and
CREDITS, one line per picture. Run at build time by dev/tarball.sh.
"""

import io
import json
import os
import sys

from PIL import Image, ImageCms, ImageOps

LONG_EDGE = 5120
MIN_WIDTH = 3160
QUALITY = 92

Image.MAX_IMAGE_PIXELS = None

catalog, downloads, out = sys.argv[1], sys.argv[2], sys.argv[3]
os.makedirs(out, exist_ok=True)
srgb = ImageCms.createProfile("sRGB")
srgb_bytes = ImageCms.ImageCmsProfile(srgb).tobytes()

rows = json.load(open(catalog, encoding="utf-8"))
entries, credits = [], []
for row in rows:
    id_ = row.get("id") or "?"
    for key in ("title", "author", "license", "licenseUrl", "source", "origin"):
        if not row.get(key):
            sys.exit(f"wallpapers: {id_} has no {key}")
    with Image.open(os.path.join(downloads, f"wallpaper-{id_}.jpg")) as opened:
        icc = opened.info.get("icc_profile")
        image = ImageOps.exif_transpose(opened)
        if icc:
            src = ImageCms.ImageCmsProfile(io.BytesIO(icc))
            image = ImageCms.profileToProfile(image, src, srgb, outputMode="RGB")
        image = image.convert("RGB")
    if image.width < MIN_WIDTH:
        sys.exit(f"wallpapers: {id_} is {image.width} wide, under {MIN_WIDTH}")
    if max(image.size) > LONG_EDGE:
        k = LONG_EDGE / max(image.size)
        image = image.resize((round(image.width * k), round(image.height * k)), Image.LANCZOS)
    file = f"{id_}.jpg"
    image.save(os.path.join(out, file), quality=QUALITY, optimize=True, icc_profile=srgb_bytes)
    entries.append({key: row[key] for key in ("id", "title", "author", "license", "licenseUrl", "source", "origin")} | {"file": file})
    credits.append(f"{row['title']} by {row['author']}, {row['source']}, {row['license']} ({row['licenseUrl']})")
    print(f"wallpapers: {file} {image.width}x{image.height}{' from ' + ImageCms.getProfileDescription(src).strip() if icc else ''}")

with open(os.path.join(out, "catalog.json"), "w", encoding="utf-8") as f:
    json.dump(entries, f, indent=2, ensure_ascii=False)
    f.write("\n")
with open(os.path.join(out, "CREDITS"), "w", encoding="utf-8") as f:
    f.write("\n".join(credits) + "\n")
