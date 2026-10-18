"""Source of truth for Popizdim icons. Writes SVGs; render.sh rasterizes them.

Glyph grid is 100x100. Tray icons are flat silhouettes (macOS uses them as
template images); the app icon is a gradient squircle with a white glyph.
"""
import pathlib

OUT = pathlib.Path(__file__).parent / "svg"
OUT.mkdir(exist_ok=True)

BRAND = "#6D6AF5"          # colored-theme tray glyph
GRAD = ("#4F46E5", "#8B5CF6")  # app icon background (indigo → violet)

# Microphone: capsule, cradle arc, stem, base.
MIC = """
  <rect x="37" y="10" width="26" height="46" rx="13" fill="{c}"/>
  <path d="M24 45 a26 26 0 0 0 52 0" fill="none" stroke="{c}" stroke-width="7.5" stroke-linecap="round"/>
  <line x1="50" y1="72" x2="50" y2="84" stroke="{c}" stroke-width="7.5" stroke-linecap="round"/>
  <line x1="37" y1="88" x2="63" y2="88" stroke="{c}" stroke-width="7.5" stroke-linecap="round"/>
"""

# Transcribed text: three lines.
LINES = """
  <line x1="29" y1="35" x2="71" y2="35" stroke="{c}" stroke-width="8" stroke-linecap="round"/>
  <line x1="29" y1="50" x2="71" y2="50" stroke="{c}" stroke-width="8" stroke-linecap="round"/>
  <line x1="29" y1="65" x2="57" y2="65" stroke="{c}" stroke-width="8" stroke-linecap="round"/>
"""


def svg(body, size=100):
    return f'<svg xmlns="http://www.w3.org/2000/svg" width="{size}" height="{size}" viewBox="0 0 100 100">{body}</svg>'


def idle(c):
    return svg(MIC.format(c=c))


def badge_disc(inner, c, mask_id):
    """A filled disc with `inner` (drawn in black) knocked out."""
    return f"""
  <mask id="{mask_id}"><rect width="100" height="100" fill="white"/>{inner}</mask>
  <circle cx="50" cy="50" r="48" fill="{c}" mask="url(#{mask_id})"/>"""


def recording(c):
    # Smaller mic knocked out of a disc.
    mic = f'<g transform="translate(50 50) scale(0.74) translate(-50 -49)">{MIC.format(c="black")}</g>'
    return svg(badge_disc(mic, c, "m"))


def transcribing(c):
    return svg(badge_disc(LINES.format(c="black"), c, "m"))


def warning(c):
    mic = f'<g transform="translate(-6 -4) scale(0.9)">{MIC.format(c=c)}</g>'
    body = f"""
  <mask id="cut"><rect width="100" height="100" fill="white"/><circle cx="74" cy="74" r="27" fill="black"/></mask>
  <g mask="url(#cut)">{mic}</g>
  <mask id="bang"><rect width="100" height="100" fill="white"/>
    <line x1="74" y1="62" x2="74" y2="77" stroke="black" stroke-width="7" stroke-linecap="round"/>
    <circle cx="74" cy="86" r="4" fill="black"/></mask>
  <circle cx="74" cy="74" r="22" fill="{c}" mask="url(#bang)"/>"""
    return svg(body)


def app_icon():
    # macOS grid: 824px body inside a 1024px canvas, continuous-corner squircle.
    glyph = f'<g transform="translate(512 500) scale(5.1) translate(-50 -50)">{MIC.format(c="white")}</g>'
    bars = ""
    for x, h in [(214, 120), (268, 220), (756, 220), (810, 120)]:
        bars += f'<rect x="{x - 15}" y="{500 - h / 2}" width="30" height="{h}" rx="15" fill="white" opacity="0.55"/>'
    return f"""<svg xmlns="http://www.w3.org/2000/svg" width="1024" height="1024" viewBox="0 0 1024 1024">
  <defs>
    <linearGradient id="bg" x1="0" y1="0" x2="1" y2="1">
      <stop offset="0" stop-color="{GRAD[0]}"/><stop offset="1" stop-color="{GRAD[1]}"/>
    </linearGradient>
    <linearGradient id="shine" x1="0" y1="0" x2="0" y2="1">
      <stop offset="0" stop-color="white" stop-opacity="0.18"/><stop offset="0.5" stop-color="white" stop-opacity="0"/>
    </linearGradient>
    <filter id="shadow" x="-20%" y="-20%" width="140%" height="140%">
      <feDropShadow dx="0" dy="14" stdDeviation="18" flood-color="#1e1b4b" flood-opacity="0.35"/>
    </filter>
  </defs>
  <g filter="url(#shadow)">
    <rect x="100" y="100" width="824" height="824" rx="186" fill="url(#bg)"/>
    <rect x="100" y="100" width="824" height="824" rx="186" fill="url(#shine)"/>
  </g>
  {bars}
  {glyph}
</svg>"""


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
(OUT / "app-icon.svg").write_text(app_icon())
print(f"wrote {len(variants) + 1} SVGs to {OUT}")
