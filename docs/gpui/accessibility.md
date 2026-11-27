# GPUI rules: Accessibility

Part of [docs/gpui.md](../gpui.md); the same rules apply. Every interactive control, landmark, heading, status message, and text input in SkyeLab must be legible to macOS VoiceOver and `agent-desktop`.

## 1. Use `crate::ui` primitives first

Always build controls from `crates/skyelab-app/src/ui/` ([components guide](../design/components.md)):
`Button`, `icon_button`, `Switch`, `Checkbox`, `SegmentedControl`, `Tabs`, `TextField`, `ui::row` / `row_surface`, `page_title`, `section_title`, `SectionHeader`, `EmptyState`, `dialog_title`, and `ui::toast`.

These primitives automatically attach `.id(...)`, semantic AccessKit `.role(...)`, `.accessibility_label(...)` / `.aria_label(...)`, state attributes (`aria_selected`, `aria_expanded`, `toggled`, `disabled`), and click handling. Drop to raw `div()` only for pure layout or custom composite widgets that explicitly wire the full accessibility contract below.

## 2. How GPUI `0.3.14` exposes AccessKit nodes

- **Both `.id(...)` and `.role(...)` are required.** GPUI only emits an AccessKit node when an element has *both* an `ElementId` and an AccessKit `Role`. Plain strings and un-roled `div`s emit zero accessibility nodes.
- **Every interactive control needs a non-empty name.** `ui::Button`, `ui::Switch`, `ui::Checkbox`, `ui::SegmentedControl`, and `ui::Tabs` enforce this in debug builds via `debug_assert!`. Provide `.label(...)`, `.accessibility_label(...)`, or `.tooltip(...)` on every button, toggle, and choice item.
- **Stable identifiers vs spoken names:** use `.accessibility_id(...)` for stable automation/test IDs (`author_id`), and `.accessibility_label(...)` (or `.aria_label(...)` on raw `Div`) when the accessible name must differ from or clarify visible text. Use `.aria_description(...)` for secondary context (such as an account or model subtitle).

## 3. Click handlers vs `on_mouse_down` & dropdown triggers

- **Never fake clicks with `.on_mouse_down(...)`.** AccessKit registers `Action::Click` (`AXPress` in macOS Accessibility) only from `.on_click(...)` or `.on_a11y_action(Action::Click, ...)`. Elements driven only by mouse-down cannot be triggered by `agent-desktop click` or assistive technology.
- **Dropdown and popover triggers:** `ui::Button` triggers that track open state through `Selectable::open` (used by `.dropdown_menu(...)` and `Popover` triggers without an `.on_click(...)` handler) automatically bind a keyed `FocusHandle`, expose `aria_expanded(open)`, and register `Action::Click` to dispatch `gpui_base::actions::Confirm`. Custom dropdown triggers (such as composer or git pickers) must likewise expose `.aria_expanded(open)` and handle click actions cleanly.

## 4. `Checkbox::decorative()` inside clickable rows

When an entire row owns `.on_click(...)` (for example selectable list rows in import dialogs or file pickers) and displays a checkbox only as its visual indicator, call `.decorative()` on `ui::Checkbox`:

- Without `.decorative()`, GPUI emits a nested unnamed checkbox inside a button, and clicking the checkbox toggles the row twice (once on the checkbox, once on the row).
- `.decorative()` renders the checkbox chrome inside an un-roled layout flex so the row stays the single accessible control (and the row's label describes checked state).
- Reserve `.decorative()` strictly for visual-only checkboxes inside an already-accessible clickable parent.

## 5. Roles, states, landmarks, and labeled inputs

- **Landmarks:** wrap major regions with semantic roles and concise `aria_label`s (`Navigation` for primary rails and settings nav, `Complementary` for sidebars, `Main` for workspace center, `Region` for panels like the composer, `Search` for find bars, `Status` for status bars, and `List` for structured collections like `"Threads"`).
- **Headings, status, and alerts:** mark page/section/dialog headings with `Role::Heading` (`page_title`, `section_title`, `SectionHeader`, `dialog_title`), empty states and non-clickable summary rows with `Role::Status`, and blocking error banners/toasts with `Role::Alert`.
- **States:** expose `.aria_selected(selected)` on tabs and selectable rows (`Role::Tab`, `Role::MenuItem`, `ui::row_surface`), `.aria_expanded(open)` on collapsibles and dropdown triggers, and `.toggled(...)` on toggle controls.
- **Names an agent can act on:** never name a control `Close`, `Minimize` or `Full Screen`; the window's traffic lights already use those names in the same tree, so `agent-desktop` can hit the wrong one (the toast's ✕ is `Dismiss notification`). A control whose visible text is its value (a dropdown showing `Every 5 minutes`) sits inside a named group: `settings::widgets::setting` makes each row a `Role::Group` named by its title.
- **Inputs:** every `ui::TextField` and every bare `gpui_component::input::Input` / `Textarea` must call `.accessibility_label(...)` (and `.accessibility_id(...)` when helpful) so the control has a clear name independent of ephemeral placeholder text.

## 6. Never duplicate `GlobalElementId` in the same frame

GPUI (`window/a11y.rs`) panics in debug builds via `debug_assert!` if two accessible elements in the same frame share the same `GlobalElementId` (identical `.id(...)` under the same parent).

