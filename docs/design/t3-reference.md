# T3 Code design reference

SkyeLab's desktop UI rebuilds T3 Code's design language natively in GPUI. This file records the values we
match, where they come from, and how each maps to gpui-component. It is reference data: we copy the layout,
density, type and color system, never T3's name, logo or wordmark.

Source: the clone at `/Volumes/leo-exe/projects/openskye/t3code` (main, `6b38ec931a`). Paths below are relative to
it; `W` means `apps/web/src`. Run `git -C /Volumes/leo-exe/projects/openskye/t3code pull` before relying on a value.
Chat, composer, diff, terminal, palette and settings are in [t3-reference-chat.md](t3-reference-chat.md).

Tailwind units: 1 unit is 4 px; `text-xs` 12 px, `text-sm` 14 px, `text-base` 16 px, `text-2xs` 11 px
(`W/index.css:183`). Desktop values (`sm:` and up) apply.

## 1. Color tokens

T3's light theme is a quiet zinc hierarchy; its dark theme is true black with white-alpha borders and fills.
Selected things are lifted (white in light, a white tint in dark), never colored. Color is spent on status only.

| Token | Light (`W/index.css:1116-1180`) | Dark (`W/index.css:1190-1228`) | gpui-component field |
|---|---|---|---|
| canvas | zinc-25 `#fcfcfc` | `#000` | `background` |
| text | zinc-900 | neutral-100 | `foreground` |
| muted text | zinc-500 | neutral-400 | `muted_foreground` |
| border | zinc-200 | white 8% | `border`, `sidebar_border`, `title_bar_border` |
| input border | zinc-300 | white 11% | `input` (`input.border`) |
| accent (hover fill) | zinc-100 | white 4% | `accent`, `list_hover` |
| muted fill | zinc-50 | white 3% | `muted`, `secondary` |
| card / popover | white | neutral-950 + 3% white `#111` | `popover` |
| primary | `oklch(.488 .217 264)` `#1b4ed8` | `oklch(.571 .21 264)` `#346bf1` | `primary`, `ring` |
| sidebar surface | zinc-50 | `#000` (`[data-app-sidebar]`, 1235-1290) | `sidebar` |
| sidebar row hover | zinc-25 | foreground 8% | (hover in `ui::row`) |
| sidebar row selected | white | foreground 7% | `sidebar_accent` |
| user message | zinc-200/100 mix | neutral-950 + 8% white | `secondary` |
| info / success / warning / error | blue-500 / emerald-500 / amber-500 / red-500 (`-foreground` 700) | same hues (`-foreground` 400) | `info`, `success`, `warning`, `danger` |
| diff add / delete | success / destructive (`W/index.css:2335-2338`) | same | `success` / `danger` at low alpha |

Tinted surfaces (`--warning-surface`, `--error-surface`) are the color at 8% (light) or 16% (dark)
(`W/index.css:1151,1167,1214,1219`). Badges use the same tint with the `-foreground` text (`W/components/ui/badge.tsx:7-34`).

## 2. Type

- Faces: system sans (`-apple-system, BlinkMacSystemFont, "Inter Variable"…`) and system mono (`ui-monospace, "SF Mono"…`)
  (`W/index.css:172-175`). GPUI's default UI font is the platform sans already; mono is `theme.mono_font_family`.
- Sizes: interface 16 px root (`packages/contracts/src/settings.ts:174`), but the UI itself is set in `text-sm` (14) and
  `text-xs` (12), with `text-2xs` (11) for counters and keys. Prompt 14, code 13, terminal 12 (`settings.ts:182-198`).
- Weights: titles and selected rows `font-medium`; page titles `font-semibold`. Nothing heavier.
- Times and counts use `tabular-nums`.

## 3. Geometry

- Radius 8 (`--radius`), derived sm 4, md 6, lg 8, xl 12 (`W/index.css:290-294,1116`). Controls 8, sidebar rows 6,
  badges 4, the composer 16 (`rounded-2xl`).
- Title bar / thread header height 52 (`--workspace-topbar-height`, `W/index.css:123`). Page gutter 20 (`sm`).
- Sidebar width 256 default, 208 min (`W/components/threadSidebarWidth.ts:2-14`).
- Chat column max width 736 (`--chat-content-max-width: 46rem`, `W/index.css:131`).
- Borders, not shadows: 1 px hairlines separate areas. The only shadow is the composer's (light: `0 1px 2px` black 5%;
  dark: none, `W/index.css:199-200`).

## 4. App shell and sidebar

- Sidebar surface `bg-sidebar`; main area `bg-background`; no divider between them in the expanded state
  (`W/components/ui/sidebar.tsx:314,410,417`).
- Header: 52 tall, product name in `text-sm font-medium` (`W/components/sidebar/SidebarChrome.tsx:64-97,188-196`).
- Search / new thread row: 32 tall field, `rounded-md`, `text-sm font-medium` muted (`sidebar/SidebarThreadHeader.tsx:60-123`).
- Menu row primitive: 32 tall, `px-2.5`, `text-sm`, radius 8; hover `sidebar-row-hover`; active `sidebar-row-selected`
  plus `font-medium`; icons 16 in the sidebar icon color (`ui/sidebar.tsx:756-772`).
