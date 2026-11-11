#!/usr/bin/env python3
"""What a region of a screenshot looks like, for the rig's sessions.

  px.py mean SHOT X Y W H      the mean red, green and blue, 0 to 255
  px.py edge SHOT X Y W H      how sharp it is: the mean step between
                               neighbouring pixels along a row
  px.py diff A B X Y W H       the mean difference between two shots
"""
import sys
from PIL import Image, ImageChops, ImageStat


def region(path, box):
    x, y, w, h = (int(v) for v in box)
    return Image.open(path).convert("RGB").crop((x, y, x + w, y + h))


def main():
    what, args = sys.argv[1], sys.argv[2:]
    if what == "mean":
        print(" ".join(str(round(v)) for v in ImageStat.Stat(region(args[0], args[1:])).mean))
    elif what == "edge":
        image = region(args[0], args[1:]).convert("L")
        shifted = ImageChops.offset(image, 1, 0)
        step = ImageChops.difference(image, shifted).crop((1, 0, image.width, image.height))
        print(round(ImageStat.Stat(step).mean[0], 2))
    elif what == "diff":
        a, b = region(args[0], args[2:]), region(args[1], args[2:])
        print(round(sum(ImageStat.Stat(ImageChops.difference(a, b)).mean) / 3, 2))
    else:
        sys.exit(__doc__)


main()
