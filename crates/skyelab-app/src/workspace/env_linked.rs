//! Linked machines: other machines whose threads also show in This Mac's list (T3's sidebar across environments),
//! each row marked with its machine and opened in place, without switching. Each linked name keeps its own
//! `RemoteEnv`, made like the switcher's (same options, node pinning, approval), so it can do exactly what the
//! switcher can there and nothing more. Its listings rebuild the sidebar only when something drawn changed.

use super::Workspace;
use super::center::Page;
use super::env_sidebar::remote_row;
use super::panels::{RowCard, RowMarks, SidebarItem, SidebarSection};
use super::thread_list::plain_section;
use crate::environment::{ConnState, EnvEvent, RemoteEnv};
use crate::ui::Status;
use gpui::{Action, Context, Entity, SharedString, Subscription, Window};
use skyelab_client::is_magicdns_name;
use std::collections::{HashMap, HashSet};
use std::hash::{Hash as _, Hasher as _};

/// Shows `host`'s threads in this machine's list, or stops showing them.
#[derive(Clone, PartialEq, Action)]
#[action(namespace = skyelab, no_json)]
pub struct ToggleLinkedEnvironment {
    pub host: String,
}

/// Opens thread `id` of linked machine `host` as the page.
#[derive(Clone, PartialEq, Action)]
#[action(namespace = skyelab, no_json)]
pub struct OpenLinkedThread {
    pub host: String,
    pub id: i64,
}

pub(super) struct Linked {
    pub env: Entity<RemoteEnv>,
    /// A hash of what the sidebar last drew from this machine.
    drawn: u64,
    /// Threads working at the last listing.
    working: HashSet<i64>,
    /// Threads that stopped working while not shown, until opened (the row's "Done").
    done: HashSet<i64>,
    _subs: [Subscription; 2],
}

/// The name's first label: "desk" for desk.tail1.ts.net.
fn short(host: &str) -> SharedString {
    host.split('.').next().unwrap_or(host).to_owned().into()
}

/// A linked machine's state in a word or two, for its sidebar row.
fn state_word(state: &ConnState) -> &'static str {
    match state {
        ConnState::Connecting => "Connecting",
        ConnState::Connected => "Connected",
        ConnState::Reconnecting(_) => "Reconnecting",
        ConnState::NotApproved(_) => "Waiting for approval",
        ConnState::Denied => "Denied",
        ConnState::Refused | ConnState::NotYourDevice(_) | ConnState::IdentityMismatch(_) => {
            "Not accepted"
        }
        ConnState::NodeChanged { .. } => "Changed machine",
        ConnState::Unverified(_) => "Can't check",
        ConnState::VersionMismatch(_) => "Update needed",
        ConnState::Stopped(_) => "Disconnected",
    }
}

fn working(status: &str) -> bool {
    matches!(Status::parse(status), Status::Starting | Status::Running)
}

/// Everything a linked machine's rows draw, hashed: its state, its projects' names and every listed field.
fn drawn(env: &RemoteEnv) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    env.state.label().hash(&mut h);
    for p in &env.projects {
        (p.id, &p.name, &p.path).hash(&mut h);
    }
    for t in &env.threads {
        (
            t.id,
            t.project_id,
            &t.title,
            &t.status,
            t.pinned,
            &t.preview,
            t.settled_at,
            t.snoozed_until,
        )
            .hash(&mut h);
        (&t.account_id, &t.account_label, t.updated_at).hash(&mut h);
    }
    h.finish()
}

impl Workspace {
    #[cfg(test)]
    pub(crate) fn linked_envs(&self) -> Vec<Entity<RemoteEnv>> {
        self.env.linked.iter().map(|l| l.env.clone()).collect()
    }

    fn linked_names(&self) -> Vec<String> {
        self.env.linked_hosts.clone()
    }

    /// The linked connections to `host` (one, or none).
    pub(super) fn linked_envs_of(&self, host: &str, cx: &gpui::App) -> Vec<Entity<RemoteEnv>> {
        self.env
            .linked
            .iter()
            .filter(|l| l.env.read(cx).host.eq_ignore_ascii_case(host))
            .map(|l| l.env.clone())
            .collect()
    }

    /// The page shows a linked thread: it is seen, however the page came up (a click, Back, Forward).
    pub(super) fn seen_linked(&mut self, cx: &gpui::App) {
        for i in 0..self.env.linked.len() {
            if let Some(id) = self.shown_on(&self.env.linked[i].env, cx) {
                self.env.linked[i].done.remove(&id);
            }
        }
    }

