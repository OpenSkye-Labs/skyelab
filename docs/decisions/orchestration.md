# Decisions: Orchestration

Part of [decisions.md](../decisions.md). SkyeLab's own MCP tools and their transport. Each entry is dated; newest at
the bottom.

- **Orchestration tools are ours, over a private socket (a named pipe on Windows).** No `rmcp` (four JSON-RPC
  methods are enough). Agents get `--mcp-config <grant file> --allowedTools mcp__skyelab`, and `--strict-mcp-config` keeps
  every other server out (since 2026-11-26 the account's own servers and claude.ai connectors load too, see
  [agents.md](agents.md); only `mcp__skyelab` is pre-allowed). Starts and sends run on the UI thread through `ThreadView`, which re-checks the account rule
  with the config the launch uses. (2026-11-23)
- **MCP socket path:** `<data dir>/skyelab/run/<pid>/mcp.sock` can exceed sun_path (104 bytes on macOS) with a long
  HOME. Then the socket goes in a fresh dir under the temp dir (else `/tmp`), created exclusively with mode 0700 and
  checked to be a real dir owned like the run dir; grant files stay in the run dir. Removed when the server drops; a
  crash leaves it for the OS's temp cleanup. (2026-11-23)
- **Windows transport: a named pipe only this user can open (2026-11-23).** `\\.\pipe\skyelab-<pid>-<64 random
  hex>`, created as the first instance (a squatted name fails the start), local clients only, and a protected DACL
  with one entry: full access for the current user's SID (no Administrators, SYSTEM or Everyone's default read).
  Then the same one-use token and grant checks as on unix. tokio's overlapped pipes, blocked on from the server's
  plain threads: a synchronous pipe handle serializes its I/O, so a blocked read would stall the other thread's write.
  tokio takes the descriptor only as a raw `SECURITY_ATTRIBUTES` pointer and the tree has no safe ACL crate, so
  `skyelab-pipe-sys` holds the unsafe calls (the user's SID, the SDDL conversion, and the pid check for stale runs).
  `mcp/transport/{unix,windows}.rs` are the only per-OS code; server and proxy are shared. The proxy checks that the
  pipe's server is the process its name names (`GetNamedPipeServerProcessId`), so once that SkyeLab is gone another
  user who re-creates the freed name gets no token. Grant files sit in the run dir under the local (never roaming,
  which can live on a file share) app data dir, private through its inherited ACL. Limit: compile-checked only
  (`scripts/check-windows.sh`), not run on Windows.
- **A lost tool connection is an error, not a reconnect (2026-11-23).** The server lives as long as the app and its
  grants only in its memory, so it "restarts" only with the app, and then the old token is spent (its file deleted)
  and the new run has no live view of the thread to answer for (every call would be refused as gone). Persisting
  tokens to re-grant would widen what a leaked file allows. So once SkyeLab's end closes while the agent still talks,
  the proxy answers every request in flight and after it itself: a tool call with an `isError` text, anything else
  with JSON-RPC error -32000, saying the tools are gone for this session and to start the thread again from SkyeLab.
  The proxy also no longer needs a half-close (pipes have none): it returns once its input ended and every request
  it forwarded has an answer.
