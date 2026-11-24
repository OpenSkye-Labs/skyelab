# Launch frames with a long thread restored

Part of [debugging.md](debugging.md) ("Performance runs").

Launch with the long thread restored (`open`, M-series Mac, profiling build): the first frame (Welcome) takes
8-12 ms; the frame that first draws the thread page and sidebar, often before the replay lands, 27-40 ms with no
timeline rows; the first rows frame 18-29 ms (44 rows); after that 4-15 ms. When the replay lands first, the two
merge into one 38-54 ms frame. Run-to-run spread is wide, so compare 5+ runs, alternating builds (an env var gate
helps). Measured for #14 and not worth it: deferring the timeline's leading overdraw (see
[gpui/redraw.md](gpui/redraw.md)), mimalloc as the global allocator (the slow frame waits on malloc's lock while
launch threads allocate, but mimalloc didn't move it), and loading every UI and mono font weight after the first
frame (2.9 ms, no change). What the slow frames hold, by samply: the sidebar list's and composer's first text
shaping (CoreText), first-time glyph rasterization, taffy layout and lock waits; an HTML card's WKWebView snapshot
(~20 ms on the main thread) lands between frames, not inside one.
