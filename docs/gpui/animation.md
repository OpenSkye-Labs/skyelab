# GPUI rules: Animation

Part of [docs/gpui.md](../gpui.md); the same rules apply. Learned building onboarding's motion
(`workspace/onboarding/motion.rs`).

- Use `AnimationExt::with_animation(id, Animation::new(d).with_easing(..), |el, t| ..)` for motion, one-shot (never
  `.repeat()` for decoration). A one-shot animation asks for frames (`window.request_animation_frame`) only until it
  ends, so a settled page draws 0 frames. Test that with `window.simulate_next_frame(cx)`: it runs the frame callbacks
  and returns how many there were, so a count above 0 right after a change and 0 once settled proves both
  (`onboarding_idle_test.rs`).
- The animation's state is element state keyed by its id, kept only while the element draws every frame. A new id
  (the step's index in a page's id) starts it again; the same id across renders keeps it finished. So key a page by
  what it shows, and a list row by the row's own key, never by a counter that changes on every render.
- GPUI has no animation delay. To stagger rows, give each a longer duration and an easing that stays at 0 for the
  delay's share: `ease(((t - start) / (1 - start)).clamp(0., 1.))`. (Onboarding no longer staggers: Rex's crossfade
  moves the whole page at once.)
- A slide is `.relative()` plus `.left(px(..))` (or `.top`) inside the animator: an offset that doesn't move layout
  around it, so the page's height doesn't jump while it slides.
- `App::reduce_motion` makes every `with_animation` draw its end state with no frames, but GPUI never reads the OS
  setting and has no notification for it (desktop platforms emit no `on_app_lifecycle` events). On macOS SkyeLab
  reads `defaults read com.apple.universalaccess reduceMotion` off the UI thread (`ui::motion::follow_reduce_motion`)
  at launch and again on every workspace window activation, which is when a user comes back from System Settings.
  `set_reduce_motion` redraws only when the value changed. It returns at once under `cfg(test)`, or a test's window
  activation would pull the machine's setting in.
- A width "morph" (the current progress dot) is the animator setting `.w(px(from + (to - from) * t))`, keyed by the
  step shown: the dot that grows and the one that shrinks each get one run per change.
