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
- GPUI has no animation delay. Stagger rows by giving each a longer duration and an easing that stays at 0 for the
  delay's share: `ease(((t - start) / (1 - start)).clamp(0., 1.))`. Cap the stagger (8 rows) so a long list doesn't
  wait.
- A slide is `.relative()` plus `.left(px(..))` (or `.top`) inside the animator: an offset that doesn't move layout
  around it, so the page's height doesn't jump while it slides.
- `App::reduce_motion` makes every `with_animation` draw its end state with no frames, but GPUI never reads the OS
  setting. On macOS SkyeLab reads `defaults read com.apple.universalaccess reduceMotion` once at launch, off the UI
  thread (`onboarding::motion::reduce_motion`, called from `main.rs`), and sets it.
- A width "morph" (the current progress dot) is the animator setting `.w(px(from + (to - from) * t))`, keyed by the
  step shown: the dot that grows and the one that shrinks each get one run per change.

## App-wide motion (`ui/motion.rs`)

Measured from Rex at 60 fps (window-only `screencapture -v -V <s> -l <id>`; the capture is variable-frame-rate, so a
gap in timestamps means nothing moved). The rule: layout that moves rides one spring; things that appear fade in;
things that switch, get selected or go away change at once.

| Rex transition | Kind | Duration | Curve | SkyeLab |
|---|---|---|---|---|
| Palette open / close / selection | none | 0 | — | palette `DialogEntrance::None` |
| Sidebar hide | width collapse | ~250 ms to rest | critically damped spring, ω ≈ 41 rad/s (`Spring::new(153 ms)`) | every dock's close (`center_skin.rs`) |
| Sidebar show | width; 83% on the first frame | ~200 ms tail | same spring | instant (base opens docks at once) |
| Split / zoom / close pane | spring + ~100 ms fade, new pane scales up from ~0.88 | ~250 ms | same spring | deferred (terminal splits) |
| Tab new / switch / close, settings page, hover, focus | none | 0 | — | already instant |
| Toast | fade in, no slide; leaves at once | 150 ms | ease-out quad | deferred: gpui-component slides 96 px / 400 ms |
| New list row | fade + ~0.97 scale | 80–100 ms | — | deferred |
| Menus, popovers | native (system) | — | — | deferred: gpui-component fades + slides 8 px / 150 ms |

- Put the values into gpui-component's `Theme::motion` (`MotionTokens`) once at `install_theme`; `Theme::change` and
  a family switch keep the field (`motion_test.rs`), so its dialogs, dock tab bars and resize handles follow them.
  The toast and dropdown timings are constants in gpui-component, not tokens: changing them means our own layer.
- `gpui_base::spring(id, target, Spring, window, cx)` is the retargetable spring: it keeps position and velocity in
  keyed element state, asks for frames only while moving, settles within `epsilon` (use `SPRING_PX`, 0.5, over
  pixels or it chases sub-pixels), and adopts the target at once under `reduce_motion`. Its first call adopts the
  target, so keep calling it at rest (`with_travel(false)`) where a later change must animate from the current value.
- gpui-component's docks have no motion and a closed dock renders nothing. A `DockAreaRenderer` can still animate a
  close: base asks `closed_dock_extent(placement)` how big to draw a closed dock and renders its content when that
  is above 0, so the skin answers with the spring's last value (`center_skin.rs`). That hook has no window, so the
  value is one frame old; a test reads a frame after the one that advanced the clock. Hold the closing content at its
  open size and pin it to the edge beside the center: the frame clips it, and a terminal inside never resizes.
- Route every dialog through one helper (`ui::open_dialog`, which sets the entrance) and ban gpui-component's
  `WindowExt::open_dialog` in clippy's `disallowed-methods`: a per-dialog default would drift.
