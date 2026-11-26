//! The shell stream's types (1.7): a snapshot of the listed projects and threads, and the changes after it.

use serde::{Deserialize, Serialize};

use crate::{Project, Thread};

/// Every listed project and thread at `seq` (1.7): projects by name, threads newest first.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShellSnapshot {
    /// The last change this snapshot includes. The next pushed change is `seq + 1`.
    pub seq: u64,
    pub projects: Vec<Project>,
    pub threads: Vec<Thread>,
    /// Some threads were left out so the snapshot fits one message: the oldest, archived ones first. Each comes
    /// back as an update when it next changes.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub truncated: bool,
}

/// One change after a snapshot (1.7). seqs count up by one per connection's stream; a gap means changes were
/// missed, and the client subscribes again for a new snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShellChange {
    pub seq: u64,
    pub delta: ShellDelta,
}

/// What changed. An update carries the whole row as it is now, so applying one twice changes nothing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "data", rename_all = "snake_case")]
pub enum ShellDelta {
    /// Added, changed, or listed again. Its listed threads follow as their own updates.
    ProjectUpdated(Project),
    /// No longer listed; its threads go with it.
    ProjectRemoved {
        id: i64,
    },
    ThreadUpdated(Box<Thread>),
    /// Deleted, or no longer listed.
    ThreadRemoved {
        id: i64,
    },
}

impl ShellSnapshot {
    /// Applies one change in place, keeping the snapshot's order. The client's reducer: the same rows the serving
    /// machine lists, without listing them again.
    pub fn apply(&mut self, delta: ShellDelta) {
        match delta {
            ShellDelta::ProjectUpdated(p) => {
                self.projects.retain(|q| q.id != p.id);
                self.projects.push(p);
                self.projects
                    .sort_by(|a, b| a.name.cmp(&b.name).then(a.id.cmp(&b.id)));
            }
            ShellDelta::ProjectRemoved { id } => {
                self.projects.retain(|p| p.id != id);
                self.threads.retain(|t| t.project_id != id);
            }
            ShellDelta::ThreadUpdated(t) => {
                self.threads.retain(|u| u.id != t.id);
                self.threads.push(*t);
                self.threads
                    .sort_by_key(|t| (std::cmp::Reverse(t.updated_at), std::cmp::Reverse(t.id)));
            }
            ShellDelta::ThreadRemoved { id } => self.threads.retain(|t| t.id != id),
        }
    }
}
