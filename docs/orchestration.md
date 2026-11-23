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
  counted), not the first result after them. A result ends the turns of the prompts it answered: claude names them
  in `user_message_uuids` (a follow-up it folded into the running turn ends with it, a background wake ends none),
  and an older CLI without the list answers one; a Codex thread that fails to open answers every queued prompt. An
  agent that exits ends its row within about a second even if a tool it started still holds its stdout, and that
  tool (in its process group) is ended with it. A refused resume leaves the child as it was and its page says why (the
  refused prompt still shows there, though it was never stored or sent); a Send from another device has
  already been answered Ok by then, but a delegated `send_message` answers only after the child's slot is claimed,
  so one that would pass the cap is refused to the parent. A row counts only while its agent is live: ending, Stop, a failed start, closing the thread (even while it
  starts) and quitting all write a non-busy status, and opening the store at app start marks rows the last run left
  `starting` or `running` as `interrupted`.
- **Starts.** `start_thread` answers with the child's `thread_id` as soon as its row exists, before its agent is up
  (`thread_status` then says `starting`, `running` or `failed`), so a slow start (a big worktree) can't outlast the
  agent's 30 s wait and leave a child it never heard of. It takes an optional `request_id` (at most 128 chars): a
  retry with the same one from the same thread returns the same child, whether the first call is still starting or
  done, and even after the child was closed; the first call's project and prompt win. The ids live in memory while the
  calling thread is open. A start without one can still outlast the wait while a big worktree is checked out (the
  row is saved after the checkout).
- **Live parent.** Starts and sends run only while the calling thread's agent is live in this window: a parent the
  user stopped, or that was closed, is refused, even for a call it queued before.
- **Current binding.** A launch re-reads accounts.toml right before it spawns the agent and checks the folder
  against that, so a rebind saved while the launch was under way (a worktree being made) is seen. An unreadable file
  binds nothing, and the launch is refused.
- **Background starts.** A delegated child (or a thread another device starts here) never takes the page: its
  sidebar row (and its project's row) gets an unread dot until the user opens it. While the window shows another
  machine, the sidebar gets a "This Mac · N new threads here" row that switches back. Its agent runs meanwhile, and
  delegated sends and Stop still reach it.
- **Transport.** `skyelab-agent::mcp` listens on a unix socket in `<data dir>/skyelab/run/<pid>/` (0700). Each agent
  session gets a grant: a one-use random token and a 0600 `--mcp-config` file that runs `skyelab --mcp-proxy`. The
  proxy's first connection consumes the token and deletes the file. Ending the session revokes the grant and cuts the
  connection. Never TCP. The run dir is created 0700 (and chmod'ed again, past the umask), the token line must
  arrive whole within 5 s of connecting, and an accept error backs off 100 ms instead of spinning.
  A `wait_thread` stops polling, with an error, as soon as the caller's grant is revoked or its row is no longer
  live (stopped, exited, or the thread closed). Browser calls from a thread that isn't running here any more are refused like start and
  send.
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

