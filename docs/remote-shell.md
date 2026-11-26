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