- Footer: icon buttons (Settings…) 32 square, padding 8 (`ui/sidebar.tsx:671-676`, `SidebarChrome.tsx:213-300`).

### Projects → threads ("folder" mode)

- Project header row (`sidebar/SidebarProjectFolderHeader.tsx:74-125`): 32 tall, `gap-2.5`, `px-2.5`, radius 8, 8 px above
  each project but the first. 16 px favicon/monogram, title `text-sm font-medium` at 90% foreground, truncated. A 6 px status
  dot when a thread needs you (attention warning, failed error, unread success, working info). On hover a 24 px ghost
  "New thread" button appears at the right.
- Monogram fallback: 16 px tile, 25% radius, `currentColor` at 14%, mono 8 px bold (`W/components/ProjectMonogram.tsx:21-38`).
- Thread row (`sidebar/SidebarThreadRow.tsx:1013-1035,1340-1446`): 36 tall in folder mode, `gap-2.5`, `px-2.5`, indented
  36 px (`ps-9`) so titles line up under the project name. Radius 6. Open thread: `sidebar-row-active` plus a 1 px
  `sidebar-border/70` inset ring (light only). Hover `sidebar-row-hover`. Settled rows: muted at 75%.
- Title `text-sm`, single line, ellipsis; `font-medium` unless settled (1096-1125).
- Status: icon + `text-xs font-medium` label in the status color, replacing the time (567-576, 789-849): working info,
  waiting muted, approval/limited warning, failed error, done success.
- Time: right-aligned, `text-xs tabular-nums` secondary label, format `now`, `5m`, `3h`, `2d` (151-165).
- Working rows add the turn's elapsed time after the label (`WorkingDuration`, 178-197, ticking each second); SkyeLab
  formats it like the chat's `AgentElapsed` (`12s`, `1m 12s`, `1h 05m`) and the thread view repeats it as
  "Working for …" (`chat/TimelineWorkGroupRows.tsx` `WorkingTimelineRow`).
- Hover card (`SidebarThreadTooltip`, 263-403, `ThreadHoverCard.tsx`): to the row's right, max width 320, title
  `text-xs font-medium`, then `text-xs` muted lines with 12 px icons (project, branch, worktree, provider). SkyeLab adds
  the PR link, the account + provider, the binding folder and the times (`panels/sidebar_card.rs`, gpui-component
  `HoverCard` anchored `LeftCenter`, 500 ms open delay). Like T3's tooltip, a click closes it and keeps cards closed
  until the pointer moves again (`Sidebar::cards_held`), so a row left or moved under a resting pointer opens nothing.

## 5. Thread header

- 52 tall, `bg-background`, `px-5`, no bottom border in T3's chat (the timeline scrolls under a fade)
  (`W/components/ChatView.tsx:3513-3527`). It is the window's top row (`WorkspacePageHeader.tsx`): no tab strip above
  it, and with the sidebar collapsed it leaves room for the window controls.
- The center shows one page, chosen by route (`W/routes/_chat.*.tsx`): a thread, a draft, the empty state, or
  settings (`_chat.settings.tsx`: a 240 px section nav on the sidebar surface, then a 48 px breadcrumb header). Tabs
  exist only in the right panel (`RightPanelTabs.tsx`).
- Breadcrumb: project (14 px favicon + name, muted) `/` thread title (`text-sm font-medium`, foreground), gap 12, the
  separator in `icon-muted` (`chat/ChatHeader.tsx:254-414`, `W/components/WorkspaceBreadcrumb.tsx:14,27-58`).
- Right: ghost 28 px icon toggles for panels (`chat/PanelLayoutControls.tsx:63-174`). Branch and environment live in the
  composer's context strip, not the header.
- Thread details card (`chat/ThreadDetailsPanel.tsx`, `threadDetailsPanelStyles.ts`): a popover from a square-menu
  toggle in T3. SkyeLab puts it in the right dock's permanent Details tab instead, as a titled card stack (the user's
  Synara reference), with Usage listing every account. Rows 32 tall, `px-2.5`, `gap-2.5`, radius 8, 13 px `font-medium` at 80% foreground,
  16 px muted icons, hover black 3.5% / white 8%. Sections (workspace, version control, lineage, usage) split by a
  hairline. SkyeLab: `workspace/details_panel.rs` and `thread_details*.rs`, radius 16; pickers open as pages inside the card.
- Modal backdrop (`ui/dialog.tsx`, `ui/sheet.tsx`): `bg-background/60 backdrop-blur`. GPUI has no in-window blur, so
  SkyeLab's `overlay` theme token is the canvas at 75%, painted by every gpui-component `Dialog`.
- No app-wide status bar.

## 6. Primitives

- Button (`W/components/ui/button.tsx:8-67`): radius 8, `font-medium`, sizes 32 / 28 (sm) / 24 (xs, `text-xs`) / 20 (micro).
  ghost = transparent, hover accent; outline = `input` border on popover, hover accent/50; default = primary.
  Focus: 2 px inset ring in `ring`.
