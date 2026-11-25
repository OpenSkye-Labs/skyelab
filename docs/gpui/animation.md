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
  setting. On macOS SkyeLab reads `defaults read com.apple.universalaccess reduceMotion` once at launch, off the UI
  thread (`onboarding::motion::reduce_motion`, called from `main.rs`), and sets it.
- A width "morph" (the current progress dot) is the animator setting `.w(px(from + (to - from) * t))`, keyed by the
  step shown: the dot that grows and the one that shrinks each get one run per change.
- A crossfade (Rex's step change, `onboarding/pages.rs::frame`) draws the old page a second time, `absolute().inset_0()`
  under the new one, fading out, with an `occlude()` layer between them: GPUI has no pointer-events-none, and an
  element at opacity 0 still takes clicks, so the occluder keeps a click during the fade off the old page while the
  new one, painted later, stays on top of it. `with_animation` has no done callback either: a `timer` task the length
  of the fade drops the old page and notifies once (one render, then 0), or it would sit there invisible, laid out and
  clickable forever. Skip the old copy entirely under `reduce_motion`.
- GPUI has no element transform or blur (`Transformation` exists only on `svg()`), so a zoom is the animator setting
  the element's size inside a fixed-size box (`motion::hero`): the box keeps layout still while the tile and its glyph
  scale. Put a test's `debug_selector` on the animated element, not on the fixed box, or its bounds never change.
- Fit an easing to a reference video with `ffmpeg -vf "tblend=all_mode=difference,signalstats"`: the per-frame
  difference is the motion's speed. Rex's speed falls about linearly from the first frame, which is ease-out quad
  (`1 - (1 - t)^2`); ease-out cubic front-loaded visibly more in the same measurement of our window recording
  (`screencapture -v -V 3 -l <id>`).
