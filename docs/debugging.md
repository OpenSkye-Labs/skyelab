# Debugging and profiling SkyeLab

How to find out why SkyeLab is slow, what it leaks, and what it said to other machines and programs. Everything here is
for people working on SkyeLab. It is off by default, and off it costs nothing: no overlay, no timer, no per-frame
element, no log. The env switches and the overlay work in release builds too, because performance problems often only
show there. The heavier profilers are cargo features, so a normal build never contains them.

## Which tool answers which question

| Question | Tool | Section |
|---|---|---|
| Is a frame slow? Which views redraw too often? | Debug overlay, `SKYELAB_DEBUG_RENDERS=1`, slow-frame log | [Overlay](#the-overlay) |
| Where does a slow operation spend its time, step by step? | `SKYELAB_TRACE` → Perfetto timeline (or Tracy, live) | [Traces](#traces-perfetto-and-tracy) |
| Which functions are hot overall (count, p50/p95/p99, total)? | `--features hotpath` | [hotpath](#hotpath-function-timing-and-allocations) |
| Where is CPU time going, down to std and GPUI? | `scripts/profile.sh` (samply or cargo-flamegraph) | [Flame graphs](#flame-graphs) |
| Which functions allocate the most? | `--features hotpath-alloc` | [hotpath](#hotpath-function-timing-and-allocations) |
| What is still on the heap at exit, and who allocated it? | `--features dhat-heap` | [Heap profiles](#heap-profiles-dhat) |
| Does something leak (views, terminals, connections, processes)? | Live counts in the overlay + **Dump Debug State** | [Leaks](#hunting-a-leak) |
| What went over the network, how long did it take, did it fail? | Debug log (overlay **Log** tab, `debug.log`) | [Debug log](#the-debug-log) |
| Are the daemon's or client's tokio tasks stuck or busy? | `--features tokio-console` | [tokio-console](#tokio-console) |

Tool choices, checked 2026-11: hotpath 0.33 (released 2026-11-22, actively maintained), samply 0.13 and
cargo-flamegraph 0.6.15 for flame graphs, tracing 0.1 with tracing-chrome 0.7 (stable; Perfetto opens its Chrome JSON),
tracing-tracy 0.12, console-subscriber 0.5, dhat 0.3. tracing-perfetto (0.1.5) was passed over: Perfetto reads
Chrome JSON and tracing-chrome is the more used crate.

## Turning debug mode on

Any of these turns it on:

- `SKYELAB_DEBUG=1` (also `true`, `on`, `yes`) for one run. The first window opens with the overlay.
- **Settings → General → Advanced → Developer mode**: on at every launch until you turn it off (saved as
  `developer_mode = true` in `settings.toml`).
- **Toggle Debug Overlay** in the command palette (<kbd>⌘K</kbd>): shows the overlay and turns debug mode on for the
  rest of this run.

While it is on, the status bar shows a **Debug** chip that shows or hides the overlay, and the palette has **Toggle
Debug Overlay**, **Open Debug Log** and **Dump Debug State**.

| Switch | What it does | Builds |
|---|---|---|
| `SKYELAB_DEBUG=1` | Debug mode on, overlay open in the first window | all |
| `SKYELAB_DEBUG_RENDERS=1` | Prints `renders/s Workspace=17 ThreadView=16 …` to stderr once a second | all |
| `SKYELAB_TRACE=path.json` | Writes a Chrome/Perfetto trace there, flushed on quit | all |
| `SKYELAB_VERIFY_*`, `SKYELAB_*_BIN` | Live-check hooks and fake agents, gh and tailscale (see CLAUDE.md) | debug only |
| `SKYELAB_GIT_CA_FILE` | Push and fetch trust this CA (the fake https remote's, see CLAUDE.md) | debug only |
| `--features hotpath` / `hotpath-alloc` | Function timing / allocation report on quit | opt-in |
| `--features dhat-heap` | dhat heap profile on quit | opt-in |
| `--features tracy` | Live Tracy profiling of the same spans | opt-in |
| `--features tokio-console` | tokio-console for the daemon and client runtimes | opt-in |
| `--features gpui-profiler` | GPUI's own frame-time overlay next to ours | opt-in |

Quit with <kbd>⌘Q</kbd> or the menu, not Ctrl-C in the terminal: traces and profiler reports are written on quit,
and a killed process writes nothing.

## The overlay

The overlay sits at the bottom right of the window. **Stats** shows:

- **Frames**: frames per second averaged over the last 5 seconds, the last frame's time and the worst of those 5 s. A frame is timed from the window
  root's `render` to its paint (render, layout, prepaint and most of paint), for every window. A frame over 50 ms is
  also printed to stderr (`skyelab: slow frame 63.0 ms`) and logged.
- **Renders/s, busiest views**: the same counts `SKYELAB_DEBUG_RENDERS` prints. `Workspace` renders once per frame per
  window, so it is the frame count; `timeline row` is rows built.
- **Live objects**: windows, thread views, terminals, browser tabs, agent sessions, remote connections and folder
  watches (one per folder a thread tab or Changes tab shows) alive now, and this process's child processes (agents,
  shells, git), read once a second off the UI thread.

GPUI keeps no public count of its entities, so the overlay counts SkyeLab's own long-lived objects instead; for
entity handles, run tests with GPUI's leak detector (see [Hunting a leak](#hunting-a-leak)).

The overlay ticks once a second, and a tick re-renders the whole window (a notify dirties every ancestor, see
[gpui/redraw.md](gpui/redraw.md)). So with it open an idle window shows about 1 frame a second: that one is the
overlay's own. Close it to measure idle redraws exactly, or use `SKYELAB_DEBUG_RENDERS=1` without it.

Builds with `--features gpui-profiler` also turn on GPUI's built-in frame overlay at the top right (current, 1%, 10%
and max draw time, frame count), which GPUI paints straight into the scene without redrawing any view.

## Traces (Perfetto and Tracy)

```sh
SKYELAB_TRACE=/tmp/skyelab-trace.json cargo run -p skyelab-app --release
# use the app, quit with ⌘Q, then open the file at https://ui.perfetto.dev (or chrome://tracing)
```

Spans in the trace:

| Span | Where |
|---|---|
| `render` (field `view`) | every view's `render` (the same calls the render counter makes) |
| `thread.replay` | a thread's stored events turned into timeline rows when it opens (UI thread) |
| `sidebar.refresh` | the sidebar's rows rebuilt from the thread list |
| `frame` event (field `ms`) | each frame debug mode timed (`scripts/perf.sh` reads them) |
| `store.events_since`, `store.append_event`, `store.thread`, `store.list_threads`, `store.list_all_threads`, `store.search_threads` | SQLite queries |
| `agent.parse_line` | one stdout line of Claude or Codex parsed into events |
| `process` (field `program`) | every short-lived child: git, gh, tailscale, the agents' one-shot calls |
| `daemon.dispatch` | one RPC served to another machine |
| `mcp.tool` (field `tool`) | one tool call from an agent to SkyeLab's MCP server |
| `rpc` event (fields `what`, `ms`, `ok`) | each RPC answered, client and daemon side, while debug mode is on |

Fields hold view and tool names and program paths, never prompts or arguments. The trace keeps only SkyeLab's own
spans (targets starting `skyelab`), not those of other crates, and the file is created 0600. It can still contain
file paths, so look before you attach it to a public issue.

Without `SKYELAB_TRACE` no subscriber is installed and each span costs one callsite check.

**Tracy** shows the same spans live, with zones and plots, while the app runs: build with `--features tracy`, start
the [Tracy profiler](https://github.com/wolfpld/tracy) app (the Tracy version the tracing-tracy 0.12 README names: the
wire protocol must match), and connect it to the running SkyeLab. The build turns off tracy's network broadcast and listens on localhost only.

```sh
cargo run -p skyelab-app --release --features tracy
```

Add a span to new hot code with `#[tracing::instrument(name = "area.what", skip_all)]` on a function, or
`let _span = tracing::info_span!("area.what").entered();` for a block. Always `skip_all` (or name the fields): an
argument can be a prompt.

## hotpath: function timing and allocations

[hotpath](https://hotpath.rs) measures the instrumented functions and prints a table on quit: calls, average, p50,
p95, p99, total and share of the run.

```sh
cargo run -p skyelab-app --release --features hotpath          # time per call
cargo run -p skyelab-app --release --features hotpath-alloc    # bytes allocated per call, too
```

The report goes to `<local data>/skyelab/debug/hotpath-report.txt` (macOS: `~/Library/Application
Support/skyelab/debug/`) unless `HOTPATH_OUTPUT_PATH` names another file (`/dev/stderr` works). Useful env vars:
`HOTPATH_OUTPUT_FORMAT=json`, `HOTPATH_FOCUS=store` (only names matching), `HOTPATH_ALLOC_CUMULATIVE=true` (include
nested calls in alloc numbers). While it runs, hotpath serves live numbers on `127.0.0.1:6770` for its TUI
(`cargo install hotpath --features tui`, then `hotpath console`); `HOTPATH_METRICS_SERVER_OFF=true` turns that off.

How to read it: start from **% Total** for where time goes in all, and **P95/P99** against **Avg** for functions that
are usually fast but sometimes stall (a lock, a disk wait). In alloc mode the numbers are bytes allocated by the
function itself, not its callees, unless `HOTPATH_ALLOC_CUMULATIVE=true`.

The instrumented functions are the same hot paths as the trace spans: the store queries, `claude::parse`, Codex's
`on_line`, `process::output`/`output_within`, `daemon::dispatch`, the MCP server's `respond` and the timeline's
`render_item`. Client requests are async and are timed in the debug log and as `rpc` trace events instead. Add one with
`#[cfg_attr(feature = "hotpath", hotpath::measure)]` (the crate needs the optional `hotpath` dependency and feature,
as `skyelab-store` has). Labels must be unique per crate. `hotpath-alloc` replaces the global allocator, so it can't
be combined with `dhat-heap` (the build says so).

## Flame graphs

```sh
scripts/profile.sh              # samply if installed, else cargo-flamegraph
scripts/profile.sh --real       # release semantics, no fakes
scripts/profile.sh --tool flamegraph
```

The script builds the `profiling` profile (release code with line-table debug info, not stripped:
`[profile.profiling]` in `Cargo.toml`), runs the app in a throwaway `HOME` under `target/profiles/<time>/` with the fake
agents, gh and tailscale and `SKYELAB_DEBUG=1`, and prints where the profile is when you quit. The fakes are honored
only in builds with debug assertions, so the default run turns them on for that build (`--real` leaves them off).
For a repeatable run, script it and let it quit itself:

```sh
SKYELAB_VERIFY_ACTIONS='skyelab::NewTerminal,skyelab::OpenSettings,skyelab::Quit' scripts/profile.sh
```

Install one tool first (neither is needed to build SkyeLab):

- **samply** (preferred, macOS and Linux): `cargo install samply --locked`. Open the result with
  `samply load <profile.json.gz>`; it shows a flame graph, call tree and timeline per thread in the Firefox Profiler.
- **cargo-flamegraph**: `cargo install flamegraph --locked`. Writes an SVG. It uses dtrace on macOS (asks for sudo)
  and `perf` on Linux.

Read a flame graph from the bottom: width is time. Look for wide towers under `skyelab-app` frames on the main thread
(that is the UI thread: anything wide there is a slow frame), and filter by thread name for agent readers
(`skyelab-agent-reader`) and the daemon's tokio workers.

## Performance runs

`scripts/perf.sh <scenario>` is the repeatable benchmark: it builds the `profiling` profile, makes a fixture HOME
with `crates/skyelab-app/examples/perf_fixture.rs` (40 projects, 601 threads, one "Long history" thread of ~6,800
events with tool rows, diffs, mermaid diagrams, tables and HTML cards, restored open at launch, Developer mode on so
frames are timed without the overlay's tick), drives the app with `SKYELAB_VERIFY_ACTIONS` and quits it. It prints:

- launch milestones from stderr: `first frame`, `threads listed` and `thread replayed`, in ms after `main` started
  (debug mode prints them and logs them as `frame` entries);
- frame count, p50, p95, max and how many took over 16 ms, read from the `frame` events (field `ms`) every timed
  frame writes into the `SKYELAB_TRACE` trace;
- for `stream`, the p50 per 10 s of the run too (flat when a delta costs the same at any reply length);
- the busiest views' renders in the run.

| Scenario | What it does |
|---|---|
| `open` | launch on the long thread, wheel-scroll down through it |
| `stream` | send the fake agent's `stream` prompt (1,500 deltas) to the long thread, wait for the turn |
| `search` | type into the sidebar search a key at a time, then clear it |
| `startup` | launch and quit twice on one HOME (cold, then warm), no thread open |
| `idle` | launch and wait: after the caret blink's 5 s timeout it should draw nothing |

`PERF_SAMPLY=1` records a samply profile of the run (with a `.syms.json` sidecar), and
`scripts/profile-top.py <profile.json.gz>` prints the main thread's top functions without the Firefox Profiler
(`SPAN=50-57` limits it to a stretch, in seconds). `PERF_FEATURES=dhat-heap` (or `hotpath`) builds with a profiler.
Results and the numbers before and after each fix are kept in `.agent/perf/`.

## Heap profiles (dhat)

```sh
cargo run -p skyelab-app --release --features dhat-heap
# quit with ⌘Q; the path is printed at start
```

[dhat](https://docs.rs/dhat) counts every allocation and writes `dhat-heap.json` to `<local data>/skyelab/debug/` on
quit. Open it in [DH View](https://nnethercote.github.io/dh_view/dh_view.html): sort by "total bytes" to find churn, by
"at t-end" to find what was never freed. It slows the app down a lot; use it on a short scripted run.

## Hunting a leak

1. Turn on debug mode and open the overlay.
2. Note the **Live objects** counts (or **Dump Debug State** for a file).
3. Do the thing you suspect ten times: open and close a thread, a terminal, a browser tab, connect to and leave a
   machine. `folder watches` goes back to the number of folders still shown (a `watch` line in the log says when
   one starts, stops, or falls back to its top folder over 8,192 folders).
4. Back where you started, the counts should be back too. A count that grows with each round is a leak: something
   still holds a strong `Entity` (a subscription, a task, a map entry). See the "Several windows" and "State and
   updates" rules in [gpui.md](gpui.md).
5. `memory_mb` in the dump is this process's resident memory: it should level off over the rounds, not climb with
   each one. `scripts/perf.sh leaks` runs the rounds for you and prints each dump's counts and memory.
6. **Dump Debug State** also lists this process's child processes (pid, name, run time): an agent or shell left
   running after its thread or terminal closed shows there.

The dump is a JSON file in `<local data>/skyelab/debug/state-<unix ms>.json`:

```json
{
  "skyelab": "0.1.0", "build": "release", "os": "macos", "debug_mode": true,
  "frames": {"fps": 1.2, "last_ms": 3.1, "worst_ms": 9.4},
  "renders_per_second": {"Workspace": 2, "ThreadView": 1},
  "live": {"windows": 1, "thread views": 3, "terminals": 1, "browser tabs": 0, "agent sessions": 1, "remote connections": 0},
  "child_processes": [{"pid": 4242, "name": "claude", "run_secs": 120}],
  "memory_mb": 412,
  "log": [ /* the last 200 debug log entries */ ]
}
```

A new kind of long-lived object gets counted with one field: `_live: skyelab_core::debug::Live::new("things")`.
GPUI's own leak detector (`GPUI_LEAK_DETECTION=1 cargo test …`, with `LEAK_BACKTRACE=1` for where a handle was made)
checks entity handles in tests.

## The debug log

While debug mode is on, SkyeLab logs:

| Kind | What | Fields |
|---|---|---|
| `rpc` | each call this app made to another machine (`client …`, including ones cut off by a lost connection) and each call it served (`daemon …`) | method, request size, round trip, error |
| `process` | each child started through `skyelab_core::process` (git, gh, agents, adb, simctl) and each short-lived one's end, including one killed past its time limit | program, args with secrets removed, pid, time, exit code |
| `tailnet` | Tailscale CLI calls (`tailscale`, the macOS app's `Tailscale`, the fake) | as `process` |
| `mcp` | each tool call an agent made to SkyeLab | tool name, time, ok or error |
| `frame` | a frame over 50 ms | time |

It never logs environment values, prompts, message text, tool inputs or results, tokens or keys. Arguments are
redacted before they are stored: values of flags whose names hold `token`, `secret`, `password`, `key`, `auth`,
`cookie`, `credential`, `header`, `prompt`, `message`, `body` or `title` (and `-m`, `-H`, `-p`, `-F`), token-shaped
words (`ghp_…`, `github_pat_…`, `sk-…`, `xoxb-…`, `AKIA…`, JWTs), URL credentials, query strings and fragments,
`-c`/`--config key=value` values (the key is redacted like any word), the text after `input text` (what an agent types
into an emulator), and any argument over 200 characters. Terminal shells start through their PTY, not
`skyelab_core::process`, so they are not logged; the overlay's child process count and the dump list them. Errors are redacted the same way. `crates/skyelab-core/tests/debug.rs` tests it.

See it in the overlay's **Log** tab (**Open Debug Log** in the palette), or in the file
`<local data>/skyelab/debug/debug.log` (JSON lines, 0600, starts over past 8 MB):

```json
{"at_ms":1764000000000,"kind":"rpc","what":"client thread_events_since","ms":38,"ok":true,"detail":"71 B"}
{"at_ms":1764000000100,"kind":"process","what":"git status --porcelain=v2 -z","ms":12,"ok":true,"detail":"exit 0"}
```

Add an entry from new code with `skyelab_core::debug::record(|| Entry::new(Kind::…, "what").ms(took))`: the closure
runs only while debug mode is on.

## tokio-console

The daemon and the remote client run on tokio. To watch their tasks:

```sh
RUSTFLAGS="--cfg tokio_unstable" cargo run -p skyelab-app --features tokio-console
cargo install tokio-console --locked && tokio-console    # in another terminal
```

`--cfg tokio_unstable` rebuilds everything; keep it to this use.

## Reporting a performance bug

1. Reproduce with debug mode on and the overlay open. Note FPS, worst frame and the busiest views.
2. **Dump Debug State** and attach the JSON.
3. Attach a trace: `SKYELAB_TRACE=trace.json`, reproduce, quit. Say what you did and roughly when.
4. For CPU: a samply profile from `scripts/profile.sh`.
5. Say the build (`release` or `debug`), the OS, the number of windows and threads open.

Check the trace and log for paths you don't want public before you attach them.
