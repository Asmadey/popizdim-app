"""Split the app icon artwork into Icon Composer layers for the macOS 26 icon.

macOS 26 draws icons without an Assets.car on a gray plate. A native icon is an
Icon Composer `.icon` bundle: a full-bleed background fill plus transparent
foreground layers that the system masks, lights and shadows itself. The source
render (see cut_app_icon.py) has the art baked onto a light plate with a metal
frame, so this script cuts the mouth and the sound wave off the plate:

* the plate colour is estimated per pixel from the pixels that are clearly plate;
* alpha is the chroma distance from that colour (brightness is ignored so the
  soft shadow under the lips stays plate), with enclosed holes (the teeth, which
  are as white as the plate) filled in;
* the cut-out is enlarged a little and centred on the 1024 px canvas (the plate
  becomes the full-bleed background fill).

    python3 branding/make_icon_layers.py branding/source/app-icon-source.webp

Writes src-tauri/icons/icon.icon (icon.json + Assets/foreground.png) and a
preview of the masked result to branding/png/app-icon-layers-preview.png.
bunx tauri build compiles the bundle to Assets.car when Xcode 26+ (actool) is
installed and keeps the icon.icns fallback otherwise.
"""

import json
import pathlib
import sys

import numpy as np
from PIL import Image
from scipy import ndimage

ROOT = pathlib.Path(__file__).resolve().parent.parent
ICON_DIR = ROOT / "src-tauri" / "icons" / "icon.icon"
PREVIEW = ROOT / "branding" / "png" / "app-icon-layers-preview.png"

# Outer edge of the metal frame in the source render, and the inner plate area.
X0, X1, Y0, Y1 = 127, 895, 126, 912
PLATE_INSET = 40  # skip the frame and its bevel
# Colour distance (RGB) below which a pixel counts as plate / as fully art.
DIST_LO, DIST_HI = 22.0, 30.0  # chroma distance; DIST_HI seeds the plate estimate
CANVAS = 1024
# Art enlargement relative to the source: the mouth and wave span about two
# thirds of the canvas, inside the safe area the system mask leaves.
SCALE = 1.15


def chroma_dist(rgb: np.ndarray, est: np.ndarray) -> np.ndarray:
    """Colour distance that ignores brightness, so the soft shadow stays plate."""
    c = rgb - rgb.mean(-1, keepdims=True)
    e = est - est.mean(-1, keepdims=True)
    return np.linalg.norm(c - e, axis=-1)


def disc(r: int) -> np.ndarray:
    y, x = np.ogrid[-r : r + 1, -r : r + 1]
    return x * x + y * y <= r * r


def plate_colour(rgb: np.ndarray, art: np.ndarray) -> np.ndarray:
    """Smooth per-pixel estimate of the plate colour, ignoring art pixels."""
    weight = (~art).astype("float32")
    sigma = 60
    num = np.stack([ndimage.gaussian_filter(rgb[..., c] * weight, sigma) for c in range(3)], -1)
    den = ndimage.gaussian_filter(weight, sigma)[..., None]
    return num / np.maximum(den, 1e-3)


