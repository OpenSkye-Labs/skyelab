# Codex provider

An account has a `provider`: `claude` (the default, so files from before Codex keep working) or `codex`. The
folder's binding decides the account and so the provider; nothing else picks it. A Codex account's config dir is its
`CODEX_HOME`, which `launch_env` sets in place of `CLAUDE_CONFIG_DIR`.

## Protocol

Codex runs as `codex app-server`: JSON-RPC over stdio, one message per line, without the `"jsonrpc"` field. One
process holds the thread: `initialize`, then `thread/start` (or `thread/resume {threadId}` for an old thread), a
`turn/start` per prompt (prompts queue behind a running turn, as with `claude`) and `turn/interrupt`. Not
`codex exec --json`: that is one process per turn, so it can't interrupt or take a follow-up without a new process.

## Events

Items map onto `skyelab_agent::Event`, so the thread view, store and remote protocol need nothing new:

| Codex | Event |
|---|---|
| `thread/start` / `thread/resume` response | `Init { session_id: thread id, model }` |
| `item/completed` agentMessage | `Assistant` text |
| `item/started` commandExecution, fileChange, mcpToolCall, webSearch | `Assistant` `ToolUse` (`shell`, `apply_patch`, `mcp__<server>__<tool>`, `web_search`) |
| `item/completed` for those | `User` `ToolResult` |
| `turn/completed` | `Result` (error unless `completed`; result is the last agent text or the error) |
| deltas, token usage, status notifications | dropped |
| anything else | `Unknown` |

## Hardening

Same as Claude's: the env allowlist plus `CODEX_HOME` (so the parent's `CODEX_HOME`, `CODEX_API_KEY`,
`CODEX_ACCESS_TOKEN` and `OPENAI_*` never pass), the binary resolved from absolute PATH entries, process group kill.

A repo's `.codex/config.toml`, hooks and `.rules` load only for trusted projects, and trust comes from the account's
own config. SkyeLab passes `-c projects={...}` marking the folder, every ancestor and (for a git worktree) its main
repo and that repo's ancestors untrusted. Codex merges `-c` overrides into the user's `projects` table, so naming only
the folder isn't enough: a subfolder of a trusted repo still loaded the repo's layer in a spike against codex 0.171.

There is no approval UI, so threads run with `approvalPolicy: "never"` and `sandbox: "workspace-write"` (writes stay
in the folder, commands get no network). Any approval request that still arrives is declined; other server requests
get a JSON-RPC error.

## Orchestration

A Codex session gets SkyeLab's MCP tools from the same grant as `claude`: its command, args and env (socket, one-use
token) are read from the grant file and sent in `thread/start`'s `config.mcp_servers.skyelab`, over stdin, so the
token is never on a command line or in codex's own env (the agent's shell). The same-account rule is unchanged.

## Limits

- The account's own `CODEX_HOME/config.toml` (MCP servers, hooks, model) still loads; it is the user's, like Claude's
  user settings. Project skills and `AGENTS.md` still load, like `CLAUDE.md`.
- Cost isn't reported (`total_cost_usd` is 0).
- Remote clients see the account, not the provider.
- Windows: no MCP tools, as for Claude.