    pub(super) fn toggle_linked(
        &mut self,
        a: &ToggleLinkedEnvironment,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !is_magicdns_name(&a.host) {
            return;
        }
        let mut names = self.linked_names();
        let before = names.len();
        names.retain(|h| !h.eq_ignore_ascii_case(&a.host));
        let link = names.len() == before;
        if link {
            names.push(a.host.clone());
        }
        self.sync_linked(names, window, cx);
        let host = a.host.clone();
        // Sets the state shown, rather than flipping the file's, so an edit made elsewhere can't invert it.
        self.save_env_settings(
            move |s| {
                if s.is_linked_environment(&host) != link {
                    s.toggle_linked_environment(&host);
                }
            },
            cx,
        );
    }

    /// Connects to every name in `names` not linked yet and drops the ones no longer in it, with their tabs.
    pub(super) fn sync_linked(
        &mut self,
        names: Vec<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (keep, gone): (Vec<_>, Vec<_>) = std::mem::take(&mut self.env.linked)
            .into_iter()
            .partition(|l| {
                let host = &l.env.read(cx).host;
                names.iter().any(|n| n.eq_ignore_ascii_case(host))
            });
        self.env.linked = keep;
        for l in gone {
            self.close_tabs_of(&l.env, window, cx);
        }
        let mut hosts: Vec<String> = Vec::new();
        for name in names {
            if hosts.iter().any(|h| h.eq_ignore_ascii_case(&name)) {
                continue;
            }
            hosts.push(name.clone());
            if !self.linked_envs_of(&name, cx).is_empty() {
                continue;
            }
            // A connection that can't even start shows nothing; the switcher says why when picked.
            let Ok(env) = self.dial(&name, cx) else {
                continue;
            };
            let subs = [
                cx.observe(&env, |this, env, cx| this.linked_changed(&env, cx)),
                cx.subscribe_in(&env, window, |this, env, e: &EnvEvent, window, cx| {
                    if let EnvEvent::Untrusted = e {
                        this.close_tabs_of(env, window, cx);
                    }
                }),
            ];
            self.env.linked.push(Linked {
                env,
                drawn: 0,
                working: HashSet::new(),
                done: HashSet::new(),
                _subs: subs,
            });
        }
        self.env.linked_hosts = hosts;
        self.refresh_sidebar(cx);
    }

    /// Closes the thread views of `env`; the page falls back to Welcome if it showed one.
    fn close_tabs_of(
        &mut self,
        env: &Entity<RemoteEnv>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (gone, keep): (Vec<_>, Vec<_>) = std::mem::take(&mut self.env.tabs)
            .into_iter()
            .partition(|(t, _)| t.read(cx).env() == env);
        self.env.tabs = keep;
        for (tab, _) in gone {
            tab.update(cx, |t, cx| t.closed(cx));
        }
        if !self.alive(&self.page, cx) {
            self.page = Page::Welcome;
            self.apply_page(window, cx);
        }
    }

    /// The thread of `env` the page shows, if any.
    fn shown_on(&self, env: &Entity<RemoteEnv>, cx: &gpui::App) -> Option<i64> {
        match &self.page {
            Page::RemoteThread(v) => v
                .upgrade()
                .map(|v| v.read(cx))
                .filter(|v| v.env() == env)
                .and_then(|v| v.thread_id()),
            _ => None,
        }
    }

    /// A linked machine notified: notes threads that stopped working unseen, and rebuilds the sidebar only when what
    /// it draws changed and this machine's list is the one shown.
    fn linked_changed(&mut self, env: &Entity<RemoteEnv>, cx: &mut Context<Self>) {
        let shown = self.shown_on(env, cx);
        let Some(l) = self.env.linked.iter_mut().find(|l| l.env == *env) else {
            return;
        };
        let e = env.read(cx);
        let now: HashSet<i64> = e
            .threads
            .iter()
            .filter(|t| working(&t.status))
            .map(|t| t.id)
            .collect();
        let finished: Vec<i64> = l
            .working
            .difference(&now)
            .copied()
            .filter(|id| Some(*id) != shown && e.threads.iter().any(|t| t.id == *id))
            .collect();
        let sig = drawn(e);
        let changed = sig != l.drawn || !finished.is_empty();
        l.drawn = sig;
        l.working = now;
        l.done.extend(finished);
        if changed && !self.is_remote() {
            self.refresh_sidebar(cx);
        }
    }

