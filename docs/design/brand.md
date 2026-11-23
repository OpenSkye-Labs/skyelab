# SkyeLab brand

<img src="../../crates/skyelab-app/icons/128x128.png" width="64" height="64" alt=""> **SkyeLab**

The mark is a folder with one diamond-shaped opening. The folder is the workspace boundary. The single opening stands for the one provider account bound to that folder. The blue-green tile and pale face evoke a clear sky, while the precise cut and beveled tile suggest a tool on a workbench. The same mark represents the workspace across machines connected through Tailscale.

## Explorations

| Direction | 1024 px | 32 px | 16 px | Decision |
| --- | --- | --- | --- | --- |
| Folder | [PNG](brand/explorations/folder-1024.png) | ![Folder](brand/explorations/folder-32.png) | ![Folder](brand/explorations/folder-16.png) | Chosen. The tab and a single open counter survive reduction, and both have a direct connection to the account binding. |
| Cloud mascot | [PNG](brand/explorations/cloud-1024.png) | ![Cloud](brand/explorations/cloud-32.png) | ![Cloud](brand/explorations/cloud-16.png) | Friendly and legible. The cloud suggests hosted compute, which is secondary to the local workspace. |
| Sky flask | [PNG](brand/explorations/flask-1024.png) | ![Flask](brand/explorations/flask-32.png) | ![Flask](brand/explorations/flask-16.png) | The flask carries the Lab half of the name. The wave inside it loses definition at launcher scale. |

The SVGs live alongside the PNGs. The folder also has [512 px](brand/explorations/folder-512.png) and [128 px](brand/explorations/folder-128.png) proofs.

## Glyph and wordmark

<img src="../../crates/skyelab-app/icons/mark.svg" width="32" height="32" alt="Folder with a diamond cutout"> **SkyeLab**

Use the flat SVG at 16 px in compact chrome and menu-bar artwork, and at 28 px above the Welcome prompt. Its single-color silhouette can take the native foreground color. The diamond is transparent.

Set the wordmark as **SkyeLab**, preserving the capital S and L. Use a medium or semibold system sans, with normal spacing. Align it vertically to the mark and leave at least a quarter of the mark’s width between the two. The README pairs the launcher asset with the live heading. Both SVGs are path artwork and contain no font data.

## Color

| Role | Light | Dark |
| --- | --- | --- |
| Brand ink on neutral backgrounds | `#184553` | `#CAE8E8` |
| Flat glyph in app chrome | Theme foreground or sidebar foreground | Theme foreground or sidebar foreground |
| Launcher tile | `#357A92` → `#184553` | `#357A92` → `#184553` |
| Launcher face | `#FFFFFF` → `#CAE8E8` | `#FFFFFF` → `#CAE8E8` |

The launcher uses one high-contrast tile on both light and dark wallpapers. The brand colors belong to the launcher and external brand treatments. Native chrome follows the zinc light and true-black dark [UI theme](t3-reference.md). Existing account and status tokens carry their existing meanings.

## Usage

- Use the rounded tile for launchers and the flat shape in native chrome.
- Keep the diamond open and preserve the folder tab, proportions, and clear space.
- Avoid putting the glyph inside an account badge or using brand teal as a status color.
- Avoid extra outlines, recolored launcher tiles, and small text inside the tile.
- Regenerate packaged artwork with `scripts/icons.sh` after editing the [launcher source](../../crates/skyelab-app/icons/icon.svg). Keep the folder path aligned with the [flat source](../../crates/skyelab-app/icons/mark.svg).
