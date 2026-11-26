# GPUI rules: Redraw discipline

Part of [docs/gpui.md](../gpui.md); the same rules apply.

- A `cx.notify()` re-renders the view **and every ancestor view** up to the window root (`Window::mark_view_dirty`).
  A cached view (a dock panel) is reused only while nothing under it notified. So moving a ticking label into its own
  entity doesn't spare its parent's render; keeping the parent's render cheap does (a virtualized `list`).
- `cx.observe(&view, ..)` fires on every notify of that view: each streamed delta, keystroke and timer tick. Don't
  rebuild derived models from it. Emit a specific event when the thing you show changes, or compare against the last
  value you drew and return early (the sidebar keeps a `RowSig` per open thread).
- A per-second tick lives in the smallest view that shows the time, runs only while something ticks, and only
  notifies. It never rebuilds a model.
- A label that changes by the minute ("3m") gets a minute timer that hashes what the labels would read and notifies
  only when that changed and the window is active, plus a re-check on window activation (`sidebar_clock.rs`). A
  `fn() -> i64` clock on the view lets a test move time without sleeping.
- `render` only reads. No `entity.update`, no `set_value`, no restoring state: do that where the state changes (an event
  handler, the task that got the result). To draw another entity's content, read it (`view.read(cx)`) and build its
  click handlers from a `WeakEntity` (see `thread_details::On`).
- Per-frame work is per visible row. Borrow items, don't clone them. Cache what's derived from text (linkified
  markdown) in element state (`window.use_keyed_state`), keyed by the element's id.
- Something slow to derive that a frame needs (a drawn diagram) is a `gpui::Asset` read with `window.use_asset`: it
  starts once per source, is cached for the session and redraws the view when it lands. A render that must be
  capped in time runs on its own thread, raced against a `background_executor().timer`, so a runaway never holds a
  pool thread. The timeout only stops the wait, not the thread: gate those threads with a small semaphore and skip
  the work when the receiver is already gone, or a reply full of diagrams starts one CPU-bound thread each
  (`mermaid::Gate`). Never do that work in a markdown block parser: gpui-component parses small texts on the UI thread.
- A poll that lands every few seconds (PR status) notifies its views only when what it read differs from what they
  show; most polls find nothing new, and each notify rebuilds the sidebar. The same holds for a folder watch firing
  (`folder_watch.rs`): the footer, the details stat and the Changes tab compare the reload with what they show
  (`Footer: PartialEq`, `Changes: PartialEq`) before `set_loaded` and `notify`; `folder_watch_test.rs` proves an
  unchanged reload draws 0 frames.
- Anything that starts threads or OS resources per view (a folder watcher) is off unless a global turns it on at
  launch (`folder_watch::init` in `main.rs`): hundreds of gpui tests open threads on temp repos, and a test opts in.
- Custom markdown blocks (the mermaid card) go through `TextView::markdown_block_parser` / `markdown_block_renderer`;
  equal parser counts keep the parsed document across frames. Match the plain code block by reading the same tokens:
  `theme.muted` background and `theme.semantic_tokens().radius.md`, not `theme.accent`.
- `img(path)` caches the decoded image by path for the whole session: a file rewritten on disk keeps drawing its old
  pixels. For a file the user or agent may change (Preview, the image viewer), read the bytes off the main thread and
  pass an `Arc<gpui::Image>` (`Image::from_bytes` keys it by a hash of the bytes).
- Work queued for a GPUI `Asset` load can't be cancelled when its view goes: the asset cache owns the future. Bound it
  instead (a fixed worker pool, a timeout after which queued jobs are skipped), never a thread per load.
- Never hand untrusted SVG to GPUI's `img()`: its usvg loads `<image href>` from any path. Rasterize with our own
  resvg `Options` whose `image_href_resolver` refuses both data and string hrefs, and hand GPUI the PNG.
- Blocking or O(n) work (store writes, ranking thousands of rows, polling a CLI) goes to `background_spawn`, and waits
  use `background_executor().timer`, never `std::thread::sleep` on the pool.
- A view drawn from another entity (the thread details card reads the thread view) observes it but compares a signature
  of what it draws (`DetailsSig`: every field `render_details` reads, plus the minute for "3m" labels) and notifies
  only when it differs. GPUI coalesces notifies per flush, so the observer runs once per frame at most.
- Element ids and keyed element state of list rows use a key the row keeps (`thread_model::Items` gives each row one
  from a global counter), never the row's index: the timeline is spliced, and an index id hands a row's markdown
  cache, selection or open fold to its neighbour. A list built again gets new keys, so stale state never attaches.
