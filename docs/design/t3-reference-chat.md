# T3 Code design reference: chat, composer, diff, terminal, palette, settings

The second half of [t3-reference.md](t3-reference.md) (tokens, shell, sidebar, primitives and the GPUI mapping are
there). Paths are relative to `/Volumes/leo-exe/projects/openskye/t3code` (main, `6b38ec931a`); `c/` means
`apps/web/src/components/`, `css` means `apps/web/src/index.css`.

## 1. Message list

- Column: max width 736 (`--chat-content-max-width`, css:131), centered (css:953-958); 48 px side padding on desktop
  (css:2613-2640).
- Spacing after a row: 16 by default, 8 after an assistant message with no meta line, 4 inside a group
  (`c/chat/MessagesTimeline.tsx:1646-1663`).
- User message: right-aligned bubble, `max-w-[80%] rounded-2xl bg-message p-3` (16 radius, 12 padding)
  (`c/chat/TimelineUserMessageRow.tsx:905,940-948`). `--message-surface`: zinc-200/zinc-100 mix (light), neutral-950 +
  8% white (dark) (css:1141,1208).
- Assistant message: no bubble; `px-1 py-0.5`; markdown `text-sm leading-relaxed` (14 / ~22.75), foreground ~90%
  (`c/chat/TimelineAssistantRows.tsx:61`, `c/ChatMarkdown.tsx:2091-2093`).
- Prose (css:1879-2230): blocks 10 px apart; headings semibold, h1 20 / h2 18 / h3 16 / h4+ 14, 20 above and 8 below;
  lists indent 20; inline code radius 4, foreground 10% fill (css:2154-2161).
- Code blocks: `rounded-lg border border-border/70 bg-secondary`, header in mono 11 (`c/chat/MarkdownCodeBlock.tsx:67-79`).

## 2. Tool calls (work log)

- Row: at least 24 tall, `gap-1.5`, `text-sm`, label in `secondary-label` (muted), one line, truncated; radius 6, padding 2
  (`c/chat/WorkLog.tsx:64-95`).
- Icon slot 24 wide with a 16 px icon in `icon-muted`; warnings `warning`, failures `destructive`
  (`c/chat/TimelineWorkEntryRow.tsx:511-569`). Failed icon: `tool-error-icon` at 40% (`c/chat/TimelineToolActivityIcon.tsx:30`).
- Expand chevron 12 px, muted at 70%, rotates 90° (`TimelineWorkEntryRow.tsx:673-679`). Details indent 28 (`ms-7`), max
  height 384; raw output in mono at the code size (fallback 11), muted, max height 256 (`WorkLog.tsx:181-183`,
  `TimelineWorkEntryRow.tsx:192-193`).
- Commands: mono 11 at 85%; a non-zero exit shows `exit N` in destructive (`c/chat/V2ItemInspector.tsx:67-68,210-236`).
  A standalone command block: `rounded-lg border bg-muted/50 font-mono px-3 py-2.5 text-sm` with a muted `$`
  (`c/CommandBlock.tsx:30-36`).
- Running: a 12 px spinner or a text shimmer (`c/chat/TimelineWorkGroupRows.tsx:369-407`).

## 3. Turn meta

- "Worked for {duration}" (`c/chat/MessagesTimeline.logic.ts:1270`). Separator: `h-px` lines at `border/70` either side
  of 12 px muted text, `gap-3`; failures turn the text destructive (`c/chat/MessagesTimeline.tsx:1791-1795`).
- Assistant meta on hover: 12 px tabular time plus copy/fork icons (`TimelineAssistantRows.tsx:242-266`).

## 4. Composer

- Shell: max width 736, radius 16 (`rounded-2xl`), surface `card` (light white, dark `surface-raised` ≈ `#111`), a 1 px
  outline black 8% (light) / white 5% (dark), shadow `0 1px 2px` black 5% in light only (`c/chat/ComposerSurface.tsx:14-41`).
- Body `px-4 pt-4 pb-2` (`c/chat/ChatComposer.tsx:3834-3835`). Editor: at least 2 lines, max 208 tall, 14 px,
  `leading-relaxed`, no focus ring of its own (`c/ComposerPromptEditorTiptap.tsx:1138,1930`). Placeholder in muted.
- Footer inside the card: `px-2 pb-2`, controls right-aligned `gap-0.5`, actions `gap-2` (`ChatComposer.tsx:4042,4065`).
- Chips (`c/chat/ComposerControl.tsx:10-17`): 28 tall, radius 8, transparent border, hover `accent`; `sm` = `px-2
  gap-1.5 font-medium text-sm` at 80% foreground; `xs` = 24 tall, `text-xs` at 70%.