- A crossfade (Rex's step change, `onboarding/pages.rs::frame`) draws the old page a second time, `absolute().inset_0()`
  under the new one, fading out, with a `block_mouse_except_scroll()` layer between them (`occlude()` would also eat
  the wheel of the scroll area under it): GPUI has no pointer-events-none, and an
  element at opacity 0 still takes clicks, so the layer keeps a click during the fade off the old page while the
  new one, painted later, stays on top of it. `with_animation` has no done callback either: a `timer` task the length
  of the fade drops the old page and notifies once (one render, then 0), or it would sit there invisible, laid out and
  clickable forever. Skip the old copy entirely under `reduce_motion`. The copy sits under a new parent id, so every
  `with_animation` inside it starts over at t=0: draw its animated parts at their end state (`motion::hero`'s `still`).
- GPUI has no element transform or blur (`Transformation` exists only on `svg()`), so a zoom is the animator setting
  the element's size inside a fixed-size box (`motion::hero`): the box keeps layout still while the tile and its glyph
  scale. Put a test's `debug_selector` on the animated element, not on the fixed box, or its bounds never change.
- Fit an easing to a reference video with `ffmpeg -vf "tblend=all_mode=difference,signalstats"`: the per-frame
  difference is the motion's speed. Rex's speed falls about linearly from the first frame, which is ease-out quad
  (`1 - (1 - t)^2`); ease-out cubic front-loaded visibly more in the same measurement of our window recording
  (`screencapture -v -V 3 -l <id>`).


## App-wide motion (`ui/motion.rs`)

Measured from Rex at 60 fps (window-only `screencapture -v -V <s> -l <id>`; the capture is variable-frame-rate, so a
gap in timestamps means nothing moved). The rule: layout that moves rides one spring; things that appear fade in;
things that switch, get selected or go away change at once.

| Rex transition | Kind | Duration | Curve | SkyeLab |
|---|---|---|---|---|
| Palette open / close / selection | none | 0 | — | palette `DialogEntrance::None` |
| Sidebar hide | width collapse | ~250 ms to rest | critically damped spring, ω ≈ 41 rad/s (`Spring::new(153 ms)`) | every dock's close (`center_skin.rs`) |
| Sidebar show | width; 83% on the first frame | ~200 ms tail | same spring | instant (base opens docks at once) |
| Split pane | spring + ~100 ms fade, new pane scales up from ~0.88 | ~250 ms | same spring | new pane fades 100 ms and its frame shrinks from 0.88 on `spring_ease` (`terminal_split.rs::appear`); the old pane takes its new size at once |
| Zoom / close pane | rect morph; closing pane shrinks + fades, sibling grows | 150–250 ms | same spring | deferred: SkyeLab has no per-pane zoom, and a close would need the closed pane's last bounds to draw a fading copy |
| Tab new / switch / close, settings page, hover, focus | none | 0 | — | already instant |
| Toast | fade in, no slide; leaves at once | 150 ms | ease-out quad | `ui::toast` (our own layer; gpui-component's is banned) |
| New list row | fade + ~0.97 scale | 80–100 ms | — | `motion::appear_row` (new thread in the sidebar, new timeline rows) |
| Menus, popovers | native (system) | — | — | already instant: gpui-component's `PopupMenu`, `DropdownMenu`, context menu, and `HoverCard` have no motion; `Select` is banned in clippy in favor of `ui::Button::dropdown_menu`. Floating overlays (`settings_modal`, `details_card`, `env_menu`) fade in over `FADE` / `FADE_FAST` with a `t³` shadow ramp (`motion::overlay_shadow`) and dismiss at once. |
| Switch thumb | critically damped spring | ~150 ms to rest | `SPRING_PX` | `ui::Switch` (`ui/toggle.rs`) drives thumb `.ml(...)` via `gpui_base::spring`; 0 idle frames, snaps under `reduce_motion` |

- Put the values into gpui-component's `Theme::motion` (`MotionTokens`) once at `install_theme`; `Theme::change` and
  a family switch keep the field (`motion_test.rs`), so its dialogs, dock tab bars and resize handles follow them.
  The toast and dropdown timings are constants in gpui-component, not tokens: changing them means our own layer.
- `gpui_base::spring(id, target, Spring, window, cx)` is the retargetable spring: it keeps position and velocity in
  keyed element state, asks for frames only while moving, settles within `epsilon` (use `SPRING_PX`, 0.5, over
  pixels or it chases sub-pixels), and adopts the target at once under `reduce_motion`. Its first call adopts the
  target, so keep calling it at rest (`with_travel(false)`) where a later change must animate from the current value.
- The dock skin (`ui::dock::Skin`) has no motion of its own and a closed dock renders nothing. A `DockAreaRenderer` can still animate a
  close: base asks `closed_dock_extent(placement)` how big to draw a closed dock and renders its content when that
  is above 0, so the skin answers with the spring's last value (`center_skin.rs`). Base also reads that hook as the
  bottom dock's closed strip during a resize drag, so report more only while a close runs, started by the toggle
  (`CenterSkin::start_close`), never while the dock is open. That hook has no window, so the
  value is one frame old; a test reads a frame after the one that advanced the clock. Hold the closing content at its
  open size and pin it to the edge beside the center: the frame clips it, and a terminal inside never resizes.
- Route every dialog through one helper (`ui::open_dialog`, which sets the entrance) and ban gpui-component's
  `WindowExt::open_dialog` in clippy's `disallowed-methods`: a per-dialog default would drift.
- Toasts are SkyeLab's own (`ui/toast.rs`): one `ToastList` entity per window, kept in a global map by `WindowId`
  (the window being updated is missing from `cx.windows()`, so keep it by id when pruning), drawn by the workspace's
  render as a `deferred` layer with priority 100 (above gpui-component's dialogs at 10 + index). Clippy bans
  `WindowExt::push_notification` and `notification::Notification`; `toast::push_system` is the one exception, an
  OS-only notification for a window in the background, since gpui-component owns the app's one OS response handler.
- A fade over something with a shadow ramps the shadow by t³: GPUI has no group compositing, so a half-faded card
  shows its own shadow through as a dark slab.
- One curve source: `motion::ease_out_quad` (a `fn(f32) -> f32` for `with_easing`) and `motion::spring_ease`, the
  critically damped spring as a one-shot easing over `motion::SETTLE` (250 ms) for motion nothing retargets.
- "Appear once" needs a marker outside element state. A row in a virtual `list` is a fresh element each time it
  scrolls into view, and a terminal pane's element path changes when its parent splits, so keying `with_animation`
  by row or pane alone would replay. Record what appeared and when (`Sidebar::appeared`, `ThreadView::appeared`,
  `TerminalPanel::appearing`) and wrap only while that is recent (a timer clears the pane's). Rows give a late
  first draw 3× the fade before they just show.
- No transform, so a "grow" that must not reflow keeps the final layout and hides the difference: a new terminal pane
  has its final size (the grid resizes once) and four background-colored bars of `relative()` width cover its edges
  and shrink to 0; a new list row (`motion::appear_row`) uses `.relative().top(px(4. * (1. - t)))` + `.opacity(t)` so
  neither its width (line wrapping on multi-line messages) nor the list's height ever moves mid-fade.
- Reduce Motion works across platforms: macOS reads `defaults read com.apple.universalaccess reduceMotion`, Linux reads
  `gsettings get org.gnome.desktop.interface enable-animations`, and Settings → Appearance → Motion (`MotionMode::Reduced`)
  lets any platform turn animations and loading spinners off immediately (`ui::motion::apply_reduce_motion`).
- A drag that base owns (a dock's resize handle, whose state is crate-private in gpui-component) can still be watched
  from the skin: paint a `canvas` over the dock that registers **capture-phase** window mouse listeners (the handle
  calls `stop_propagation` in bubble), arm on a press inside the handle's hit area (5 px at the edge), and act on the
  release only if `cx.has_active_drag()` (still set during the release's listeners), so a press with no drag (one
  under a dialog) never counts. Don't require the drag on moves: GPUI starts it a move late, so a quick flick lands
  before it. Act through `window.defer`, after base's own mouse-up settles the drag (`center_skin.rs::track_sidebar_drag`).
  The canvas needs `.absolute().inset_0()`: an absolute box with no offsets stays at its flow position (below the
  content), so its bounds were off the dock and every press missed.
- A drag whose source disappears mid-drag (the hidden sidebar's restore handle, gone once the sidebar shows) keeps
  working if the `on_drag_move::<T>` listener sits on an element that stays drawn, such as the workspace root: it
  fires on every move while a `T` is dragged, wherever the pointer is.