- Parameterize repeated controls, rows, cards, and tool/script items with a unique domain identifier (thread id, path, account id, section key, or tuple index) rather than a static string literal.

## 7. Verifying in the running app with `agent-desktop`

Run a throwaway debug instance with fake CLIs so real `accounts.toml`, logins, and history stay untouched:

```sh
export HOME=$(mktemp -d)
SKYELAB_CLAUDE_BIN=$PWD/crates/skyelab-app/tests/fixtures/fake-claude.sh \
SKYELAB_GH_BIN=$PWD/crates/skyelab-git/tests/fixtures/fake-gh.sh \
SKYELAB_VERIFY_THEME=dark \
  ./target/debug/skyelab
```

- **Do NOT set `SKYELAB_VERIFY_TOPMOST=1` when using `agent-desktop`.** Topmost mode opens a floating `WindowKind::PopUp` / `NonactivatingPanel`, which macOS Accessibility does not return from `agent-desktop list-windows --app skyelab`. Reserve `SKYELAB_VERIFY_TOPMOST=1` strictly for `screencapture` + `SKYELAB_VERIFY_ACTIONS`.
- Typical `agent-desktop` inspection loop:
  1. `agent-desktop list-windows --app skyelab` — capture `window_id` and confirm `accessible: true`.
  2. `agent-desktop snapshot --window-id <id> -i --compact` — confirm every interactive element has a non-empty `name`, expected role (`AXButton`, `AXCheckBox`, `AXTextField`, `AXPopUpButton`, `AXRadioButton`, `AXTab`, etc.), state, and `AXPress` (`"Click"`) action.
  3. `agent-desktop snapshot --window-id <id> --compact` — verify landmarks (`Navigation`, `Main`, `Complementary`, `Status`), headings, alerts, and status nodes.
  4. `agent-desktop click @<ref>` refuses a control scrolled out of view (`live offscreen state is true`). GPUI `0.3.14` ignores `ScrollIntoView` (`AXScrollToVisible`) and scroll containers expose no scroll action, so `scroll` and `scroll-to` do nothing: scroll with the physical wheel, `agent-desktop mouse-wheel --headed --x <screen x> --y <screen y> --dy=-15` over the scroll area (the window's `bounds` from `list-windows` give screen points), re-snapshot and click. It moves the real pointer.
  5. `agent-desktop click @<ref>` — exercise dropdown triggers, tabs, toggles, and dialogs and re-snapshot to confirm state transitions and newly revealed menu/panel controls.

## 8. Automated tests and static audit guardrails

- **`#[gpui::test]` limitation:** `gpui::TestWindow` does not initialize a native AccessKit tree bridge, so `window.debug_a11y_tree_json()` returns `None` in headless tests.
- **Test with `gpui-base` snapshots + AccessKit node probes (`ui::a11y_test`):**
  - Use `gpui_base::test_support::snapshots(window)` (enabled via `gpui-base`'s `test-support` dev-dependency feature) to inspect rendered roles, labels, descriptions, placeholders, and selected/expanded/checked/disabled states, and to dispatch real clicks / confirm actions (e.g., verifying `Button::dropdown_menu` flips `expanded` from `false` to `true`).
  - Probe `Element::a11y_role` and `Element::write_a11y_info(&mut accesskit::Node)` on concrete `ui` elements to verify `author_id`, `supports_action(Action::Click)`, `Role::Heading`, `Role::Status`, `Role::Alert`, and `ui::row`.
- **Snapshot facts to know:** a `SegmentedControl` item is a `RadioButton`, so its state reads as `checked()`, not `selected()`. A field whose editor wasn't painted this frame (scrolled or clipped out) snapshots with `role() == None` and no label; it has no native node yet, so skip role-less snapshots rather than calling them unnamed. `toast::card` alone has no click: `ToastList` adds it to the same element.
- **Static audit (`ui::a11y_audit_test`):** scans `crates/skyelab-app/src/` to catch any custom `.on_click(...)` or left `.on_mouse_down(...)` outside known accessible primitives (unless it is a pure `cx.stop_propagation()` bubble guard or a press that only focuses a region) that omits `.role(...)` or `.aria_label(...)` / `.accessibility_label(...)`, and any `ui::row(...)` / `row_surface(...)` call missing `.aria_label(...)`. It skips only `#[cfg(test)]` items, and a primitive or role inside `.child(...)` doesn't count for the parent. It does not cover `skyelab-browser`, and a `.role(...)` written after `.on_click(...)` is not seen: put role and name first.
  A receiver built by a file-local helper passes when that helper's definition builds a primitive: a `fn`, a `let button = |..| Button::new(..)` closure, a `let seg = SegmentedControl::new(..)` local or a `note: Toast` parameter. A name the file doesn't define passes when every crate `fn` of that name builds one. A helper shared under a common name (`thread_details_card.rs`'s `row`) is listed in `IMPORTED_HELPERS`.
  When it flags a call that really is accessible, teach the audit (and add a probe), never rename code to dodge it. To probe it, drop a throwaway `.rs` file under `src/` with a bare `div().id(..).on_click(..)`: the audit reads files from disk, so the file needn't compile or be in the module tree. Delete it afterwards.
