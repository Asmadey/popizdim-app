"""Outline the "Popizdim" wordmark into a single SVG path.

The typeface is licensed, so the font file is never committed or bundled:
only the outlines (logo artwork) are. Shaping (kerning) comes from HarfBuzz,
outlines from fontTools.

    brew install harfbuzz && pip install fonttools
    python3 branding/wordmark.py /path/to/Font-BoldItalic.ttf

Writes branding/svg/wordmark.svg and prints the viewBox and path data for
src/components/icons/AppLogo.tsx.
"""

import json
import pathlib
import subprocess
import sys

from fontTools.pens.svgPathPen import SVGPathPen
from fontTools.pens.transformPen import TransformPen
from fontTools.ttLib import TTFont

TEXT = "Popizdim"
COLOR = "#DD1E3A"
OUT = pathlib.Path(__file__).parent / "svg" / "wordmark.svg"


def shape(font_path: str) -> list[dict]:
    out = subprocess.run(
        ["hb-shape", "--output-format=json", "--no-glyph-names", font_path, TEXT],
        check=True,
        capture_output=True,
        text=True,
    ).stdout
    return json.loads(out)


def main() -> None:
    font_path = sys.argv[1]
    font = TTFont(font_path)
    glyph_set = font.getGlyphSet()
    order = font.getGlyphOrder()

    pen = SVGPathPen(glyph_set, ntos=lambda v: f"{v:.1f}".rstrip("0").rstrip("."))
    x = 0
    for g in shape(font_path):
        name = order[g["g"]]
        # Font units are y-up; SVG is y-down.
        glyph_set[name].draw(
            TransformPen(pen, (1, 0, 0, -1, x + g["dx"], -g["dy"]))
        )
        x += g["ax"]
    path = pen.getCommands()

    # Tight box from the outlines themselves (italic overhangs the advances).
    from fontTools.pens.boundsPen import BoundsPen

    bounds = BoundsPen(glyph_set)
    x = 0
    for g in shape(font_path):
        glyph_set[order[g["g"]]].draw(
            TransformPen(bounds, (1, 0, 0, -1, x + g["dx"], -g["dy"]))
        )
        x += g["ax"]
    x0, y0, x1, y1 = bounds.bounds
    pad = 0.02 * (y1 - y0)
    view_box = f"{x0 - pad:.0f} {y0 - pad:.0f} {x1 - x0 + 2 * pad:.0f} {y1 - y0 + 2 * pad:.0f}"

    OUT.write_text(
        f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="{view_box}">'
        f'<path fill="{COLOR}" d="{path}"/></svg>\n'
    )
    print(view_box)
    print(path)


if __name__ == "__main__":
    main()
