"""Cut the app icon out of artwork whose transparency is a baked-in checkerboard.

The source is an opaque 1024x1024 render: a blue rounded square (already on
the macOS icon grid, ~824 px body) over a fake transparency checkerboard plus
a drop shadow. The icon outline is traced row by row from its blue frame,
drawn as a 4x supersampled polygon for anti-aliased edges, and used as the
alpha mask; everything outside (checkerboard, shadow) becomes transparent.

    python3 branding/cut_app_icon.py branding/source/app-icon-source.webp
    bunx tauri icon branding/png/app-icon.png

Writes branding/png/app-icon.png.
"""

import pathlib
import sys

from PIL import Image, ImageDraw, ImageFilter

OUT = pathlib.Path(__file__).parent / "png" / "app-icon.png"
SUPERSAMPLE = 4
# The frame's outermost pixel row is blended with the checkerboard; pull the
# mask in by this much (source pixels) so no gray fringe survives.
INSET = 1.0


def is_icon(pixel: tuple[int, int, int]) -> bool:
    """Blue frame/body; the checkerboard and shadow are neutral gray."""
    r, g, b = pixel
    return b > r + 25 and b > g + 10


def main() -> None:
    src = Image.open(sys.argv[1]).convert("RGB")
    w, h = src.size
    px = src.load()

    left, right = [], []
    for y in range(h):
        xs = [x for x in range(w) if is_icon(px[x, y])]
        if xs:
            left.append((xs[0] + INSET, y + 0.5))
            right.append((xs[-1] + 1 - INSET, y + 0.5))
    if not left:
        sys.exit("no icon found")
    top = left[0][1] - 0.5 + INSET
    bottom = left[-1][1] + 0.5 - INSET
    outline = (
        [(left[0][0], top), (right[0][0], top)]
        + right
        + [(right[-1][0], bottom), (left[-1][0], bottom)]
        + left[::-1]
    )

    s = SUPERSAMPLE
    mask = Image.new("L", (w * s, h * s), 0)
    ImageDraw.Draw(mask).polygon([(x * s, y * s) for x, y in outline], fill=255)
    mask = mask.filter(ImageFilter.GaussianBlur(s / 2)).resize((w, h), Image.LANCZOS)

    icon = src.convert("RGBA")
    icon.putalpha(mask)
    OUT.parent.mkdir(exist_ok=True)
    icon.save(OUT)
    x0, x1 = left[0][0], right[0][0]
    print(f"{OUT}: body {min(x for x, _ in left):.0f}..{max(x for x, _ in right):.0f} x {top:.0f}..{bottom:.0f}")


if __name__ == "__main__":
    main()
