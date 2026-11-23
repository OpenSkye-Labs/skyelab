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
  show; most polls find nothing new, and each notify rebuilds the sidebar.
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
- Measure redraws with `SKYELAB_DEBUG_RENDERS=1` (debug builds): every view's `render` calls
  `crate::debug_renders::count("Name")`, and stderr gets `renders/s Workspace=17 ThreadView=16 timeline row=144 …`
  once a second. `Workspace` is the frame count per window. A new view adds the call as its render's first line.
  Measured: the focused composer's caret blink re-renders `ThreadView` (and rebuilds its visible timeline rows)
  twice a second while idle; only moving the timeline into its own cached view would stop that.
- Subscriptions to a view that can close live in a map keyed by its `EntityId` and are removed when it closes. A
  one-shot wait removes its own `Subscription` once it fires.

- `cx.emit` delivers in `flush_effects`, after the emitting `update` returns, so a view that asks its parent to
  decide (the composer's multi-model send) must leave its draft untouched when it emits: the parent takes the text
  and files only once it accepts, and a refusal leaves the composer as it was (`compare::compare_send`).
