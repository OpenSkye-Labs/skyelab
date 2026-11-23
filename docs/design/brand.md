# SkyeLab brand

<img src="../../crates/skyelab-app/icons/128x128.png" width="64" height="64" alt=""> **SkyeLab**

Grip is Bean, the SkyeLab cloud, pulling itself into view from the lower-left corner. The tilted eyes and small smile are the first read. A separate puff hand hooks the left edge and gives the crop a purpose. The cloud connects to the Skye name, and the grip makes the workspace feel approachable.

The [launcher master](../../crates/skyelab-app/icons/icon.svg) redraws the [approved Grip study](brand/explorations/round5/grip-1024.png) as editable paths. The slightly larger hand and open gap keep the gesture visible at 32 px. At 16 px, the face carries the expression.

## Glyphs and wordmark

Use [Grip](../../crates/skyelab-app/icons/grip.svg) in larger native placements such as the 28 px Welcome mark. Use the [uncropped mini-Bean](../../crates/skyelab-app/icons/mark.svg) at 16 px, where the short gap around Grip's hand reads less clearly. Both SVGs have transparent facial cutouts and take a single foreground color.

Set **SkyeLab** in a medium or semibold system sans, with normal spacing and capital S and L. Vertically align the mark and wordmark, with at least a quarter of the mark's width between them. The README lockup uses a 44 px launcher icon beside live text.

## Palette

| Role | Light | Dark |
| --- | --- | --- |
| Launcher tile | Midnight `#101420` | Midnight `#101420` |
| Launcher cloud | White `#FFFFFF` | White `#FFFFFF` |
| Standalone glyph | Midnight `#101420` on white `#FFFFFF` | White `#FFFFFF` on midnight `#101420` |
| Native glyph | Active theme foreground | Active theme foreground |

The native glyph inherits the T3-grounded [UI theme](t3-reference.md). Account and status colors retain their existing meanings.

## Themes

Settings → Appearance switches between five theme families, each with a light and a dark side
(`crates/skyelab-app/themes/*.json`, gpui-component's theme format):

| Theme | Light | Dark | Accent |
| --- | --- | --- | --- |
| SkyeLab (default) | zinc `#fcfcfc` | true black `#000000` | blue `#1b4ed8` / `#346bf1` |
| Skye | cool white `#f7f9fc` | Midnight `#101420`, the launcher tile | sky `#1d5fd6` / `#5b9bff` |
| Graphite | stone `#f5f5f4` | `#1c1c1e` | teal `#0f766e` / `#2dd4bf` |
| Dawn | paper `#fbf7f1` | ember `#1a1411` | coral `#c2410c` / `#fb8a4a` |
| Grove | `#f6f8f5` | `#0f1512` | leaf `#15803d` / `#4ade80` |

Skye carries the brand: the dark side is the launcher tile's Midnight, so the app and its icon match. An accent preset
(blue, violet, teal, green, amber, rose) overrides any theme's own. Every theme, mode and accent keeps body text and
button text at 4.5:1 or better against its surfaces; `ui/contrast_test.rs` fails the build otherwise. Status colors
(success, warning, danger) and account colors stay the same hues in every theme so their meaning never changes.

## Usage

- Use the launcher tile for package icons and the one-color glyphs in native chrome.
- Keep Grip's lower-left crop, tilted face, and hand gap intact. Use mini-Bean when the placement needs a complete silhouette.
- Keep the navy tile flat and avoid outlines, glow, shadows, extra color, and text inside the tile.
- Run `scripts/icons.sh` after changing the master. The desktop artwork clips both shapes to the rounded tile. Mobile uses the full tile bounds and the launcher applies its own mask.

## Exploration history

1. [Round 1](brand/explorations/) compared a folder, a flask, and a cloud.
2. [Round 2](brand/explorations/round2/comparison.png) tried softly lit, glossy cloud mascots.
3. Round 3 (sheet not kept: it showed other products' logos for reference) moved to flat marks and established Bean.
4. [Round 4](brand/explorations/round4/comparison.png) reframed Bean and established the Corner crop.
5. [Round 5](brand/explorations/round5/comparison.png) explored expressions and gestures. Grip was selected for its small hand on the tile edge.
