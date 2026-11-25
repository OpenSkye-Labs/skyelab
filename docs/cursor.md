# Cursor provider

An account has a `provider`: `claude` (the default, so files from before Codex/Cursor keep working), `codex`, or
`cursor`. The folder's binding decides the account and so the provider; nothing else picks it. A Cursor account's
config dir is its `CURSOR_CONFIG_DIR`, which `launch_env` sets in place of `CLAUDE_CONFIG_DIR` or `CODEX_HOME`.

## Binary lookup

Cursor ships its CLI as `cursor-agent` (and newer installs also install it as `agent`). When launching a Cursor
account with the default binary name `"cursor-agent"`, SkyeLab searches absolute entries of `PATH` for `cursor-agent`
first and falls back to `agent`. Explicit absolute paths (such as `SKYELAB_CURSOR_BIN` in debug builds) resolve
strictly with no fallback.

## Protocol

Cursor runs non-interactively in print mode, one process per turn:

```text
cursor-agent -p --output-format stream-json --stream-partial-output --workspace <dir> \
  [--model <id>] [--mode plan | --force] [--resume <session_id>]
```

The prompt (plus any attached text files, wrapped in `<attached_file name=...>`) is written to stdin and stdin is
closed (EOF) immediately, so prompt text never appears on the command line. Resuming a thread passes `--resume
<session_id>`. Interrupting a running turn sends `SIGINT` to the process group on Unix (or terminates the job/child on
Windows) and reports an interrupted result. Queued follow-up messages wait for the current turn's process to end and
automatically resume the thread with the next queued prompt.

## Events

Cursor's NDJSON stream maps onto `skyelab_agent::Event`, so the thread view, store, timeline, and remote protocol need
no provider-specific schema:

| Cursor stream-json | Event |
|---|---|
| `system` (`subtype: "init"`) | `Init { session_id, model }` |
| `thinking` (`subtype: "delta"`) | `Delta { kind: Thinking, text }` (buffered) |
| `thinking` (`subtype: "completed"`) | `Assistant { content: [Thinking] }` |
| `tool_call` (`subtype: "started"`) | `Assistant { content: [ToolUse] }` (both nested `*ToolCall` and flat `tool_call_id`/`tool_name`/`arguments`) |
| `tool_call` (`subtype: "completed" \| "failed"`) | `User { content: [ToolResult] }` |
| `assistant` with `timestamp_ms` and text blocks | `Delta { kind: Text, text }` (buffered) |
| `assistant` (final / un-timestamped or non-text) | `Assistant { content }` (clears text delta buffer) |
| `user` | dropped (prompt is already shown and stored) |
| `result` | `SessionGone` (if resuming and session missing) + `Result` (duration, usage, error/interrupted) |
| anything else / malformed JSON | `Unknown` |

Nested Cursor tool keys normalize to SkyeLab's tool card names: `shellToolCall` → `Bash`, `readToolCall` → `Read`,
`writeToolCall` → `Write`, `editToolCall` → `Edit`, `deleteToolCall` → `Delete`, `lsToolCall` → `LS`,
`grepToolCall` → `Grep`, `globToolCall` → `Glob`, `mcpToolCall` → `mcp__<server>__<tool>`.

## Hardening & bounded parsing

- **Environment isolation**: Every Cursor subprocess starts from `env_clear()` plus the allowlist in
  `skyelab_core::env` plus the bound account's `CURSOR_CONFIG_DIR` (and `GH_CONFIG_DIR` / git credential env when
  configured). Parent `CURSOR_CONFIG_DIR`, `CURSOR_API_KEY`, `CURSOR_AUTH_TOKEN`, `CURSOR_BACKEND_URL`,
  `CLAUDE_CONFIG_DIR`, and `CODEX_HOME` are never inherited.
- **Permission modes**: `Mode::Plan` passes `--mode plan`; `Mode::Bypass` passes `--force`. `Mode::Default` and
  `Mode::AcceptEdits` both omit `--force` so `AcceptEdits` never silently escalates to full access.
- **Bounded stream caps**:
  - `MAX_LINE`: 16 MiB per stdout line (`pipes::read_events` truncates oversized lines to `Event::Unknown`).
  - `MAX_BLOCKS_PER_LINE`: at most 64 content blocks parsed per assistant message.
  - `MAX_ACCUM_BYTES`: at most 8 MiB buffered for in-flight thinking or text deltas.
  - `MAX_OPEN_TOOLS`: at most 1,024 tracked in-flight tool call ids (oldest evicted).
  - `MAX_SESSION_EVENTS`: at most 100,000 events emitted per turn, after which one error `Event::Result` is emitted
    and further output is suppressed until EOF.
  - `list_cursor_models` (`cursor-agent models`): 10 s deadline, 1 MiB stdout cap, 256 models cap, and `valid_name`
    filtering (rejecting flag-like or invalid ids).
  - `account_default_model` (`<CURSOR_CONFIG_DIR>/cli-config.json`): regular-file check (`is_file()`), 256 KiB cap,
    and `valid_name` validation.

## OS keychain caveat

SkyeLab isolates every Cursor subprocess by passing its own `CURSOR_CONFIG_DIR` and stripping parent auth environment
variables (`CURSOR_API_KEY`, `CURSOR_AUTH_TOKEN`, `CURSOR_BACKEND_URL`). However, on macOS (and desktops with an OS
secret service), `cursor-agent` itself may store or read OAuth credentials in the shared OS keychain rather than
exclusively inside `CURSOR_CONFIG_DIR`. When running multiple Cursor accounts on macOS, check `cursor-agent status`
or use per-account API configuration inside each `CURSOR_CONFIG_DIR` if Cursor shares keychain tokens across folders.

## Known limits

- **One process per turn**: `cursor-agent -p` exits after each turn. Mid-turn steering and interactive per-tool
  permission prompts are not available on Cursor threads; queued prompts automatically resume on the next turn.
- **Attachments**: Cursor threads accept UTF-8 text attachments (inlined on stdin) and refuse images and PDFs.
- **Usage limits**: Cursor CLI does not expose a rate-limit polling endpoint; `usage_limits` returns `Unavailable`.
- **Session import**: Cursor stores chats in SQLite under `chats/`; SkyeLab's JSONL transcript importer currently
  returns no importable sessions for Cursor.
- **Cost**: Cost is not reported (`total_cost_usd` is 0).
- **CLI update check**: Cursor CLI is not distributed as an npm package, so onboarding does not query npm for Cursor
  CLI updates.
- **Terminal isolation**: Project terminals in non-Cursor or unbound folders point `CURSOR_CONFIG_DIR` at an empty
  private directory so `cursor-agent` typed there does not silently fall back to `~/.cursor`.