def main() -> None:
    src = Image.open(sys.argv[1]).convert("RGB")
    if src.size != (1024, 1024):
        sys.exit(f"expected a 1024x1024 source, got {src.size}")
    rgb = np.asarray(src).astype("float32")

    ys, xs = np.mgrid[0:1024, 0:1024]
    u = np.abs(xs + 0.5 - (X0 + X1) / 2) / ((X1 - X0) / 2 - PLATE_INSET)
    v = np.abs(ys + 0.5 - (Y0 + Y1) / 2) / ((Y1 - Y0) / 2 - PLATE_INSET)
    inner = (u**5 + v**5) <= 1

    # First pass: plate colour from the median of the inner area, then refine.
    base = np.median(rgb[inner].reshape(-1, 3), axis=0)
    art = (chroma_dist(rgb, base) > DIST_HI) & inner
    for _ in range(3):
        art = ndimage.binary_dilation(art, iterations=6)
        est = plate_colour(rgb, art | ~inner)
        art = (chroma_dist(rgb, est) > DIST_HI) & inner
    est = plate_colour(rgb, ndimage.binary_dilation(art, iterations=6) | ~inner)

    dist = chroma_dist(rgb, est)
    solid = ndimage.binary_closing((dist > DIST_LO) & inner, iterations=8)
    # Fill holes (white teeth) and drop specks, keeping only the main art.
    solid = ndimage.binary_fill_holes(solid)
    labels, n = ndimage.label(solid)
    sizes = ndimage.sum(solid, labels, range(1, n + 1))
    keep = np.isin(labels, [i + 1 for i, s in enumerate(sizes) if s > 400])
    # Outside the mouth body (thin wave bars survive no opening) plate-coloured
    # pixels are glow or gaps between the bars, not art.
    body = ndimage.binary_opening(keep, structure=disc(28))
    keep &= body | (dist > DIST_LO)
    keep = ndimage.binary_erosion(ndimage.binary_opening(keep, iterations=2), iterations=1)
    alpha = ndimage.gaussian_filter(keep.astype("float32"), 1.0)

    layer = np.dstack([rgb, alpha * 255]).astype("uint8")
    img = Image.fromarray(layer, "RGBA")

    ys, xs = np.where(alpha > 0.02)
    bx0, bx1, by0, by1 = xs.min(), xs.max() + 1, ys.min(), ys.max() + 1
    print(f"art bbox {bx0},{by0}-{bx1},{by1} ({bx1 - bx0}x{by1 - by0})")

    # The plate becomes the whole canvas; scale the art around the plate centre.
    scale = SCALE
    cx, cy = (X0 + X1) / 2, (Y0 + Y1) / 2
    big = img.resize((round(1024 * scale), round(1024 * scale)), Image.LANCZOS)
    ox = round(CANVAS / 2 - cx * scale)
    oy = round(CANVAS / 2 - cy * scale)
    out = Image.new("RGBA", (CANVAS, CANVAS), (0, 0, 0, 0))
    out.alpha_composite(big, (ox, oy)) if ox >= 0 and oy >= 0 else out.paste(
        big.crop((-ox, -oy, -ox + CANVAS, -oy + CANVAS)), (0, 0)
    )

    (ICON_DIR / "Assets").mkdir(parents=True, exist_ok=True)
    out.save(ICON_DIR / "Assets" / "foreground.png")

    far = inner & ~ndimage.binary_dilation(keep, iterations=40)
    fill = np.percentile(rgb[far].reshape(-1, 3), 60, axis=0) / 255
    fill_str = "srgb:{:.5f},{:.5f},{:.5f},1.00000".format(*fill)
    spec = {
        "fill": {"solid": fill_str},
        "groups": [
            {
                "layers": [
                    {"glass": False, "image-name": "foreground.png", "name": "foreground"}
                ],
                "shadow": {"kind": "neutral", "opacity": 0.5},
                "translucency": {"enabled": False, "value": 0.5},
            }
        ],
        "supported-platforms": {"circles": ["watchOS"], "squares": "shared"},
    }
    (ICON_DIR / "icon.json").write_text(json.dumps(spec, indent=2) + "\n")

    # Preview: fill + layer under a macOS-style squircle mask.
    bg = Image.new("RGBA", (CANVAS, CANVAS), tuple(int(c * 255) for c in fill) + (255,))
    bg.alpha_composite(out)
    s = 4
    ys2, xs2 = np.mgrid[0 : CANVAS * s, 0 : CANVAS * s]
    u = np.abs((xs2 + 0.5) / s - CANVAS / 2) / (CANVAS / 2)
    v = np.abs((ys2 + 0.5) / s - CANVAS / 2) / (CANVAS / 2)
    mask = Image.fromarray((((u**5 + v**5) <= 1) * 255).astype("uint8")).resize((CANVAS, CANVAS), Image.LANCZOS)
    bg.putalpha(mask)
    PREVIEW.parent.mkdir(exist_ok=True)
    bg.save(PREVIEW)
    print(f"{ICON_DIR} and {PREVIEW}")


if __name__ == "__main__":
    main()