- Send: 32 px circle in primary, up-arrow icon 14 px; disabled = foreground 12% fill, muted icon. Stop: 32 px circle in
  destructive 90% with a 12 px rounded square (`c/chat/ComposerPrimaryActions.tsx:164-180,443-483`).
- Context strip under the card (`ComposerSurface.tsx:72-81`, `c/BranchToolbar.tsx:784-897`): `px-2 pt-2 gap-1 text-xs`,
  muted: environment, a 14 px divider, local/worktree mode, branch on the right. Strip controls 24 tall, `px-1.75`,
  hover accent (`ComposerControl.tsx:12-22`).

## 5. Diff / changes

- Panel header 40 tall, `border-b border-border/60`, `px-4` (`c/DiffPanelShell.tsx:14-33`).
- `+N −N` counts in mono, emerald (`diff-addition-foreground`) and red (`diff-deletion-foreground`), 700 in light, 400
  in dark (`c/chat/DiffStatLabel.tsx:35-46`, css:2335-2338).
- Code surface `code-background` (card 90% + background) (css:1181,2365-2391). File header: sticky, 32 tall, sans 12,
  `px-3 py-2`, counts mono 11 (`c/diffs/StyledDiffCodeView.tsx:97-118`). Hunk separator 24 tall, sans 11 at 50%
  (`StyledDiffCodeView.tsx:123-136`). Modified-file icon: muted blue (css:1292-1300).
- Git actions: ghost `xs` (24) or `sm` (28) buttons; dialogs outline `sm` (`c/GitActionsControl.tsx:1568-1728`).

## 6. Terminal

- Background = app background; font mono 12; cursor `rgb(38 56 78)` light / `rgb(180 203 255)` dark
  (`c/ThreadTerminalDrawer.tsx:1220`, css:1183-1229).
- No tab strip: a floating control group top right; terminals listed in a 144 px side list, items 24 tall `text-xs`,
  radius 6, active `accent` (`ThreadTerminalDrawer.tsx:1428-1459,1861-1937`).

## 7. Command palette

- Popup max width 576, max height 420, radius 16, border, 10vh from the top (`c/ui/command.tsx:48,75-78`).
- Input 34 tall, no border, search icon muted (`command.tsx:112-121`). Items 28 tall, `px-2 py-1.5`, radius 4,
  `text-sm`; highlighted = foreground 9% fill (`command.tsx:197-199`, `c/ui/autocomplete.tsx:127`). Group heading
  `text-xs font-medium` muted (`autocomplete.tsx:148`). Shortcut right-aligned `text-xs` (`command.tsx:214-215`).

## 8. Settings

- Page max width 896, `px-6 pt-6 pb-12`, 32 between sections (`c/WorkspacePageContainer.tsx:8-22`,
  `c/settings/settingsLayout.tsx:662`).
- Section title `text-base font-semibold` (16), 10 above its group (`settingsLayout.tsx:239-249`).
- Group: `rounded-xl` (12), `border-border/60`, `bg-card/40`; rows divided by `border-t border-border/50`
  (`c/settings/SettingsGroup.tsx:18-24`). Row `px-4 py-3`: label `text-sm font-medium`, description `text-xs` muted,
  control on the right (`settingsLayout.tsx:532-569`).

## 9. Mapping to GPUI (chat side)

- Message column: a `v_flex` capped at `ui::CHAT_MAX_W` (736) and centered; user prompts in a `secondary` bubble with
  radius 16 and padding 12; assistant markdown through gpui-component's `TextView::markdown`.
- Tool rows: `ui` tool row = 24 tall, 16 px `IconName` icon in `muted_foreground`, name medium, summary muted mono,
  chevron 12 px; expanded blocks indented 28, mono at `ui::CODE`, on `muted`.
- Turn meta: "Done in 4.2s · $0.04" as muted 12 px between hairlines.
- Composer: one `v_flex` card, radius 16, `popover` surface, `border` outline; `Textarea` with `appearance(false)`
  inside; a footer row with the account chip, the worktree switch and a 32 px round primary Send (icon `ArrowUp`).
- Changes: 40 px header; file rows 28; `+/−` in `success`/`danger`; added/removed line fills at 10–12% alpha.
- Settings: 896 max width, grouped rows (radius 12, hairline dividers), section titles 16 semibold.
- Palette: gpui-component's own; only theme fields (`list.*`, `popover`) are ours.
