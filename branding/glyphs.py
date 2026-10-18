"""Source of truth for the Popizdim tray glyphs. Writes SVGs; render.py rasterizes them.

Glyph grid is 100 wide by 60 tall (viewBox 0 20 100 60): the mouth is wide, so
the canvas is too, and macOS scales a tray image to 18 pt high at its own
aspect. Tray icons are flat silhouettes (macOS uses them as template images). The app icon is raster artwork, see cut_app_icon.py.
"""
import math
import pathlib

OUT = pathlib.Path(__file__).parent / "svg"
OUT.mkdir(exist_ok=True)

BRAND = "#E8645A"  # colored-theme tray glyph (the coral of the app icon's lips)

# Open mouth, front view: wide lips with pointed corners and a cupid's bow, a
# dark opening (even-odd hole) and a tongue, followed by three sound waves.
LIPS = (
    "M4 50 C 12 34, 24 31.5, 32 36.5 C 40 31.5, 52 34, 60 50 C 48 76, 16 76, 4 50 Z "
    "M13.5 50 C 20 43, 27 41.5, 32 43.5 C 37 41.5, 44 43, 50.5 50 "
    "C 45 59, 38 61.5, 32 61.5 C 26 61.5, 19 59, 13.5 50 Z"
)
TONGUE = "M20 55.5 C 24 51, 40 51, 44 55.5 C 40 59.5, 24 59.5, 20 55.5 Z"


def waves(c, cx=62, cy=50, radii=(12, 22, 32), angle=55, width=7):
    """Arcs of growing radius opening to the right of the mouth."""
    out = ""
    for r in radii:
        x = cx + r * math.cos(math.radians(angle))
        dy = r * math.sin(math.radians(angle))
        out += (
            f'<path d="M{x:.1f} {cy - dy:.1f} A{r} {r} 0 0 1 {x:.1f} {cy + dy:.1f}" '
            f'fill="none" stroke="{c}" stroke-width="{width}" stroke-linecap="round"/>'
        )
    return out


def mouth(c):
    return (
        f'<path d="{LIPS}" fill="{c}" fill-rule="evenodd"/>'
        f'<path d="{TONGUE}" fill="{c}"/>' + waves(c)
    )


# Transcribed text: three lines.
LINES = """
  <line x1="24" y1="38" x2="76" y2="38" stroke="{c}" stroke-width="7" stroke-linecap="round"/>
  <line x1="24" y1="50" x2="76" y2="50" stroke="{c}" stroke-width="7" stroke-linecap="round"/>
  <line x1="24" y1="62" x2="58" y2="62" stroke="{c}" stroke-width="7" stroke-linecap="round"/>
"""


def svg(body):
    return f'<svg xmlns="http://www.w3.org/2000/svg" width="100" height="60" viewBox="0 20 100 60">{body}</svg>'


def idle(c):
    return svg(mouth(c))


def badge_disc(inner, c, mask_id):
    """A filled pill with `inner` (drawn in black) knocked out."""
    return f"""
  <mask id="{mask_id}"><rect y="20" width="100" height="60" fill="white"/>{inner}</mask>
  <rect y="20" width="100" height="60" rx="30" fill="{c}" mask="url(#{mask_id})"/>"""


def recording(c):
    # Smaller mouth knocked out of a disc.
    glyph = f'<g transform="translate(50 50) scale(0.72) translate(-50 -50)">{mouth("black")}</g>'
    return svg(badge_disc(glyph, c, "m"))


def transcribing(c):
    return svg(badge_disc(LINES.format(c="black"), c, "m"))


def warning(c):
    glyph = f'<g transform="translate(50 50) scale(0.85) translate(-50 -50)">{mouth(c)}</g>'
    body = f"""
  <mask id="cut"><rect y="20" width="100" height="60" fill="white"/><circle cx="85" cy="66" r="19" fill="black"/></mask>
  <g mask="url(#cut)">{glyph}</g>
  <mask id="bang"><rect y="20" width="100" height="60" fill="white"/>
    <line x1="85" y1="59" x2="85" y2="68" stroke="black" stroke-width="4.6" stroke-linecap="round"/>
    <circle cx="85" cy="74" r="2.7" fill="black"/></mask>
  <circle cx="85" cy="66" r="14.5" fill="{c}" mask="url(#bang)"/>"""
    return svg(body)


variants = {
    "tray_idle": idle("white"),
    "tray_idle_dark": idle("black"),
    "tray_recording": recording("white"),
    "tray_recording_dark": recording("black"),
    "tray_transcribing": transcribing("white"),
    "tray_transcribing_dark": transcribing("black"),
    "tray_idle_warning": warning("white"),
    "tray_idle_warning_dark": warning("black"),
    "tray_colored_idle": idle(BRAND),
    "tray_colored_warning": warning(BRAND),
    "tray_colored_recording": recording(BRAND),
    "tray_colored_transcribing": transcribing(BRAND),
}
for name, body in variants.items():
    (OUT / f"{name}.svg").write_text(body)
print(f"wrote {len(variants)} SVGs to {OUT}")
