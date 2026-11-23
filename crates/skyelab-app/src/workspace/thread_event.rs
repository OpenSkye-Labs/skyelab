//! What a thread's tab tells the workspace.

use skyelab_store::Thread;
use std::path::PathBuf;

pub enum ThreadViewEvent {
    /// The thread got its store row (the first prompt was sent).
    Started(Box<Thread>),
    /// The agent's Result event arrived: its changes may be on disk now.
    TurnDone,
    /// The thread got a generated title (already stored).
    Retitled(String),
    /// The agent reported its account's usage limits (stored): the Details tab reads them again.
    UsageReported,
    OpenChanges,
    /// The Details tab's Commit & push menu: run it in this thread's Changes tab.
    Git(GitAction),
    /// A file the agent's work names (a card's path, a link): show it in the Changes tab.
    OpenPath(PathBuf),
    /// A reply links to a file (relative to the thread's folder, or absolute): show it in the Preview tab at the line.
    Preview(PathBuf, Option<u32>),
    /// A file a turn changed: the Changes tab scoped to that checkpoint turn, at the file.
    OpenTurnPath(u32, PathBuf),
    /// The folder's files or checkpoints changed (a turn's checkpoint, a branch switch): Changes tabs reload.
    FilesChanged,
    /// Run a project script in this thread's folder, on its account.
    RunScript(Box<skyelab_store::ProjectScript>),
    /// The Details tab unlinked this pull request from the thread: the workspace offers to undo it.
    PrUnlinked(String),
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

/// What the Details tab's Commit & push menu runs (T3's GitActionsControl).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GitAction {
    Commit,
    CommitPush,
    Push,
    /// Fast-forward only.
    Pull,
    OpenPr,
}
