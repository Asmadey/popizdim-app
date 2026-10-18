---
name: Popizdim
description: Design system of the Popizdim settings window, modeled on the look of the Raycast macOS settings window
source: values read from Raycast 2.5–2.8 (frontend CSS and bundled themes) and from its settings window on screen
colors:
  light:
    background: "#FFFFFF"
    foreground: "#000000"
    accent: "#138AF2"
    error: "#B12424"
    warning: "#C75D07"
    success: "#006B4F"
  dark:
    background: "#0A0E17"
    foreground: "#FFFFFF"
    accent: "#4FA3F8"
    error: "#FF6363"
    warning: "#FF9217"
    success: "#59D499"
  brand:
    logo-light: "#DD1E3A"
    logo-dark: "#EF4A60"
typography:
  family: "Inter Variable"
  base-size: 13px
  sizes: [11, 13, 16, 18, 24]
  weights: [400, 500, 600]
  features: ["liga", "calt", "kern", "ss03"]
rounded:
  card: 16px
  panel: 20px
  control: 12px
  nav-item: 10px
  tile: 6px
  pill: 9999px
spacing: [2, 4, 6, 8, 12, 16, 24, 32, 40, 48, 56]
---

# Popizdim design system

A calm, flat, Mac-native settings window: a floating frosted sidebar, soft
low-contrast cards, one blue accent, and almost no borders. Everything is a
**tint of the foreground color over the background**, so one pair of colors
(plus an accent) drives both themes.

## 1. Visual theme and atmosphere

- Quiet and dense. Content is the interface; chrome is whisper-light.
- Surfaces are separated by tone (a few percent of foreground), not by lines.
- Depth comes from soft, layered shadows and a frosted-glass sidebar panel, not
  from strong borders or gradients on content.
- Color is rare: the accent (selected toggles, primary buttons, focus) and the
  per-section icon tiles are the only saturated elements. The logo keeps the
  brand red, independent of the interface accent.

## 2. Color palette and roles

Source themes are Raycast Light and Raycast Dark (background, foreground,
accent, plus red/orange/yellow/green/blue/purple/magenta status colors).

| Role                | Light     | Dark      | Notes                                                                 |
| ------------------- | --------- | --------- | --------------------------------------------------------------------- |
| Background          | `#FFFFFF` | `#0A0E17` | window and page (dark is a deep navy, Raycast Dark is pure `#000000`) |
| Foreground          | `#000000` | `#FFFFFF` | all text and tints derive from it                                     |
| Accent              | `#138AF2` | `#4FA3F8` | toggles on, primary button, focus                                     |
| Error / destructive | `#B12424` | `#FF6363` | Raycast red                                                           |
| Warning             | `#C75D07` | `#FF9217` | Raycast orange                                                        |
| Success             | `#006B4F` | `#59D499` | Raycast green                                                         |
| Logo (brand)        | `#DD1E3A` | `#EF4A60` | only the wordmark, never UI controls                                  |

Other Raycast palette entries (not used yet): yellow `#F8A300` / `#FFC531`,
purple `#6A3DEC` / `#A485FF`, magenta `#9A1B6E` / `#CF2F98`,
blue `#138AF2` / `#56C2FF`.

### Foreground opacity ladder

Raycast derives every neutral from the foreground at fixed opacities:
**5 · 10 · 20 · 40 · 60 · 80 %** (`--fg-5` … `--fg-80`). Use the same ladder.

| Use                                  | Value                                       |
| ------------------------------------ | ------------------------------------------- |
| Primary text                         | foreground                                  |
| Secondary text, descriptions         | foreground 60 %                             |
| Tertiary, placeholder, disabled text | foreground 40 %                             |
| List hover                           | foreground 5 %                              |
| List selected (nav item)             | foreground 10 %                             |
| Field fill                           | foreground 5 %                              |
| Field outline (1 px ring)            | foreground 20 % (Raycast) / 16 % (Popizdim) |
| Hairline divider                     | foreground 5–10 %                           |
| Switch track off                     | foreground 20 %                             |
| Text selection                       | foreground 20 %                             |

The selected nav item is a **neutral gray** (foreground 10 %), never the accent
color and never bold.

## 3. Typography

- Family: **Inter Variable** (SIL OFL 1.1; bundled as
  `src/assets/fonts/InterVariable.woff2`), system sans as fallback.
- OpenType features: `liga`, `calt`, `kern`, `ss03`; no optical sizing.
- Raycast base size is **13 px**; the scale is 11 / 13 / 16 / 18 / 24 px.
  Popizdim keeps a 15 px root, so Tailwind `text-sm` ≈ 13 px is the body size.
- Weights 400 (body), 500 (labels, buttons, field text), 600 (rare emphasis).
- Section titles are plain sentence case at body size, weight 500. No
  uppercase, no tracking.
- Descriptions use secondary text (60 %) at `text-xs`; keep them to **one line**.

## 4. Component styling

### Sidebar (floating glass panel)

- Panel inset 8 px from the window, radius **20 px**, width 224 px.
- Fill: background at ~86 % + backdrop blur 28 px, saturate 1.5, 0.5 px inner
  hairline, soft outer shadow (see section 6).
- Nav item: height 32 px, radius **10 px**, 2 px gap, text 13 px regular.
  Selected = foreground 10 %; hover = foreground 5 %.
- Icon tile: **22 px**, radius 6 px, vertical gradient in the section color
  (blue, orange, purple, sky, green, yellow, gray, pink), white 13 px glyph,
  hairline highlight on top. Tiles are the only colorful element.