    /// The command palette's entries for linked machines: a link toggle for every machine this window knows (recent
    /// or linked), and each linked machine's threads.
    pub(super) fn linked_palette_entries(&self, cx: &gpui::App) -> Vec<crate::palette::Entry> {
        use crate::palette::{Entry, Kind};
        let entry = |label: String, kind, checked, action: Box<dyn Action>| {
            Entry::new(
                label,
                kind,
                std::rc::Rc::new(move |window, cx| {
                    crate::workspace::dispatch(action.boxed_clone(), window, cx)
                }),
            )
            .checked(checked)
        };
        let mut hosts = self.linked_names();
        if let Some(picker) = &self.env.picker {
            for r in &picker.read(cx).recents {
                if !hosts.iter().any(|h| h.eq_ignore_ascii_case(r)) {
                    hosts.push(r.clone());
                }
            }
        }
        let linked = self.linked_names();
        let mut out: Vec<Entry> = hosts
            .into_iter()
            .map(|host| {
                let on = linked.iter().any(|h| h.eq_ignore_ascii_case(&host));
                entry(
                    format!("Show threads from {host} in this list"),
                    Kind::Action("skyelab::ToggleLinkedEnvironment"),
                    Some(on),
                    Box::new(ToggleLinkedEnvironment { host }),
                )
            })
            .collect();
        // As in the sidebar: a machine that isn't connected lists no threads.
        for l in &self.env.linked {
            let e = l.env.read(cx);
            if e.state != ConnState::Connected {
                continue;
            }
            out.extend(e.threads.iter().map(|t| {
                entry(
                    format!("Open thread: {} · {}", t.title, short(&e.host)),
                    Kind::Thread,
                    None,
                    Box::new(OpenLinkedThread {
                        host: e.host.clone(),
                        id: t.id,
                    }),
                )
            }));
        }
        out
    }

    pub(super) fn open_linked_thread(
        &mut self,
        a: &OpenLinkedThread,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(env) = self.linked_envs_of(&a.host, cx).pop() else {
            return;
        };
        self.open_thread_on(env, a.id, window, cx);
    }

    /// This machine's list's rows from linked machines: (pinned, working, the rest newest first, snoozed, settled)
    /// and a "Linked machines" section for those not connected, with each row's marks added to `marks`. Nothing while searching or
    /// scoped to projects: those filter this machine's threads.
    pub(super) fn linked_rows(
        &self,
        marks: &mut HashMap<SharedString, RowMarks>,
        cx: &gpui::App,
    ) -> ([Vec<SidebarItem>; 5], Option<SidebarSection>) {
        let mut out: [Vec<SidebarItem>; 5] = Default::default();
        let now = crate::ui::now_ms();
        let mut waiting = Vec::new();
        if self.thread_search.is_some() || !self.scope.is_empty() {
            return (out, None);
        }
        for l in &self.env.linked {
            let e = l.env.read(cx);
            if e.state != ConnState::Connected {
                // One short line; the full state (an error, a code to approve) is in its hover card.
                let id: SharedString = format!("linked-state-{}", e.host).into();
                let card = RowCard {
                    title: e.host.clone().into(),
                    note: Some(e.state.label()),
                    ..RowCard::default()
                };
                marks.insert(
                    id.clone(),
                    RowMarks {
                        card: Some(std::rc::Rc::new(card)),
                        ..RowMarks::default()
                    },
                );
                waiting.push(SidebarItem {
                    id,
                    label: format!("{} · {}", short(&e.host), state_word(&e.state)).into(),
                    selected: false,
                    action: Box::new(gpui::NoAction),
                    status: None,
                    updated_ms: None,
                    account: None,
                    project: None,
                });
                continue;
            }
            let shown = self.shown_on(&l.env, cx);
            let names: HashMap<i64, &skyelab_client::Project> =
                e.projects.iter().map(|p| (p.id, p)).collect();
            for t in &e.threads {
                let (item, m) = remote_row(e, &names, t, shown == Some(t.id));
                let item = SidebarItem {
                    id: format!("linked-{}-{}", e.host, t.id).into(),
                    action: Box::new(OpenLinkedThread {
                        host: e.host.clone(),
                        id: t.id,
                    }),
                    ..item
                };
                marks.insert(
                    item.id.clone(),
                    RowMarks {
                        machine: Some(short(&e.host)),
                        unread: l.done.contains(&t.id),
                        ..m
                    },
                );
                out[super::env_sidebar::remote_slot(t, working(&t.status), now)].push(item);
            }
        }
        out[2].sort_by_key(|i| std::cmp::Reverse(i.updated_ms));
        let section = (!waiting.is_empty()).then(|| plain_section("Linked machines", waiting));
        (out, section)
    }
}

/// Puts each of `add` (newest first) before the first row of `rows` that changed earlier, never between a
/// comparison group's row and its nested threads.
pub(super) fn merge_by_time(
    rows: &mut Vec<SidebarItem>,
    add: Vec<SidebarItem>,
    marks: &HashMap<SharedString, RowMarks>,
) {
    let nested = |i: &SidebarItem| marks.get(&i.id).is_some_and(|m| m.nested);
    for item in add {
        let at = rows
            .iter()
            .position(|r| !nested(r) && r.updated_ms < item.updated_ms)
            .unwrap_or(rows.len());
        rows.insert(at, item);
    }
}
