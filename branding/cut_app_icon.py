"""Cut the app icon out of artwork whose transparency is a baked-in checkerboard.

The source is an opaque 1024x1024 render: a light rounded square with a metal
frame over a fake transparency checkerboard plus a drop shadow. The icon is a
squircle (superellipse, exponent 5) fitted to the frame's outer edge, drawn
4x supersampled for anti-aliased edges and used as the alpha mask; everything
outside (checkerboard, shadow) becomes transparent. The body is then scaled to
the macOS icon grid (824 px in a 1024 px canvas) and given a soft drop shadow.

    python3 branding/cut_app_icon.py branding/source/app-icon-source.webp
    bunx tauri icon branding/png/app-icon.png

Writes branding/png/app-icon.png (1024 px, with margin and shadow) and
src/assets/app-icon.png (256 px body only, used by the in-app mark).
"""

import pathlib
import sys

import numpy as np
from PIL import Image, ImageFilter

ROOT = pathlib.Path(__file__).resolve().parent.parent
OUT = ROOT / "branding" / "png" / "app-icon.png"
OUT_APP = ROOT / "src" / "assets" / "app-icon.png"

# Outer edge of the frame in the source render (measured), left/right/top/bottom.
X0, X1, Y0, Y1 = 127, 895, 126, 912
EXPONENT = 5
SUPERSAMPLE = 4
# Pull the mask in by this much (source pixels) so no gray fringe survives.
INSET = 0.5
BODY = 824  # macOS icon grid body size inside the 1024 canvas
SHADOW_BLUR, SHADOW_DY, SHADOW_ALPHA = 16, 10, 0.30


def squircle_mask(size: int) -> Image.Image:
    cx, cy = (X0 + X1) / 2, (Y0 + Y1) / 2
    a, b = (X1 - X0) / 2 - INSET, (Y1 - Y0) / 2 - INSET
    s = SUPERSAMPLE
    ys, xs = np.mgrid[0 : size * s, 0 : size * s]
    u = np.abs((xs + 0.5) / s - cx) / a
    v = np.abs((ys + 0.5) / s - cy) / b
    inside = (u**EXPONENT + v**EXPONENT) <= 1
    mask = Image.fromarray((inside * 255).astype("uint8"))
    return mask.resize((size, size), Image.LANCZOS)


def main() -> None:
    src = Image.open(sys.argv[1]).convert("RGB")
    if src.size != (1024, 1024):
        sys.exit(f"expected a 1024x1024 source, got {src.size}")

    icon = src.convert("RGBA")
    icon.putalpha(squircle_mask(1024))
    body = icon.crop((X0 - 1, Y0 - 1, X1 + 2, Y1 + 2)).resize((BODY, BODY), Image.LANCZOS)

    margin = (1024 - BODY) // 2
    shadow = Image.new("RGBA", (1024, 1024), (0, 0, 0, 0))
    shadow.paste((0, 0, 0, 255), (margin, margin + SHADOW_DY), body.getchannel("A"))
    shadow = shadow.filter(ImageFilter.GaussianBlur(SHADOW_BLUR))
    shadow.putalpha(shadow.getchannel("A").point(lambda v: int(v * SHADOW_ALPHA)))

    canvas = Image.new("RGBA", (1024, 1024), (0, 0, 0, 0))
    canvas.alpha_composite(shadow)
    canvas.alpha_composite(body, (margin, margin))
    OUT.parent.mkdir(exist_ok=True)
    canvas.save(OUT)

    body.resize((256, 256), Image.LANCZOS).save(OUT_APP)
    print(f"{OUT} (1024, body {BODY}) and {OUT_APP} (256)")


if __name__ == "__main__":
    main()