- Pinned to the bottom of the panel: the active-model selector, a full-width
  pill styled exactly like a nav item (dot, name, chevron).

### Cards (settings groups)

- Radius **16 px**, fill foreground ~4.5 %, 0.5 px hairline, no heavy shadow.
- Rows: horizontal padding 20 px, vertical 12 px, separated by 1 px hairlines
  (foreground 10 %).
- Group title sits above the card, 8 px gap.

### Fields and dropdowns

- Height 32 px (24 px small), horizontal padding 12 px, gap 8 px.
- Fill foreground 5 %, **1 px ring** instead of a border, radius **12 px**.
- Focus: 2 px accent ring. Hover: 1 px accent ring.
- **One line, never wrapped.** When text does not fit, the **end** of the text
  stays visible (paths, keys, names); the full value is in the tooltip. Multi-line
  editors (prompt textareas) are the only exception.
- Dropdown menu: opaque background, radius 16 px, 4 px padding, rows radius
  8 px, selected row foreground 10 %, same layered shadow as panels. It must
  paint above sibling cards (cards use no `backdrop-filter`, so they do not
  create stacking contexts).

### Buttons

- Secondary (default): foreground ~15 % fill, no border, radius 12 px, hover
  foreground ~25 %. Primary: accent fill, white text, tiny shadow, hover
  brightens. Sizes: sm (px 12, text 12), md (px 16, text 13).
- Links to external pages are rendered as secondary buttons.

### Switch

- Pill (`rounded-full`), Raycast macOS medium is 36 × 16 px; Popizdim uses
  42 × 26 px for pointer comfort. Off = foreground 20–30 %, on = accent.
- Thumb white (`#FFFFFFE6`), 22 px, with a faint shadow and 0.5 px outline.
  Thumb motion `.366s cubic-bezier(.16, -.12, .29, 1.31)`, track color `.216s`.

### Overlays

- Tooltips and menus: opaque background, radius 12–16 px, panel shadow.

## 5. Layout principles

- Spacing scale (px): 2, 4, 6, 8, 12, 16, 24, 32, 40, 48, 56.
- Window 790 × 620 (min 680 × 570). Sidebar left, content scrolls on the right
  with 20 px side padding and 20 px between groups. Footer holds only the
  update status and version, right-aligned.
- Icon sizes: 12, 16, 22 (tile), 32.
- Radius scale (Raycast): 2, 4, 6, 8, 10, 12, 16, 18, 20, 26, full. Window
  corners follow macOS (26 px on macOS 26, 20 px on 27); the native window is
  rounded by the OS.

## 6. Depth and elevation

Raycast's panel shadow, reused for the sidebar and menus:

```
0 1px 1px -1px #0000000d,
0 4px 2px -3px #0000000d,
0 8px 4px -6px #0000000d,
0 12px 40px -12px #0000001a,
0 24px 64px -24px #00000033
```

- Cards use only a 0.5 px inner hairline; they do not float.
- Blur steps (Raycast): 6 / 12 / 24 / 36 px (up to 120 px for large backdrops).
- Focus ring: 1–2 px, foreground at 30–50 % (Raycast) or the accent (Popizdim).
- Motion easing: `cubic-bezier(.16, 1, .3, 1)` (ease-out-expo) and
  `cubic-bezier(.34, 1.56, .64, 1)` (ease-out-back).

### Glass and transparency (known pitfalls)

- The frosted effect is a translucent panel blurring **soft tinted gradients on
  an opaque page**. A see-through window (native vibrancy behind the page) made
  elements with `opacity` (dimmed, inert groups) render as mismatched
  rectangles, and made text hard to read. Keep the page background opaque.
- Do not put `backdrop-filter` on cards: every card becomes a stacking context
  and later cards paint over dropdown menus opened in earlier ones.
- WebKit keeps stale text colors in backdrop-filter layers after a runtime theme
  switch; force a repaint when the theme changes.

## 7. Do and don't

Do

- Derive neutrals from foreground opacities (5/10/20/40/60/80).
- Keep selected/hover states neutral; reserve the accent for on-states, primary
  actions and focus.
- Keep every field and description on one line; show the end of long values.
- Use sentence case for titles and labels.

Don't

- No uppercase or letter-spaced section titles.
- No accent-colored or bold selected nav items.
- No hard borders around cards or fields (use the 1 px ring / hairlines).
- No saturated colors outside the accent, status colors and icon tiles.
- Do not copy Raycast icons, images or CSS; only measured values and ratios are
  taken from it.

## 8. Agent prompt guide

> Build UI for Popizdim in the Raycast-style system in this file: Inter 13 px;
> white/black background with one accent (`#138AF2` light, `#4FA3F8` dark); all
> neutrals as foreground opacities 5/10/20/40/60/80 %; cards radius 16 px with a
> 4.5 % foreground fill; fields 32 px high, radius 12 px, 1 px ring, no border;
> nav items 32 px, radius 10 px, selected = foreground 10 %; 22 px gradient icon
> tiles; one-line fields showing the end of long text; layered panel shadow for
> floating surfaces only.

## 9. Provenance and confidence

- **Read directly from Raycast's code:** theme colors, radius/spacing/icon/blur
  scales, font family and base size, foreground opacity ladder, field/dropdown/
  switch/list metrics, shadow and easing values.
- **Measured on screen (1:1 capture of the settings window):** sidebar inset
  (8 px), nav row height (32 px), card radius (~16 px), selected-row tone.
- **Inferred or chosen for Popizdim:** control radius 12 px, switch 42 × 26 px,
  glass blur/saturation, tile gradients and colors, brand-red logo exception.
