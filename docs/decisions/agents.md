# Decisions: Agents

Part of [decisions.md](../decisions.md). How agent processes launch: env, settings, permissions, providers, output.
Each entry is dated; newest at the bottom.

- **Agent env:** parent env is cleared, then an allowlist passes through. Network and session plumbing (proxy, CA certs,
  TZ, XDG) passes. Anything that can pick an account, provider or base URL does not. (2026-11-23)
- **Project settings:** agents launch with `--setting-sources user --strict-mcp-config`, so a cloned repo's
  `.claude/settings*.json` and `.mcp.json` can't redirect credentials or run hooks. (2026-11-23)
- **Codex provider:** `codex app-server` under `CODEX_HOME`, the folder's account picks the provider. Protocol,
  event mapping, hardening and limits: [codex.md](../codex.md). (2026-11-23)
- **One env allowlist** (`skyelab_core::env`) for agents, project terminals and every git/gh call SkyeLab runs. A
  terminal gets the agent list plus harmless shell vars (`EDITOR`, `PAGER`, colours, display, tool homes such as
  `CARGO_HOME`) and keeps relative PATH entries; no credential, provider or base-URL var (`ANTHROPIC_*`,
  `CLAUDE_CODE_*`, `OPENAI_*`, `CODEX_*`, `GH_*`/`GITHUB_TOKEN`, `AWS_*`, `GOOGLE_*`, `CLOUD_ML_*`, `GIT_*`) is ever in
  it. A terminal always names both provider homes: the bound account's, and an empty private dir under
  `<data dir>/skyelab/unbound` for the other one (and for `GH_CONFIG_DIR` when the folder is unbound), so the CLI
  asks to log in rather than using the default account. Those dirs are per account (`unbound/account-<hex id>/`),
  and one more set for unbound folders, so a login made in one account's terminal never answers in another's;
  agents never use them. git's https credentials come only from the account's gh (no helper without one), as for
  agents. Limit: a login shell's rc files may export credentials again. (2026-11-23)
- **Agent git credentials and ssh:** agents get `GIT_CONFIG_COUNT`/`KEY_n`/`VALUE_n` that reset `credential.helper`
  and add `gh auth git-credential` (under the account's `GH_CONFIG_DIR`; no helper at all without one, so nothing
  answers), and clear `http.extraHeader`, generically and for every URL-scoped `http.<url>.extraHeader` git config
  holds at launch. `SSH_AUTH_SOCK` no longer reaches agents, and `core.sshCommand` fails with a message: an ssh key
  isn't bound to any account, so an ssh push would land as whoever owns the key. An account opts in with
  `ssh_agent = true` (accounts.toml only, not the form). Terminals keep `SSH_AUTH_SOCK`: a terminal is the user's own
  shell, so their ssh keys are theirs to use there; https pushes from it still go through the account's gh when it
  has one. This stops accidents, not a hostile agent, which can still read `~/.ssh` (HOME passes). (2026-11-23)
- **Accounts as saved, for every action.** Agent launches, terminals and the Changes tab's Commit, Push and Open PR
  read accounts.toml when they act (an unreadable file binds nothing), not the window's copy. The launch re-check
  compares the login (`Account::same_login`: id, provider, config dir, gh dir, ssh), not the label. (2026-11-23)
- **Repo instructions still load:** checked against the real CLI (2.1.330), `--setting-sources user --strict-mcp-config`
  still reads the project's `CLAUDE.md`, so blocking project *settings* does not drop project *instructions*. (2026-11-23)
- **Agent permissions through the host.** Claude runs with `--permission-prompt-tool stdio` (the Agent SDK's switch),
  so every permission ask reaches SkyeLab as a stream-json `can_use_tool` control request and the CLI waits for our
  `control_response`; a `control_cancel_request` withdraws it. We picked this over `--permission-prompt-tool` with
  our own MCP server because the ask, its answer and its withdrawal stay on the session's own pipes, ordered with
  its events, and no token or socket is involved. "Allow for this thread" sends only the CLI's own
  `addRules` allow suggestions with every destination forced to `session` (suggestions that change the mode, add
  directories or replace rules are dropped); with none, a rule for that exact command, or for the tool when it has no
  command. Nothing is written to the project's or the user's settings; it lasts while that process runs. Ask ids are
  a per-process nonce plus the provider's id, and every ask still open when stdout ends is withdrawn, so a stored ask
  never answers a later process's request. Codex's command and
  file-change approvals go through the same `Event::Permission`, answered `accept` / `acceptForSession` /
  `decline`. Asks and answers are stored as thread events (`Permission`, `PermissionResolved`), so remote viewers see
  them through the pushes they already get, and answer with `answer_permission` (protocol 1.3), served only after
  hello from an approved device. The answer's reply waits for the session's own check (the id must still be open).
  Modes: Ask (`--permission-mode default`, always named so an account's `defaultMode` setting can't loosen it;
  2.1.330 accepts `default` though `--help` lists `manual`; Codex `untrusted`), Plan (`plan`; Codex read-only), Accept
  edits (`acceptEdits`; Codex `untrusted`, since workspace-write already lets it edit), Full access (`bypassPermissions` + `--allow-dangerously-skip-permissions`; Codex `never` +
  `danger-full-access`), behind a confirm. Limits: checked against claude 2.1.330's `--help` and binary only (the
  control protocol isn't in `--help`); a running Claude session takes Full access only if it started in it, so
  `Session::set_mode` refuses that switch and the choice applies at the next start; `AskUserQuestion` has no answer UI. (2026-11-23)
- **Agent stderr is captured.** The agent's stderr is a third streaming pipe (still copied to the app's stderr); a
  thread keeps its newest 64 KiB per session. Nothing waits on its EOF. After a failed turn, or a nonzero exit that
  wasn't a Stop, what is new is stored as an `Event::Stderr` row and shown as a collapsed **Agent output** row.
  (2026-11-23)
- **A turn's cost is its growth over Claude's running total.** Claude's `result.total_cost_usd` (and `modelUsage`) is a
  running total for the process, and since Claude Code 2.1.277 a resumed session starts from the session's restored
  total; `usage` is the turn's own (main loop only, no subagents). Evidence: the Agent SDK cost-tracking docs ("Track
  costs in streaming input mode": `usage` "covers only that turn", the totals "carry the running total for the whole
  call so far, plus any spend restored when the call resumed a session"), and T3 Code's recorded `multi_turn`
  transcript, where turn 2's $0.0154371 is turn 1's $0.01171425 plus turn 2's own usage priced at $0.00372285.
  `skyelab_agent::turn_cost` takes the growth over the thread's previous non-zero total; a total that dropped (an older
  CLI's resume, a fresh session) counts whole; zero (an aborted turn, a crash) counts nothing and is no baseline. An
  imported turn recorded no cost, so after one the baseline is unknown and the next turn counts nothing: a resumed
  imported session's first total includes its spend from before the import. It never overstates, and undercounts in
  three cases: that first turn after an import; a fork's or a session-gone rerun's first turn (a fresh session, still
  diffed against the copied or earlier total) when it spends more than that total; and an older CLI's resumed first
  turn that exceeds the old total. The timeline
  divider and the store trigger (migration 17, which also recomputed stored `turn_usage` rows, from the events or, for
  a purged thread, from the stored totals) apply the same rule. Codex's `thread/tokenUsage/updated` `total` is
  thread-wide and `last` the newest model call, so a turn is its `total` growth, based at `total - last` when the
  process has no earlier reading (a resumed thread). Stored Codex rows can't be checked (only the turn's difference was
  kept), so they are left as they are. (2026-11-24)
