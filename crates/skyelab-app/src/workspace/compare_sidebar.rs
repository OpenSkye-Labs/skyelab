//! The sidebar side of multi-model send: a comparison group's threads fold into one row with a "×N" toggle, and
//! list under it, each led by its model, while it is expanded.

use super::Workspace;
use super::center::Page;
use super::compare::OpenCompare;
use super::panels::{RowMarks, SidebarItem};
use crate::ui::Status;
use gpui::SharedString;
use skyelab_store::Thread;
use std::collections::{HashMap, HashSet};

/// A sidebar entry: a thread, or a comparison group's members folded under one row.
pub(super) enum Entry<'a> {
    One(&'a Thread),
    Group(i64, Vec<&'a Thread>),
}

/// Folds each comparison group of `threads` into one entry, where its first member is listed.
pub(super) fn fold(threads: Vec<&Thread>) -> Vec<Entry<'_>> {
    let mut out: Vec<Entry> = Vec::new();
    for t in threads {
        match t.compare_group {
            None => out.push(Entry::One(t)),
            Some(g) => match out
                .iter_mut()
                .find(|e| matches!(e, Entry::Group(h, _) if *h == g))
            {
                Some(Entry::Group(_, members)) => members.push(t),
                _ => out.push(Entry::Group(g, vec![t])),
            },
        }
    }
    // One thread left listed (the others archived, or filtered out): a plain row.
    out.into_iter()
        .map(|e| match e {
            Entry::Group(_, m) if m.len() == 1 => Entry::One(m[0]),
            e => e,
        })
        .collect()
}

/// Moves every comparison group's members into the first of `sections` that holds one of them (pinned before
/// working before the rest), so a group isn't split while one model still works.
pub(super) fn keep_groups_together(sections: &mut [Vec<&Thread>]) {
    for i in 0..sections.len() {
        let groups: HashSet<i64> = sections[i].iter().filter_map(|t| t.compare_group).collect();
        if groups.is_empty() {
            continue;
        }
        for j in i + 1..sections.len() {
            let (moved, kept): (Vec<&Thread>, Vec<&Thread>) = std::mem::take(&mut sections[j])
                .into_iter()
                .partition(|t| t.compare_group.is_some_and(|g| groups.contains(&g)));
            sections[j] = kept;
            sections[i].extend(moved);
        }
    }
}

/// A comparison group's sidebar row, for [`mark_groups`].
pub(super) struct GroupRow {
    group: i64,
    open: bool,
    /// The members' row ids, oldest first.
    members: Vec<SharedString>,
}

impl Workspace {
    /// One sidebar section's rows: each thread's, and for a comparison group one row (its first thread's title, the
    /// busiest member's status) that opens the Compare page, followed by its threads while it is expanded.
    pub(super) fn fold_section(
        &self,
        threads: Vec<&Thread>,
        item: &mut dyn FnMut(&Thread) -> SidebarItem,
        rows: &mut Vec<GroupRow>,
        cx: &gpui::App,
    ) -> Vec<SidebarItem> {
        let shown = match self.page {
            Page::Compare(g) => Some(g),
            _ => None,
        };
        let mut out = section_items(threads, &self.compare.open, shown, item, rows);
        // A group's threads share a title: each listed one leads with its model, once its view knows it.
        for i in &mut out {
            let member = rows.iter().any(|r| r.members.contains(&i.id));
            let id = i.id.strip_prefix("thread-").and_then(|n| n.parse().ok());
            if let (true, Some(v)) = (member, id.and_then(|id| self.open_view(id, cx))) {
                i.label = format!("{} · {}", v.read(cx).pick_label(true), i.label).into();
            }
        }
        out
    }
}

