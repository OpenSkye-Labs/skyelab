//! The sidebar while another machine is shown: the same flat list as this machine's (Pinned, Working, then by day),
//! built from what that machine lists (protocol 1.4 adds the pin and reply preview; an older one sends neither), then
//! its projects, the selected one taking New thread.

use super::Workspace;
use super::center::Page;
use super::env_mode::{
    NewRemoteThread, OpenRemoteThread, SelectRemoteProject, SwitchEnvironment, TrustRemoteNode,
};
use super::panels::{RowAccount, RowCard, RowMarks, SidebarItem, SidebarSection};
use super::thread_list::{flat_sections, plain_section, preview_text};
use crate::environment::{ConnState, RemoteEnv};
use crate::ui::Status;
use gpui::{Context, SharedString};
use skyelab_client::Thread;
use std::collections::HashMap;

/// The remote sidebar's sections, each thread row's marks and the Snoozed and Settled shelves.
pub(super) type RemoteSidebar = (
    Vec<SidebarSection>,
    HashMap<SharedString, RowMarks>,
    Vec<super::panels::Shelf>,
);

/// Where another machine's thread row lists: 0 pinned, 1 working, 2 the rest, 3 snoozed, 4 settled. T3's order: only
/// working beats a shelf, and a shelf beats the pin.
pub(super) fn remote_slot(t: &Thread, working: bool, now: i64) -> usize {
    if !working && skyelab_core::snooze::is_asleep(t.snoozed_until, now) {
        3
    } else if !working && t.settled_at.is_some() {
        4
    } else if t.pinned {
        0
    } else if working {
        1
    } else {
        2
    }
}

/// A thread's account as the row badge shows it: the remote's label, else its id. Only id and label are drawn; the
/// folder is the other machine's, so none is given.
fn account_of(t: &Thread) -> RowAccount {
    RowAccount::Bound(skyelab_core::Account {
        id: t.account_id.clone(),
        label: t
            .account_label
            .clone()
            .unwrap_or_else(|| t.account_id.clone()),
        provider: Default::default(),
        config_dir: Default::default(),
        gh_config_dir: None,
        ssh_agent: false,
    })
}

/// A thread row of another machine and its marks, as the switcher's list and a linked machine's rows draw it. Its
/// action opens it through the switcher; a linked row replaces it.
pub(super) fn remote_row(
    env: &RemoteEnv,
    names: &HashMap<i64, &skyelab_client::Project>,
    t: &Thread,
    selected: bool,
) -> (SidebarItem, RowMarks) {
    let project: SharedString = names
        .get(&t.project_id)
        .map(|p| p.name.clone().into())
        .unwrap_or_default();
    let card = RowCard {
        title: t.title.clone().into(),
        project: project.clone(),
        path: names
            .get(&t.project_id)
            .map(|p| p.path.clone().into())
            .unwrap_or_default(),
        account: Some(account_of(t)),
        binding: Some(format!("Runs on {}", env.host).into()),
        created_ms: t.created_at,
        updated_ms: t.updated_at,
        ..RowCard::default()
    };
    let marks = RowMarks {
        pinned: t.pinned,
        preview: t
            .preview
            .as_deref()
            .map(preview_text)
            .filter(|p| !p.is_empty())
            .map(Into::into),
        card: Some(std::rc::Rc::new(card)),
        shelf: Some(super::panels::RowShelf {
            id: t.id,
            settled: t.settled_at.is_some(),
            snoozed_until: t.snoozed_until,
            host: Some(env.host.clone().into()),
        }),
        ..RowMarks::default()
    };
    let item = SidebarItem {
        id: format!("remote-thread-{}", t.id).into(),
        label: t.title.clone().into(),
        selected,
        action: Box::new(OpenRemoteThread { id: t.id }),
        status: Some(Status::parse(&t.status)),
        updated_ms: Some(t.updated_at),
        account: Some(account_of(t)),
        project: Some(project),
    };
    (item, marks)
}