- Badge (`ui/badge.tsx:7-34`): radius 4, `font-medium`, 18 tall `text-xs`; tinted variants = color 8%/16% + `-foreground` text.
- Kbd (`ui/kbd.tsx:15-19`): 20 tall, `px-1`, radius 4, `text-xs` muted, fill foreground 7%.
- Tooltip (`ui/tooltip.tsx:142-155`): radius 6, border, popover, `text-xs`, `px-2 py-1`.
- Section header (`ui/collapsible-section-header.tsx:5-44`): 32 tall, `px-2`, `text-xs font-medium` muted, a 1 px
  `sidebar-border/50` rule filling the rest of the line, a `tabular-nums` count.
- Context chip (`W/components/ContextChip.tsx:1-104`): inline, no box: `font-medium`, the chip's accent as ink, a 1em
  icon. Environment/model chips in the composer strip are this shape. SkyeLab's prompt chips (terminal, thread, pull request;
  `composer_context.rs`) keep a 28 px boxed chip above the prompt instead, with the kind's icon in its accent, since
  they open to show their text.
- Usage page (`W/usage/UsagePage.tsx`): header with segmented range/metric toggles, a `text-4xl` semibold total, one
  line per provider (dot, name, `text-2xs` session count, value, "x% of cost · tokens"), the daily chart beside it,
  then a table. SkyeLab: Settings → Usage (`workspace/usage_page*.rs`), one line per **account** (the comparison is the
  point), with that account's limit bars, a stacked 30-day bar chart drawn with divs, and a per-project table.

## 7. Empty states

- `ui/empty.tsx:5-115`: centered column, gap 24, title `text-xl font-semibold` (hero 30), description `text-sm` muted,
  content max width 384.
- No thread: header "Threads", body "Pick a thread to continue" plus one line, no actions (`NoActiveThreadState.tsx:9-36`).
- No projects: "What should we work on?", an `sm` "Add project" button with a plus icon (`NoProjectsHero.tsx:30-58`).
- Hosted: title, one `sm` action, then numbered steps (`HostedEmptyState.tsx:28-170`).

## 8. Mapping to GPUI

- **Theme, not views.** All values in §1 live in `crates/skyelab-app/themes/skyelab.json` (gpui-component's theme
  format: `background`, `sidebar.background`, `sidebar.accent.background`, `border`, `input.border`, `primary.background`,
  `ring`, `base.*`…). `ui::install_theme` loads it as the light and dark themes; views read `cx.theme()` only.
- **Tokens** in `crates/skyelab-app/src/ui/mod.rs`: radii (`RADIUS_SM` 4, `RADIUS_MD` 6, `RADIUS` 8, `RADIUS_LG` 12,
  `RADIUS_XL` 16), type (`META` 12, `CODE` 13, `BODY` 14, `SECTION` 16, `PAGE` 20), geometry (`ROW_H` 32, `HEADER_H` 52,
  `CHAT_MAX_W` 736, `PAGE_MAX_W` 896). `ui::ink` darkens a status color for text in light themes (T3's 700 shades).
- **Components** in `ui/components.rs`: `row` (sidebar menu row), `SectionHeader` (caption + rule + trailing control),
  `StatusDot`, `Pill` (T3 badge), `EmptyState` (§7; `.page()` for the large one), `KeyHint` (label + gpui-component
  `Kbd`), `Monogram`, `header` (52 px view header), `page_title`, `section_title`, `group`/`group_row` (settings groups),
  `mono`, `AccountBadge`.
- **gpui-component reused as is:** `Button` (ghost/outline/primary; xsmall = 24, small = 28), `Kbd`, `Input`/`Textarea`
  (with `appearance(false)` inside the composer), `Checkbox`, `Switch`, `Tooltip`, `IconName` icons (needs
  `gpui-kit-assets` registered), the dock's tab bar (styled through `tab_bar.*`/`tab.*` theme fields).
- **SkyeLab's one addition: the account badge.** T3 has no account concept. Ours follows T3's context chip: inline,
  no box in sidebar rows (6 px dot in the account's color, label `font-medium`), and a 20 px tinted chip (color 12%,
  radius 4, like T3's badge, plus the provider in muted text) where it labels a surface: thread header, composer,
  status bar, terminal tab, publish controls, settings. The color comes from the theme's `base.*` palette (red
  excluded, so an account never reads as an error): FNV-1a of the id picks a slot, and configured accounts that clash
  move to the next free slot in sorted-id order, so it is stable across sessions and distinct until there are more
  accounts than colors.
- **Where SkyeLab differs on purpose:** a status bar stays (it carries the bound account and sharing state, which T3 has
  no equivalent for); the right and bottom docks keep gpui-component's tab bars (32 px, so they don't line up with the
  52 px header row); the center has no tabs (`workspace/center.rs` hosts one page as the dock's only center panel); the parent thread link stays in the header breadcrumb;
  Interrupt and Stop stay in the header (a running session still takes follow-ups, so Send never turns into Stop).
- **Not matched yet:** the terminal's 12 px font (skyelab-terminal) and a collapsible PR form.
