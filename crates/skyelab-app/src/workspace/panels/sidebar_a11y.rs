//! The accessible names of sidebar rows: what the row shows, read as one line ("title, project, account, status").

use super::sidebar::{RowAccount, RowMarks, SidebarItem};
use crate::ui;

/// The account part: its name, "No account" for an unbound folder, nothing when the account is hidden.
fn account(item: &SidebarItem) -> Option<String> {
    match &item.account {
        Some(RowAccount::Bound(acc)) => Some(acc.label.clone()),
        Some(RowAccount::Unbound) => Some("No account".to_owned()),
        Some(RowAccount::Hidden) | None => None,
    }
}

/// A thread row: title, project, account, then the one status the row shows (a question, a badge, or done), branch.
pub(super) fn thread_label(
    item: &SidebarItem,
    project: &str,
    marks: &RowMarks,
    unread: bool,
) -> String {
    let mut parts = vec![item.label.to_string()];
    if !project.is_empty() {
        parts.push(project.to_owned());
    }
    parts.extend(account(item));
    if let Some(ask) = marks.ask {
        parts.push(ask.label().to_owned());
    } else if let Some((_, word)) = item.status.and_then(ui::Status::badge) {
        parts.push(word.to_owned());
    } else if unread {
        parts.push("Done".to_owned());
    }
    if let Some(b) = &marks.branch {
        parts.push(b.to_string());
    }
    parts.join(", ")
}

/// A project or plain row: title, account, status badge.
pub(super) fn plain_label(item: &SidebarItem) -> String {
    let mut parts = vec![item.label.to_string()];
    parts.extend(account(item));
    if let Some((_, word)) = item.status.and_then(ui::Status::badge) {
        parts.push(word.to_owned());
    }
    parts.join(", ")
}
