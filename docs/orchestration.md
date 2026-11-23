# Agent orchestration

Part of [architecture.md](architecture.md).

An agent can delegate work to other threads through SkyeLab's own MCP tools: `list_projects`, `start_thread`,
`thread_status`, `read_thread`, `send_message` and `wait_thread`. It can also drive a browser tab of its own with
`browser_open`, `browser_navigate`, `browser_url` and `browser_eval`.

- **Account rule.** A delegated thread runs under its target project's bound account, through the normal launch
  path, pinned to the account the start was approved for. An agent under account A can only list and start in
  projects bound to A. It can read, wait on and message only threads it delegated itself (its descendants), recorded
  under A, and only while they run: a message never restarts a thread that ended or that the user stopped. An unbound
  target is refused; there is never a default account. This keeps a work agent from spending the personal plan, and
  the reverse, and keeps an agent out of the user's other threads.
- **Limits.** Depth 2 at most (a user's thread is depth 0), and 4 children starting or running per thread (a child
  that finished its turn frees its slot). The children limit is claimed in the launch itself, under the store lock
  together with the status write (`child_slots::claim`): on every new child row, every resume of a stopped child,
  and every follow-up that gives an idle child a new turn, whatever sent the prompt (the parent, another device,
  the user). A child counts as busy until the result of its last queued prompt (follow-ups sent mid-turn are
  counted), not the first result after them. A refused resume leaves the child as it was and its tab says why (the
  refused prompt still shows in the tab, though it was never stored or sent); a Send from another device has
  already been answered Ok by then. Rows left `running` by a crash still count until they are opened and end.
- **Remote mode.** A delegated child (or a thread another device starts here) that opens while the window shows
  another machine is kept out of the dock: the sidebar gets a "This Mac · N new threads here" row, and switching
  back adds the tabs. Its agent runs meanwhile, and delegated sends and Stop still reach it.
- **Transport.** `skyelab-agent::mcp` listens on a unix socket in `<data dir>/skyelab/run/<pid>/` (0700). Each agent
  session gets a grant: a one-use random token and a 0600 `--mcp-config` file that runs `skyelab --mcp-proxy`. The
  proxy's first connection consumes the token and deletes the file. Ending the session revokes the grant and cuts the
  connection. Never TCP.
- **Browser tools.** A thread reaches only the tab it opened (`BrowserPanel::agent` names its thread); the user's
  tabs, another thread's and one the user took over are never matched, and there is no fallback. Calls run on the UI
  thread through `skyelab-browser`'s agent path: `parse_agent_url` (http/https), `check_agent_eval` and the in-page
  guard. Limits: one tab per thread (`browser_open` reuses it), URL 2048 chars, expression 32 000 chars, eval 10 s,
  result 64 KiB (a bigger one is an error, never cut JSON; a thrown message is cut). Each agent tab has its own
  non-persistent WebKit data store (`with_incognito`), so it shares no cookies or storage with the user's tabs (on the
  default persistent store) or other threads' tabs. Open and navigate answer once the new page commits. Take over
  reloads the page, ending the agent's scripts, and an eval still running answers with an error.
- **UI.** A child's header says "Delegated by <parent>"; a parent lists its delegated threads. The store records
  `parent_thread_id` (schema 3).
- **Codex** sessions get the same tools and grant; see [codex.md](codex.md).
- **Not yet:** Windows (no named-pipe transport, so threads there run without the tools) and delegation to other
  machines.

