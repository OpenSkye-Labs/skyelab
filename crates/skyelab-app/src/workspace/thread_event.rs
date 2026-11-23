//! What a thread's tab tells the workspace.

use skyelab_store::Thread;
use std::path::PathBuf;

pub enum ThreadViewEvent {
    /// The thread got its store row (the first prompt was sent).
    Started(Box<Thread>),
    /// The agent's Result event arrived: its changes may be on disk now.
    TurnDone,
    OpenChanges,
    /// The folder's files or checkpoints changed (a turn's checkpoint, a branch switch): Changes tabs reload.
    FilesChanged,
    /// Run a project script in this thread's folder, on its account.
    RunScript(Box<skyelab_store::ProjectScript>),
    /// The project's scripts were edited.
    ScriptsChanged,
    /// The thread got a new worktree: the project's setup scripts run there.
    WorktreeCreated {
        cwd: PathBuf,
        account_id: String,
    },
    /// A terminal in the thread's worktree, under the thread's account.
    OpenTerminal {
        cwd: PathBuf,
        account_id: String,
    },
}
