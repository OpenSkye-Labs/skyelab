# Security model

- **Account invariant.** A folder resolves to its account or to nothing. There is never a default account. An
  unknown account id in `accounts.toml` is a load error.
- **Clean environment, one allowlist.** Each agent starts with an empty environment. A short allowlist passes
  through (`PATH`, `HOME`, locale, proxy and CA settings, and the like), and then SkyeLab sets `CLAUDE_CONFIG_DIR`
  or `CODEX_HOME` for the bound account. Nothing that could pick another account, provider or API base URL
  (`ANTHROPIC_*`, `CLAUDE_CODE_*`, `OPENAI_*`, `CODEX_*`, `GH_TOKEN`, `AWS_*`, the parent's own homes) is inherited.
  Project terminals and every git/gh call SkyeLab runs use the same list (terminals add shell vars such as
  `EDITOR`, `PAGER` and `SSH_AUTH_SOCK`). An agent's own `git push` asks only the account's `gh` for https
  credentials, and gets no ssh-agent unless the account sets `ssh_agent = true`.
- **Project settings don't load.** Agents run with `--setting-sources user --strict-mcp-config`, so a cloned repo's
  `.claude/settings*.json` and `.mcp.json` can't redirect credentials or run hooks. The MCP servers you added to the
  bound account itself (Claude's user scope, Codex's `config.toml`) do load, and ask before each tool call; the thread
  details card lists them with their state, and **Restart Agent Session** reloads them. Codex runs with the folder and
  every folder above it (and a worktree's main repo) marked untrusted, so a repo's `.codex/config.toml`, hooks and
  `.rules` never load. Codex keeps its `workspace-write` sandbox (writes stay in the folder, no network) unless a
  thread picks Plan (read-only) or Full access, and its approval requests come to the thread.
- **Approvals stay with you.** An agent's permission ask waits in its thread until you answer it there, or from
  a device you approved for sharing. "Allow for this thread" lasts while that agent runs, and is never written to
  the project's or your Claude settings. Full access asks before it turns on.
- **Delegation stays on one account.** An agent can start threads only in projects bound to its own account, and can
  read or message only the threads it started. Its tools reach SkyeLab over a private socket with a one-use token.
- **Tailnet trust.** Sharing listens only on this machine's tailnet addresses and serves only TLS 1.3 with the
  machine's own Tailscale certificate. Each connection must come from one of your own, untagged devices (checked
  with `tailscale whois`), and carry a device key you approved on this machine for that device (kept in the
  macOS keychain by release builds on a Mac). Browser pages (any `Origin` header) are refused. A connected device can list and drive
  threads, and forward ports from this machine's loopback (1024 and up, not SkyeLab's own port or a denylist of
  services such as Docker's API or debuggers), each tunnel through the same gate and only while that device's
  connection asked for it. It can't add projects, change accounts or bindings, or open terminals. The client checks the
  certificate against the exact MagicDNS name it dialled, and the desktop app also asks its own Tailscale who it
  dialled: only your own untagged devices, and the same Tailscale node it saw for that name before. The phone
  remembers the node too, but only from what the machine says about itself (it has no Tailscale CLI to ask).

The full rules are in [docs/remote.md](remote.md) and [docs/decisions.md](decisions.md).
