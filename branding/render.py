"""Rasterize the tray glyphs in branding/svg/ with headless Chrome (transparent background).

Tray glyphs are drawn at 256px and downsampled to 64px for clean edges.
Usage: python3 render.py  -> branding/png/*.png
"""
import pathlib, subprocess, tempfile
from PIL import Image

HERE = pathlib.Path(__file__).parent
CHROME = "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"
OUT = HERE / "png"
OUT.mkdir(exist_ok=True)


def rasterize(svg_path: pathlib.Path, px: int) -> Image.Image:
    with tempfile.TemporaryDirectory() as tmp:
        html = pathlib.Path(tmp) / "page.html"
        html.write_text(
            f'<html><body style="margin:0;background:transparent">'
            f'<img src="file://{svg_path}" width="{px}" height="{px}" style="display:block"></body></html>'
        )
        shot = pathlib.Path(tmp) / "shot.png"
        subprocess.run(
            [CHROME, "--headless", "--disable-gpu", "--hide-scrollbars",
             "--default-background-color=00000000", f"--window-size={px},{px}",
             "--allow-file-access-from-files", f"--screenshot={shot}", f"file://{html}"],
            check=True, capture_output=True, timeout=60,
        )
        return Image.open(shot).convert("RGBA").crop((0, 0, px, px))


# Only the tray glyphs are vector. The app icon is raster artwork, cut by
# cut_app_icon.py; the wordmark SVG is used as is.
for svg in sorted((HERE / "svg").glob("tray_*.svg")):
    rasterize(svg, 256).resize((64, 64), Image.LANCZOS).save(OUT / f"{svg.stem}.png")
    print("rendered", svg.stem)
