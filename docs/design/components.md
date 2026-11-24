# SkyeLab components

SkyeLab draws its controls with its own components in `crates/skyelab-app/src/ui/`, not gpui-component's defaults.
Each looks like its T3 counterpart ([t3-reference.md §6](t3-reference.md#6-primitives)). Each is built the way Zed's
`crates/ui` builds components: a stateless `RenderOnce` builder with size and variant enums (only a tooltip is a
view, built when the pointer rests). Colors come from the theme
and geometry from `ui::Size` and the `ui` constants, so a redesign changes tokens, not call sites.

**The gallery.** In a debug build, or with Developer mode on (Settings → General), open the palette and run **Open Ui Gallery** (`skyelab::OpenUiGallery`). It opens a
window showing every component, variant, size and state in the current theme and family. Redesign from there:
change a token, rebuild, and check both appearances with `SKYELAB_VERIFY_THEME=light|dark`.

`crates/skyelab-app/clippy.toml` bans gpui-component's `Button`, `ButtonVariant`, `Switch`, `Checkbox`, `Tooltip` and
the `Kbd` constructors, so new code reaches for these.

## Sizes (`ui::Size`)

| Size | Height | Text | Icon | Use |
|---|---|---|---|---|
| `Xs` (`.xsmall()`) | 24 | 12 | 14 | dense rows, cards, inline actions, hover buttons |
| `Sm` (`.small()`, default) | 28 | 13 | 14 | toolbars, headers, the composer, settings rows |
| `Md` (`.medium()`) | 32 | 13 | 16 | page actions, forms, a TextField's default |

`Size` also gives each control's side padding, gap and radius (6 for `Xs`, 8 otherwise). Disabled controls fade to
`DISABLED_OPACITY` (64%) and keep their colors.

## Components

- **`Button`** (`ui/button.rs`). Variants:
  - `primary`: the one main action of a surface (Send, Save, Create).
  - `secondary`: a quieter filled action beside a primary one.
  - `ghost`: toolbar and row actions, with no box until hovered.
  - `outline`: a standalone neutral action (Cancel, Choose…, Log in). This is the default.
  - `danger`: destroys or ends something.
  - `warning`: asks for a second look (Re-confirm).

  Builders: `label`, `icon` (leading), `icon_end`, `loading` (spinner, clicks swallowed), `disabled`, `selected` (a toggle
  that is on or a trigger whose menu is open), `toggled` (screen-reader toggle state), `dropdown_caret`, `key_hint`
  (the bound key inside the button), `tooltip` / `tooltip_with_action`, `on_click`. `.dropdown_menu(..)` opens the shared
  popup menu. A press never takes focus from the field you were typing in.
- **`icon_button(id, icon)`**: a ghost, square, icon-only `Button`. Always give it a tooltip.
- **`Switch` / `Checkbox`** (`ui/toggle.rs`): `checked`, `label`, `accessibility_label` (when there is no visible
  label), `disabled`, `xsmall`. `on_click` gets the value the control flips to. Each is a tab stop that Space and
  Enter flip, and a press keeps the focused field focused. A row that is clickable itself takes
  the handler, and the control inside it is only its picture (docs/gpui/elements.md).
- **`SegmentedControl` / `Tabs`** (`ui/choice.rs`): `item`, `icon_item`, `items`, `selected(Some(ix))`, `with_size`.
  `on_click` gets the clicked index. Use a segmented control to pick one setting from a few (merge method,
  appearance) and tabs to switch between sections of a page. Wrap a segmented control in an `h_flex` inside a
  `v_flex`, or it stretches.
- **`TextField`** (`ui/text_field.rs`): a single-line field with our chrome around gpui-component's `Input`
  (`small`, `xsmall`, `prefix`, `suffix`, `cleanable`, `disabled`). An input that sits inside a chrome of its own (the
  composer, the settings search, find-in-thread) keeps the bare `Input` with `appearance(false)`.
- **`Pill`**: T3's badge and SkyeLab's chip, neutral or tinted with `color`, with an optional `icon`.
  **`AccountBadge`**, **`StatusDot`**, **`SectionHeader`**, **`EmptyState`**, **`KeyHint`** are in `ui/components.rs`.
- **`CommandRow`** (`ui/command_row.rs`): a command palette row after T3's: icon, title with `strong` byte ranges
  (matched letters, a project name), an optional muted `subtitle`, then a toggle's tick (`checked`), the `shortcut` as
  plain text and a chevron for a `submenu` row. The list around it draws the hover and selected (accent) background.
- **`Kbd`** (`ui/kbd.rs`): a key cap, `Kbd::for_action(action, context, window)` / `for_action_in(action, focus,
  window)`; `.bare()` drops the cap inside a row or button; `.with_label("Enter")` writes words instead of the symbol (the palette footer). Escape reads "Esc", as in T3: macOS's ⎋ looks like refresh at 12 px.
- **`Tooltip`** (`ui/tooltip.rs`): `Tooltip::new(text).action(..).build(window, cx)`, or
  `.tooltip(Tooltip::text("Copy"))` on any stateful element. Buttons take `.tooltip(..)` directly.
- **`Card` / `Divider`** (`ui/surface.rs`): a card is radius 12 with a hairline border on the muted fill. Use `raised()`
  for one floating over the page and `flush()` for one holding `group_row`s. A divider is a 1 px hairline, `vertical()`
  inside a row.
- **Dialog shell** (`ui/dialog.rs`): the modal itself is gpui-component's `Dialog` (`window.open_dialog`: focus trap,
  Esc, the dimmed `overlay` backdrop). Fill it with a title, the body and `dialog_footer()` holding `dialog_cancel(id)`
  and the action button. `notice_action` is a toast's action button. gpui-component's `Notification` takes only its own
  button type, so this is the one place it is still built.

## Still gpui-component

- **The popup menu** (`DropdownMenu` / `PopupMenu`, `context_menu`): keyboard navigation, submenus and focus. It is
  colored by the theme's `popover`, `accent` and `border`.
- **`Input` / `Textarea`**: text editing. GPUI has no text input of its own.
- **`Dialog`**, **`Notification`** and **`HoverCard`**: layers inside `Root`.
- **The dock and its tab bars**, and **`TextView`** (markdown).
- **The browser crate** (`skyelab-browser`) keeps gpui-component's buttons, since it can't depend on the app's `ui`.

## Restyling

- **Colors**: change the theme JSON (`crates/skyelab-app/themes/*.json`). A button's look is one function,
  `ui::button::colors(variant, selected, theme)`, which maps each variant to theme fields. Change that function to
  change which fields a variant uses.
- **Sizes**: change `ui::Size` (heights, text, icons, padding, gap, radius). Every control follows it.
- **Type and radius**: change the constants in `ui/mod.rs` (`META`, `CODE`, `BODY`, `RADIUS_*`).
- Check the change in the gallery in light and dark, then in the real pages with `/loop-verify-ui`.