- Measure redraws with `SKYELAB_DEBUG_RENDERS=1` (any build) or debug mode's overlay ([debugging.md](../debugging.md)):
  every view's `render` starts with `let _render = crate::debug_renders::count("Name");`, and stderr gets
  `renders/s Workspace=17 ThreadView=16 timeline row=144 …` once a second. `Workspace` is the frame count per window. A
  new view adds the line as its render's first, keeping the guard: it is also the view's `render` span in a
  `SKYELAB_TRACE` trace. Tests read the counts with `debug_renders::take_test_count` (per test thread, always on).
  Measured: the focused composer's caret blink re-renders `ThreadView` twice a second, and no timeline row.
- A cached view nested under another cached view is **not** reused when that ancestor renders again: on a cache miss
  GPUI sets `window.refreshing` for the whole subtree (`prepaint_view` in `view.rs`). And a notify dirties every
  view on the dispatch path above the notifier (`mark_view_dirty`), so a cached view misses whenever anything
  inside it notifies. Uncached ancestors don't refresh anything. So the cache boundary goes on the expensive part,
  with no cached view above it that a frequent notifier also sits under. Zed works the same way: its panes aren't
  cached views, so an editor's caret blink re-renders the pane but not the cached views beside it.
- gpui-component's skin draws every dock panel cached (`render_active_panel` in its `tab_panel.rs`). The center's
  thread page has a frequent notifier, the composer: gpui-base's text area is a view of its own (`InputBaseState`
  is `Render`) and notifies on each caret blink phase. With `Center` cached, each blink missed the cache and
  rebuilt every visible timeline row, and a cached timeline view under it didn't help (measured on branch
  `t3/split-thread-timeline-view-attempt`). The fix: `center_skin::CenterSkin` wraps the skin through
  `DockArea::with_renderer` and draws the center panel and side chat panels (thread pages too) uncached
  (`TabGroupRenderer::render_active_panel` is the hook for that). `Center` caches non-thread pages itself, and the thread page draws its rows and minimap as a cached
  `TimelineView`. It observes `ThreadView`, so anything `ThreadView` notifies (an event, a delta, a fold, find)
  still rebuilds the rows, but a blink (`InputBaseState`'s own notify) doesn't. `scripts/perf.sh idle`: 156 to 21
  frames, 4,902 to 266 timeline rows, 0 rows in the seconds the caret blinks. Tests: `blink_redraw_test.rs`, `side_chat_blink_test.rs`.
- The list inside `TimelineView` notifies `TimelineView` on a wheel scroll (the view that drew it), not `ThreadView`.
  Whatever reads the scroll position each frame (the minimap's current tick) is drawn by `TimelineView` too.
- Subscriptions to a view that can close live in a map keyed by its `EntityId` and are removed when it closes. A
  one-shot wait removes its own `Subscription` once it fires.

- `cx.emit` delivers in `flush_effects`, after the emitting `update` returns, so a view that asks its parent to
  decide (the composer's multi-model send) must leave its draft untouched when it emits: the parent takes the text
  and files only once it accepts, and a refusal leaves the composer as it was (`compare::compare_send`).

- The caret blink keeps an idle window redrawing twice a second (the thread page minus its timeline rows, which are
  cached). `ui::CURSOR_BLINK` stops blinking after 5 s idle (`CursorBlink::with_idle_timeout`); measured with
  `SKYELAB_DEBUG_RENDERS=1`, idle redraws drop to zero.

- A list's wheel scroll notifies the view that drew the `list` (`cx.notify(current_view)` in `ListState`), so that
  view's render can read `logical_scroll_top()` for "where am I" marks (the minimap's current tick) with no scroll
  handler of its own.
- An overlay drawn from a view's own rows (the timeline minimap) stays in that view's render: inside a dock panel a
  child entity would render on every frame of its parent anyway. Keep its per-frame work to a tag scan of the rows
  and one div per mark, notify only when its hover changes, and derive text (previews) only for the hovered mark.
- A progress bar fed by frequent updates (the update download) is two plain divs (`w(relative(fraction))`), not
  gpui-component's `Progress`: that one runs a `transition` animation on every value change, so a stream of updates
  keeps the window animating. Also send the UI one update per percent, not per read.

- A debug overlay that is an entity re-renders the whole window on each of its ticks (ancestor dirtying), so it is
  absent, not hidden, while off: `Workspace::debug_hud` is `None` and nothing ticks (`debug_hud_test.rs` proves an
  idle window draws 0 frames in 5 s). Its own tick shows up as one frame a second; close it to measure idle.
- GPUI ships a frame-time overlay painted straight into the scene, with no view redraw
  (`window.set_debug_frame_overlay_mode`), but only with its `profiler` feature, which also times every task. We turn
  it on only in `--features gpui-profiler` builds.
- Frame time without GPUI's profiler: note `Instant::now()` at the root view's render and close it in the paint of an
  invisible `canvas` that is the root's last child (`debug_mode::frame_probe`). It covers render, layout, prepaint and
  most of paint; deferred layers paint after it, so it slightly undercounts.
- A smooth scroll of a `ListState` (a minimap jump): GPUI has no animated list scroll. Read the start and the target
  in the list's own pixel estimate (`scroll_px_offset_for_scrollbar` before and after a `scroll_to`, then `scroll_to`
  back, all before anything draws), then step it with `scroll_by` from a stored `Task` on a 16 ms timer for ~200 ms,
  eased out, and end with an exact `scroll_to(target)` (rows measured on the way move the estimate). Stop when
  `logical_scroll_top()` isn't where the last step left it: the user scrolled. The task ends when it arrives, so an
  idle view never redraws for it (`smooth_scroll.rs`). Tests advance it with `executor().advance_clock`.

- Overlapping quads are not free: GPUI orders each painted primitive in a `BoundsTree`, and an insert pays for every
  quad it overlaps. A strip of hundreds of stacked ticks (the minimap's one tick per prompt in a 600-turn thread)
  made `BoundsTree::insert` a third of each streaming frame. Cap what a fixed-size strip draws by its pixels, not by
  the data (`minimap::MAX_TICKS`); measured with `scripts/perf.sh stream`, frame p50 7.7 to 2.1 ms.
- A `ListState` anchors on (row, offset in row), so rows above the first drawn one never move what is shown, but it
  keeps each row's height from when it was measured in the leading overdraw (600 px above the viewport) and maps a
  wheel delta to a row through those heights. A row that finishes laying out later keeps a stale, smaller height there:
  gpui-component parses a `TextView` over 4 KB in the background (its first layout is nearly empty), and diagrams,
  images and HTML cards land late. The wheel step that crosses into such a row lands at its top, a jump of its whole
  height (5,500 px for a 9 KB reply in `timeline_scroll_jump_test`). The timelines call
  `timeline_list::settle_rows_above` as they build the `list`: it `remeasure_items` the 8 rows above the first drawn
  one, so a parse or asset that lands (and re-renders the view) fixes their heights before the reader gets there.
  This is the one write `render` makes, an exception to "render only reads": it only marks list rows for measuring
  (no notify, no entity update), and the rows it marks lie in the leading overdraw the list lays out anyway.
- `ListState::splice` puts a reader whose top row is in the spliced range back at that row's top (offset 0), and drops
  the rows' heights. For rows changed in place (a tool result, a fold toggle, a row refreshed) use `remeasure_items`,
  which keeps the offset; `timeline_list::splice` does that for the rows a change keeps and splices only the rows added or
  removed. Test scrolling with `ScrollEvent` pixel deltas and check each step moves a drawn row by exactly the delta.

- A `list` row is laid out whole on every frame it draws, so one growing row costs more with each delta: a 15 KB
  streamed reply's frames climbed from 2.2 to 3.1 ms (p50) as it grew. Split what grows into rows: the timeline
  model moves a streaming reply's whole markdown blocks (cut after a blank line, never inside a code fence) into a
  `Live` row of their own once the tail passes 2 KB (`timeline::split_live`), so a delta re-lays out only the tail
  and the rows on screen; the complete block joins the parts into one row again. Measured with `scripts/perf.sh
  stream`'s p50 per 10 s: flat at 2.0-2.2 ms. Parts sit together (no row padding between them) to read as one message.
- A `list` lays out its leading overdraw (600 px above the viewport for the timeline) and never paints it: at launch
  the first rows frame builds 44 rows, 28 with no overdraw (`scripts/perf.sh idle`). `set_item_measurement_readiness`
  can't defer it (it marks visible rows provisional and moves the scroll anchor), and deferring it doesn't pay:
  a list made with no overdraw, swapped for the 600 px one at the same anchor on the next render, made that frame
  21-29 ms (28 rows) against 18-23 ms (44 rows) without, plus 11-18 ms for the swap frame (`scripts/perf.sh open`,
  6 runs each, #14). The overdraw rows are cheap next to the visible rows' first layout. Keep each row cheap.
- `TextSystem::prewarm_fonts` does nothing on macOS (gpui-pre-apple doesn't implement it), and warming CoreText
  yourself off the main thread (`layout_width` before the window opens) doesn't speed up the first frames either:
  they wait on each window's own glyph atlas and layout, which only that window can build (#14,
  [debugging-launch-frames.md](../debugging-launch-frames.md)). Don't add a font warm-up.
- A dialog or notification lives in gpui-component's `Root`, so every notify inside it (each palette keystroke or
  hover) re-renders `Root` and, through it, the window's main view. `Root`'s child is therefore a tiny shell that
  returns the workspace as `AnyView::cached` (`windows.rs`, `CachedWorkspace`): the workspace renders only on its own
  notify or a window refresh. Check with `SKYELAB_DEBUG_RENDERS=1`: typing in the palette prints `Palette` and no
  `Workspace`.