fn section_items(
    threads: Vec<&Thread>,
    open: &HashSet<i64>,
    shown: Option<i64>,
    item: &mut dyn FnMut(&Thread) -> SidebarItem,
    rows: &mut Vec<GroupRow>,
) -> Vec<SidebarItem> {
    let mut out = Vec::new();
    for entry in fold(threads) {
        let (group, members) = match entry {
            Entry::One(t) => {
                out.push(item(t));
                continue;
            }
            Entry::Group(g, mut members) => {
                members.sort_by_key(|t| t.id); // oldest row first, under the newest-first list
                (g, members)
            }
        };
        let items: Vec<SidebarItem> = members.into_iter().map(&mut *item).collect();
        let statuses = || items.iter().filter_map(|i| i.status);
        let status = statuses()
            .find(|s| matches!(s, Status::Starting | Status::Running))
            .or_else(|| statuses().find(|s| *s == Status::Failed))
            .or(items[0].status);
        out.push(SidebarItem {
            id: format!("compare-{group}").into(),
            label: items[0].label.clone(),
            selected: shown == Some(group),
            action: Box::new(OpenCompare { group }),
            status,
            updated_ms: items.iter().filter_map(|i| i.updated_ms).max(),
            account: items[0].account.clone(),
            project: items[0].project.clone(),
        });
        let open = open.contains(&group);
        rows.push(GroupRow {
            group,
            open,
            members: items.iter().map(|i| i.id.clone()).collect(),
        });
        if open {
            out.extend(items);
        }
    }
    out
}

/// Gives each group row its marks (the "×N" toggle, its first thread's preview and card, unread if any member is)
/// and marks the members' rows as nested under it.
pub(super) fn mark_groups(marks: &mut HashMap<SharedString, RowMarks>, rows: Vec<GroupRow>) {
    for r in rows {
        let member = |id: &SharedString| marks.get(id);
        let head = r.members.first().and_then(member);
        let row = RowMarks {
            unread: r.members.iter().filter_map(member).any(|m| m.unread),
            started_ms: r
                .members
                .iter()
                .filter_map(member)
                .filter_map(|m| m.started_ms)
                .min(),
            preview: head.and_then(|m| m.preview.clone()),
            branch: head.and_then(|m| m.branch.clone()),
            card: head.and_then(|m| m.card.clone()),
            group: Some((r.members.len(), r.open, r.group)),
            ..Default::default()
        };
        for id in &r.members {
            if let Some(m) = marks.get_mut(id) {
                m.nested = true;
            }
        }
        marks.insert(format!("compare-{}", r.group).into(), row);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(id: i64, group: Option<i64>) -> Thread {
        Thread {
            id,
            project_id: 1,
            title: format!("t{id}"),
            cwd: "/demo".into(),
            account_id: "acme".into(),
            agent_session_id: None,
            status: "idle".into(),
            created_at: 0,
            updated_at: 0,
            worktree_path: None,
            worktree_branch: None,
            worktree_base: None,
            pr_url: None,
            parent_thread_id: None,
            permission_mode: "default".into(),
            pinned_at: None,
            archived_at: None,
            unread: false,
            forked_from: None,
            pr_state: None,
            side_of: None,
            compare_group: group,
            scheduled_task_id: None,
        }
    }

    #[test]
    fn groups_fold_at_their_first_member_and_stay_together() {
        let rows = [t(1, None), t(2, Some(9)), t(3, None), t(4, Some(9))];
        let folded = fold(rows.iter().collect());
        let shape: Vec<String> = folded
            .iter()
            .map(|e| match e {
                Entry::One(t) => t.id.to_string(),
                Entry::Group(g, m) => format!(
                    "{g}:{}",
                    m.iter().map(|t| t.id.to_string()).collect::<String>()
                ),
            })
            .collect();
        assert_eq!(shape, ["1", "9:24", "3"]);
        // The kept one alone: no group row.
        let kept = [t(5, Some(9))];
        assert!(matches!(fold(kept.iter().collect())[..], [Entry::One(_)]));

        // One model still working: the finished one joins it on the working shelf.
        let (working, rest) = (t(4, Some(9)), [t(2, Some(9)), t(3, None)]);
        let mut sections = vec![vec![], vec![&working], rest.iter().collect()];
        keep_groups_together(&mut sections);
        let ids = |s: &Vec<&Thread>| s.iter().map(|t| t.id).collect::<Vec<_>>();
        assert_eq!(
            sections.iter().map(ids).collect::<Vec<_>>(),
            [vec![], vec![4, 2], vec![3]]
        );
    }
}
