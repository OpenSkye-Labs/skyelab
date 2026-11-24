# SkyeLab brand

<img src="../../crates/skyelab-app/icons/128x128.png" width="64" height="64" alt=""> **SkyeLab**

Porcelain is the final launcher treatment for Bean, the SkyeLab cloud. Bean pulls itself into view from the lower-left corner. The tilted eyes and small smile are the first read. A separate puff hand hooks the left edge and gives the crop a purpose. The cloud connects to the Skye name, and the grip makes the workspace feel approachable.

The [SVG launcher master](../../crates/skyelab-app/icons/icon.svg) redraws the [approved Porcelain study](brand/explorations/round6/porcelain-1024.webp) with editable paths, warm satin shading, a soft contact shadow, and a cool rim on the navy tile. The larger hand and dark gap keep the gesture visible at 32 px. At 16 px, the face carries the expression.

## Glyphs and wordmark

Use [Grip](../../crates/skyelab-app/icons/grip.svg) in larger native placements such as the 28 px Welcome mark. Use the [uncropped mini-Bean](../../crates/skyelab-app/icons/mark.svg) at 16 px, where the short gap around Grip's hand reads less clearly. Both SVGs have transparent facial cutouts and take a single foreground color.

Set **SkyeLab** in a medium or semibold system sans, with normal spacing and capital S and L. Vertically align the mark and wordmark, with at least a quarter of the mark's width between them. The README lockup uses a 44 px launcher icon beside live text.

## In-app icons

The [Cloud-touched icon gallery](brand/icons/README.md) shows the SVGs used in the workspace. Draw on a
16 × 16 grid with simple silhouettes, rounded corners, and open counters. Use 1.5-unit rounded strokes
for line controls and details. Keep Bean's cloud shape for SkyeLab actions and identities such as new
thread, side chat, agents, and remote work.

Use one `currentColor` for the entire glyph. Cutouts are transparent so the active theme and existing
account and status tints carry through. Use generic shapes for provider glyphs. Check new assets at
14 and 16 px in both themes at 1× and 2×.

## Palette

| Role | Light | Dark |
| --- | --- | --- |
| Launcher tile | Slate `#3D4D68` to midnight `#0D1425` | Slate `#3D4D68` to midnight `#0D1425` |
| Launcher cloud | Ivory `#FFFCF6` to warm gray `#968B85` | Ivory `#FFFCF6` to warm gray `#968B85` |
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
- Keep Bean’s lower-left crop, tilted face, and hand gap intact. Use mini-Bean when the placement needs a complete silhouette.
- Keep Porcelain’s lighting soft and directional. Reserve shading for the launcher artwork and keep native glyphs flat.
- Run `scripts/icons.sh` after changing the master. The desktop artwork clips both shapes to the rounded tile. Mobile uses the full tile bounds and the launcher applies its own mask.

## Exploration history

1. [Round 1](brand/explorations/) compared a folder, a flask, and a cloud.
2. [Round 2](brand/explorations/round2/comparison.png) tried softly lit, glossy cloud mascots.
3. Round 3 (sheet not kept: it showed other products' logos for reference) moved to flat marks and established Bean.
4. [Round 4](brand/explorations/round4/comparison.png) reframed Bean and established the Corner crop.
5. [Round 5](brand/explorations/round5/comparison.png) explored expressions and gestures. Grip was selected for its small hand on the tile edge.
6. [Round 6](brand/explorations/round6/comparison.webp) compared three material treatments and three rebrands. Porcelain preserved the Grip gesture and added soft depth without losing the face at launcher sizes.
