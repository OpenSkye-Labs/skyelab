# SkyeLab brand

<img src="../../crates/skyelab-app/icons/128x128.png" width="64" height="64" alt=""> **SkyeLab**

Fairweather is Bean, the SkyeLab cloud, peeking into a morning sky. The lower-left corner cuts through the ivory body. Three rounded lobes are visible above the two round eyes and tilted smile. Keep the face fully inside the tile and leave open sky above it.

The [Corner Peek launcher](../../crates/skyelab-app/icons/icon.svg) follows the [approved Fairweather study](brand/explorations/round7/fairweather-corner-peek.png). It uses a storm-blue contour, a wider white rim, and warm porcelain shading. The distant cloud shelves sit high on the right. In the [small launcher](../../crates/skyelab-app/icons/icon-small.svg), the eyes sit farther apart, the smile is heavier, and a single shelf suggests the sky at 16 px.

## Glyphs and wordmark

Use [Peek](../../crates/skyelab-app/icons/peek.svg) for larger placements such as the 28 px Welcome mark. Use [compact Peek](../../crates/skyelab-app/icons/mark.svg) at 16 px. Both keep the cropped lower-left corner and use transparent cutouts for the eyes and smile. The compact mark opens those cutouts to preserve the expression.

Set **SkyeLab** in a medium or semibold system sans, with normal spacing and capital S and L. Vertically align the mark and wordmark, with at least a quarter of the mark’s width between them. The README banner pairs the title and tagline with the launcher on the right.

## In-app icons

The [Cloud-touched icon gallery](brand/icons/README.md) shows the SVGs used in the workspace. Draw on a
16 × 16 grid with simple silhouettes, rounded corners, and open counters. Use 1.5-unit rounded strokes
for line controls and details. Keep Bean's cloud shape for SkyeLab actions and identities such as new
thread, side chat, agents, and remote work.

Use one `currentColor` for the entire glyph. Cutouts are transparent so the active theme and existing
account and status tints carry through. Provider logos are the exception: they are the
brands' own files, drawn as images in their official colors, never tinted (see [third-party-marks.md](third-party-marks.md)). Check new assets at
14 and 16 px in both themes at 1× and 2×.

## Palette

| Role | Light | Dark |
| --- | --- | --- |
| Launcher sky | Morning blue `#3D83C7` through `#76B9E6` to pale sky `#C8E3ED` | Same artwork |
| Bean | Ivory `#FFFFFA` through `#FFF4E4` to satin gray `#C7C7CB` | Same artwork |
| Contour and rim | Storm blue `#385F82` inside white `#FFFFFF` | Same artwork |
| Eyes and smile | Blue ink `#203D58` to `#354D69`, smile `#293F55` | Same artwork |
| Standalone glyph | Midnight `#101420` on white `#FFFFFF` | White `#FFFFFF` on midnight `#101420` |
| Native glyph | Active theme foreground | Active theme foreground |

The native glyph inherits the T3-grounded [UI theme](t3-reference.md). Account and status colors retain their existing meanings.

## Themes

Settings → Appearance switches between fourteen theme families, each with a light and a dark side
(`crates/skyelab-app/themes/*.json`, gpui-component's theme format; hex colors only, since gpui-component drops names it
can't parse). SkyeLab's own:

| Theme | Light | Dark | Accent |
| --- | --- | --- | --- |
| SkyeLab (default) | porcelain `#fbfaf8`, ink `#16181d` | midnight slate `#111318` | blue `#3358d4` / `#4c6fe0` |
| Classic | zinc `#fcfcfc` | true black `#000000` | blue `#1b4ed8` / `#346bf1` |
| Skye | cool white `#f7f9fc` | Midnight `#101420` | sky `#1d5fd6` / `#5b9bff` |
| Graphite | stone `#f5f5f4` | `#1c1c1e` | teal `#0f766e` / `#2dd4bf` |
| Dawn | paper `#fbf7f1` | ember `#1a1411` | coral `#c2410c` / `#fb8a4a` |
| Grove | `#f6f8f5` | `#0f1512` | leaf `#15803d` / `#4ade80` |

The default theme pairs a warm ivory light canvas with a midnight slate dark canvas. Both use blue for focus and action.
Classic is the zinc and true-black theme SkyeLab shipped first. Settings saved before the change say `skyelab` whether
the user picked it or never touched the setting, so everyone on that key gets the new default; Classic is one click
away.

Eight more are ports of well-loved editor palettes: Catppuccin, Rosé Pine, Tokyo Night, Kanagawa, Everforest, Gruvbox,
Nord and Flexoki ([NOTICE.md](../../NOTICE.md) has sources and licenses). `scripts/gen-themes.py` writes these and the
default from a few source colors per side, derives every other key, and nudges any color under the contrast floor.

The Skye theme pairs midnight chrome with a sky-blue accent. An accent preset
(blue, violet, teal, green, amber, rose) overrides any theme's own. Every theme, mode and accent keeps body text and
button text at 4.5:1 or better against its surfaces; `ui/contrast_test.rs` fails the build otherwise. Status colors
(success, warning, danger) stay green, amber and red in SkyeLab's own families; a port uses its palette's nearest
hues (Rosé Pine, which has no green, marks success in pine teal), and the diff's `+` and `-` carry the meaning too.
Account colors stay the same in every theme.

## Usage

- Use the launcher tile for package icons and the one-color glyphs in native chrome.
- Keep the lower-left crop, the three visible cloud lobes, and the tilt between the eyes. Leave room under the smile.
- Keep lighting soft and directional on the launcher. Use a single foreground color in native chrome.
- Run `scripts/icons.sh` after editing either launcher master. Desktop PNG, ICO and ICNS representations up to 32 physical pixels use the small master. Mobile uses the full-bleed large master and the platform applies its own mask.

## Exploration history

1. [Round 1](brand/explorations/) compared a folder, a flask, and a cloud.
2. [Round 2](brand/explorations/round2/comparison.png) tried softly lit, glossy cloud mascots.
3. Round 3 (sheet not kept: it showed other products' logos for reference) moved to flat marks and established Bean.
4. [Round 4](brand/explorations/round4/comparison.png) reframed Bean and established the Corner crop.
5. [Round 5](brand/explorations/round5/comparison.png) explored expressions and gestures. Grip was selected for its small hand on the tile edge.
6. [Round 6](brand/explorations/round6/comparison.webp) compared three material treatments and three rebrands. Porcelain preserved the Grip gesture and added soft depth without losing the face at launcher sizes.
7. [Round 7](brand/explorations/round7/fairweather-corner-peek.png) compares Porcelain, Bottom Peek, and the selected Corner Peek across launcher sizes.
