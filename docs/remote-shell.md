# The shell stream (protocol 1.7)

Part of [remote.md](remote.md). A remote or linked machine's sidebar stays current without polling, like T3's shell
snapshot and events:

- `subscribe_shell` answers `Reply::Shell { seq, projects, threads }`: every listed project and thread (removed
  projects, deleted threads and side chats left out, archived ones in), threads newest first. Thread rows carry the
  serving machine's whole row (`ThreadRow`, flattened, each field left out when empty: cwd, permission mode, session
  id, worktree path/branch/base, PR url/state, parent, pinned/archived time, unread, fork, compare group, scheduled
  task, limit resume), so a remote sidebar can draw what that machine's own draws.
- After the reply, that connection gets `ServerMessage::Shell { seq, delta }` pushes, seq counting up by one:
  `project_updated` (a project listed again brings its threads as their own updates), `project_removed` (its
  threads go too), `thread_updated` (the whole row as it is now, so applying one twice changes nothing) and
  `thread_removed`. `ShellSnapshot::apply` is the shared reducer.
- Source of the changes: the store's row hook (`Store::on_rows_changed`, SQLite's update hook), which fires on every
  insert, update and delete of a `projects` or `threads` row whatever method wrote it. A worker in `StoreBackend`
  (started by the first subscribe) batches writes 100 ms apart, re-reads those rows and numbers the deltas; a
  streaming turn bumps its row on every event, so a busy thread sends at most about ten updates a second. This
  machine's own `LocalSource` subscribes to the same worker in-process.
- The client checks the seqs. A missed one, a dropped connection, or a connection still more than 1,024 changes
  behind when the next batch comes (the daemon closes it; one batch may be bigger, a project's update carries all
  its threads) ends the stream; the app subscribes again on the next approved connection, and the new snapshot
  replays whatever changed meanwhile. A subscribe that fails or times out while connected is tried again after 3 s. Only approved connections past hello can subscribe; a revoke closes
  the connection and its stream with it.
- A 1.6 machine is never sent `subscribe_shell` (`Client::subscribe_shell` fails it locally, "update SkyeLab on
  <machine>"): the app lists it per project and lists again a second after pushed events, as before. A 1.6 client of
  a 1.7 machine never subscribes and never gets a shell message.
- Bounds and failures: on the wire a snapshot stays under 8 MiB (half the client's 16 MiB message limit). A store
  with more sends the newest threads that fit, archived ones left out first, with `truncated: true`; a left-out
  thread comes back as an update when it next changes. This machine's in-process snapshot is never cut. A changed row
  the worker can't read ends every stream (each client resubscribes) rather than skip a change with no gap to show.
  When two subscribes cross, the newest one keeps the stream and an older reply's changes end at once. The
  1.7 check runs on the connection the call goes out on, so a reconnect to an older machine is refused locally.
- This machine's own sidebar follows the same stream in-process (from the app's backend, set in `main.rs`): a thread
  another writer adds (a remote call, an agent, another window) shows, and a deleted one goes, without a reload. A
  row the window already holds is never replaced from the stream, since the window edits its rows before their
  store writes land and a streamed row read in between would be older.

## Cached shell snapshots

A linked or switched-to machine's rows show at launch, before its `wss://` handshake finishes, from the last ones
saved (`skyelab_env::cache`, app glue in `environment_cache.rs`):

- One file per MagicDNS name in `<cache dir>/<APP_DIR>/remote-shells/` (`~/Library/Caches/skyelab-dev/...` for a
  debug build): `<sha256 of the lowercase name>.json`, so a name never becomes a path. The dir is 0700, the file
  0600, written through a temp file and a rename. The payload is `{schema: 1, host, node_id, saved_at_ms, projects,
  threads}`.
- Read at `RemoteEnv::new`, off the UI thread, only when the name has a pinned node (`environment_nodes` in
  `settings.toml`) and the file names both that host and that node. A missing, broken, other-schema or over-8 MiB
  file is no cache, never an error. The rows show with the connecting state; every action stays refused until the
  connection is `Connected`, as with any rows of a machine not connected. A linked machine shows them in This
  Mac's list next to its "Connecting" row (also while reconnecting).
- Live rows always win: the first snapshot, delta or 1.6 listing marks the rows live, and a cache read finishing
  later is dropped. Live rows are saved 2 s after a change (once however many come), only while `Connected` on the
  pinned node.
- Deleted from the window and the disk on every refusal (`Refused`, `Certificate`, `NotYourDevice`,
  `IdentityMismatch`, `NodeChanged`, `Unverified`, `Denied`), on Forget and on unlinking. A per-name counter bumped
  immediately on delete drops a save or read that began before it, while a background I/O lock serializes writes and
  deletes. Tests set no `ShellCacheDir` global unless they pass a temp dir, so they never touch the real cache.
