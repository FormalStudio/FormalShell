"""Cuts the earbuds panel's product pictures down to what the panel draws.

    earbuds-art.py EAR_WEB LIBREPODS OUT

EAR_WEB and LIBREPODS are the two repos' source trees. OUT/earbuds holds
<set>_<part>.webp, part one of duo, case, left, right, each at most SIZE
pixels on its long side: a Nothing set is ear-web's own `b175_black`, an
AirPods set librepods' `airpods_pro_3` with its `buds` picture renamed
`duo`. Run at build time by dev/tarball.sh; nothing here ships.
"""

import glob
import os
import re
import sys

from PIL import Image

SIZE = 256
QUALITY = 88

ear_web, librepods, out = sys.argv[1], sys.argv[2], sys.argv[3]
dest = os.path.join(out, "earbuds")
os.makedirs(dest, exist_ok=True)

sources = [(p, re.fullmatch(r"(b\d{3}_[a-z]+_(?:duo|case|left|right))\.webp", os.path.basename(p))) for p in glob.glob(os.path.join(ear_web, "res/assets/*.webp"))]
sources += [(p, re.fullmatch(r"(airpods_[a-z0-9_]+_(?:buds|case|left|right))\.png", os.path.basename(p))) for p in glob.glob(os.path.join(librepods, "android/app/src/main/res-apple/drawable/*.png"))]

count = 0
for path, m in sources:
    if not m:
        continue
    name = re.sub(r"_buds$", "_duo", m.group(1))
    image = Image.open(path).convert("RGBA")
    image.thumbnail((SIZE, SIZE), Image.LANCZOS)
    image.save(os.path.join(dest, name + ".webp"), "WEBP", quality=QUALITY, method=6)
    count += 1
if count == 0:
    sys.exit("earbuds-art: no pictures found")
print(f"earbuds-art: {count} pictures")