impl Workspace {
    /// The sidebar in remote mode. `None` on this machine.
    pub(super) fn remote_sections(&self, cx: &Context<Self>) -> Option<RemoteSidebar> {
        let env = self.remote_env()?.read(cx);
        let shown = match &self.page {
            Page::RemoteThread(v) => v.upgrade().and_then(|v| v.read(cx).thread_id()),
            _ => None,
        };
        let names: HashMap<i64, &skyelab_client::Project> =
            env.projects.iter().map(|p| (p.id, p)).collect();
        let mut marks = HashMap::new();
        let now = crate::ui::now_ms();
        let mut slots: [Vec<&Thread>; 5] = Default::default();
        for t in &env.threads {
            let working = matches!(Status::parse(&t.status), Status::Starting | Status::Running);
            slots[remote_slot(t, working, now)].push(t);
        }
        slots[3].sort_by_key(|t| t.snoozed_until);
        slots[4].sort_by_key(|t| std::cmp::Reverse(t.settled_at));
        let [pinned, working, rest, snoozed, settled] = slots.map(|ts| {
            ts.into_iter()
                .map(|t| {
                    let (item, m) = remote_row(env, &names, t, shown == Some(t.id));
                    marks.insert(item.id.clone(), m);
                    item
                })
                .collect::<Vec<_>>()
        });
        let shelves = super::thread_list::shelves(snoozed, settled);
        let connected = env.state == ConnState::Connected;
        let mut sections = flat_sections(pinned, working, rest);
        if sections.is_empty() && shelves.is_empty() && connected && !env.projects.is_empty() {
            sections.push(SidebarSection {
                empty_title: match &env.error {
                    Some(e) => format!("Couldn't list threads: {e}").into(),
                    None => "No threads there".into(),
                },
                empty_hint: "Start one with New thread. It runs on the other machine.".into(),
                ..plain_section("Threads", Vec::new())
            });
        }
        let trust = match &env.state {
            // Only a node the connection really presents can be trusted (none: an old server sent no id).
            ConnState::NodeChanged { now, .. } if env.presented_node() == Some(now) => Some((
                SharedString::from("Trust it"),
                Box::new(TrustRemoteNode {
                    node_id: now.clone(),
                }) as Box<_>,
            )),
            _ => None,
        };
        let projects = env
            .projects
            .iter()
            .map(|p| SidebarItem {
                project: None,
                id: format!("remote-project-{}", p.id).into(),
                label: p.name.clone().into(),
                selected: env.project == Some(p.id),
                action: Box::new(SelectRemoteProject { id: p.id }),
                status: None,
                updated_ms: None,
                // A project row; its binding lives on the other machine, so no badge.
                account: Some(RowAccount::Hidden),
            })
            .collect();
        sections.push(SidebarSection {
            empty_title: match (&env.error, connected) {
                (Some(e), true) => format!("Couldn't list projects: {e}").into(),
                (None, true) => "No projects there".into(),
                _ => env.state.label(),
            },
            empty_hint: match (&trust, &env.error) {
                (Some(_), Some(e)) => e.clone(),
                _ => format!("Projects on {}. Add them on that machine.", env.host).into(),
            },
            button: trust.or_else(|| {
                env.project.map(|_| {
                    (
                        SharedString::from("New thread"),
                        Box::new(NewRemoteThread) as Box<_>,
                    )
                })
            }),
            ..plain_section("Projects", projects)
        });
        // Open local threads marked unread: background starts, and turns that finished unseen.
        let waiting = self
            .open_threads
            .iter()
            .filter(|v| v.read(cx).thread.as_ref().is_some_and(|t| t.unread))
            .count();
        if waiting > 0 {
            let label = match waiting {
                1 => "1 new thread here".into(),
                n => format!("{n} new threads here").into(),
            };
            sections.push(plain_section(
                super::env_picker::THIS_MAC,
                vec![SidebarItem {
                    project: None,
                    id: "queued-local-tabs".into(),
                    label,
                    selected: false,
                    action: Box::new(SwitchEnvironment { host: None }),
                    status: None,
                    updated_ms: None,
                    account: None,
                }],
            ));
        }
        Some((sections, marks, shelves))
    }
}
