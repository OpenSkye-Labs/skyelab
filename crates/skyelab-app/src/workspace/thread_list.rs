//! The sidebar's rows: every project's threads in one list (pinned, working, then by day), the archived split, the
//! project scope filter and the thread filter.

use super::panels::{
    ArchivedRows, RowAccount, RowCard, RowMarks, ScopeMenu, ScopeProject, SidebarItem,
    SidebarSection,
};
use super::thread_rows::row_state;
use super::{AddProject, OpenThread, ShowAllProjects, Workspace, lock};
use gpui::{Action, AppContext as _, Context, SharedString, Window};
use skyelab_store::Thread;
use std::collections::HashMap;

/// Adds the project to the sidebar's scope filter, or takes it out.
#[derive(Clone, PartialEq, Action)]
#[action(namespace = skyelab, no_json)]
pub struct ToggleProjectScope {
    pub id: i64,
}

/// New thread in this project (the project picker's entries).
#[derive(Clone, PartialEq, Action)]
#[action(namespace = skyelab, no_json)]
pub struct NewThreadIn {
    pub id: i64,
}

impl Workspace {
    pub(super) fn refresh_sidebar(&mut self, cx: &mut Context<Self>) {
        if let Some((sections, marks)) = self.remote_sections(cx) {
            return self.sidebar.update(cx, |sidebar, cx| {
                sidebar.sections = sections;
                sidebar.marks = marks;
                sidebar.archived = None;
                sidebar.searchable = false;
                sidebar.scope = None;
                sidebar.sync_tick(cx);
                cx.notify();
            });
        }
        // An open tab knows its thread's state now; the row is only as fresh as the last load.
        for v in &self.open_threads {
            if let Some(t) = &v.read(cx).thread
                && let Some(row) = self.threads.iter_mut().find(|r| r.id == t.id)
            {
                row.pr_url.clone_from(&t.pr_url); // a PR opened or linked in the tab
            }
        }
        let live = self.live_rows(cx);
        let projects: HashMap<i64, &skyelab_store::Project> =
            self.projects.iter().map(|p| (p.id, p)).collect();
        let home = dirs::home_dir();
        let shown = self.shown_thread(cx);
        let previews = &self.previews;
        let mut marks = std::collections::HashMap::new();
        let mut item = |t: &Thread| {
            let id: SharedString = format!("thread-{}", t.id).into();
            let state = row_state(&live, t);
            let branch = t
                .worktree_branch
                .clone()
                .or_else(|| previews.branches.get(&t.project_id).cloned())
                .map(SharedString::from);
            let account = self.project_accounts.get(&t.project_id);
            let card = RowCard {
                title: t.title.clone().into(),
                project: projects
                    .get(&t.project_id)
                    .map(|p| p.name.clone().into())
                    .unwrap_or_default(),
                path: projects
                    .get(&t.project_id)
                    .map(|p| crate::ui::short_path(&p.path, home.as_deref()).into())
                    .unwrap_or_default(),
                worktree: t
                    .worktree_path
                    .as_deref()
                    .map(|w| crate::ui::short_path(w, home.as_deref()).into()),
                branch: branch.clone(),
                pr_url: t.pr_url.clone().map(Into::into),
                // The account the thread runs on, which a rebind since its start doesn't change.
                account: self
                    .config
                    .accounts
                    .iter()
                    .find(|a| a.id == t.account_id)
                    .map(|a| RowAccount::Bound(a.clone())),
                binding: match account {
                    Some(Some(a)) if a.id != t.account_id => {
                        Some(format!("Folder now bound to {}", a.label).into())
                    }
                    Some(None) => Some("Folder no longer bound".into()),
                    _ => previews.bindings.get(&t.project_id).map(|b| {
                        format!("Bound at {}", crate::ui::short_path(b, home.as_deref())).into()
                    }),
                },
                created_ms: t.created_at,
                updated_ms: t.updated_at,
            };
            marks.insert(
                id.clone(),
                RowMarks {
                    pinned: t.pinned_at.is_some(),
                    unread: t.unread,
                    approval: state.approval,
                    started_ms: state.started_ms.filter(|_| state.running()),
                    card: Some(std::rc::Rc::new(card)),
                    menu: super::manage::thread_menu(t),
                    preview: previews
                        .replies
                        .get(&t.id)
                        .map(|r| preview_text(r))
                        .filter(|p| !p.is_empty())
                        .map(Into::into),
                    branch,
                    pr: t.pr_url.as_deref().map(|u| pr_label(u).into()),
                },
            );
            SidebarItem {
                id,
                label: t.title.clone().into(),
                selected: Some(t.id) == shown,
                action: Box::new(OpenThread { id: t.id }),
                status: Some(state.status),
                updated_ms: Some(t.updated_at),
                account: Some(match account {
                    Some(Some(a)) => RowAccount::Bound(a.clone()),
                    Some(None) => RowAccount::Unbound,
                    None => RowAccount::Hidden,
                }),
                project: Some(
                    projects
                        .get(&t.project_id)
                        .map(|p| p.name.clone())
                        .unwrap_or_default()
                        .into(),
                ),
            }
        };
        let groups = self.thread_groups(cx);
        let [pinned, working, rest] = groups;
        let (pinned, working): (Vec<_>, Vec<_>) = (
            pinned.into_iter().map(&mut item).collect(),
            working.into_iter().map(&mut item).collect(),
        );
        let rest: Vec<SidebarItem> = rest.into_iter().map(&mut item).collect();
        let empty = rest.is_empty();
        let archived = ArchivedRows {
            items: self.listed_threads().1.into_iter().map(&mut item).collect(),
            open: self.show_archived,
            toggle: Box::new(super::manage::ToggleArchivedThreads),
        };
        let scope = ScopeMenu {
            projects: self
                .projects
                .iter()
                .map(|p| ScopeProject {
                    id: p.id,
                    name: p.name.clone().into(),
                    checked: self.scope.contains(&p.id),
                })
                .collect(),
        };
        let searching = self.thread_search.is_some();
        let no_projects = self.projects.is_empty();
        let section = plain_section;
        let mut sections = flat_sections(pinned, working, rest);
        // Nothing outside Pinned and Working: the list's empty state, which says what to do next.
        if empty {
            sections.push(SidebarSection {
                empty_title: if searching {
                    "No matching threads"
                } else if no_projects {
                    "No projects yet"
                } else {
                    "No threads yet"
                }
                .into(),
                empty_hint: if searching {
                    "Titles and messages are searched."
                } else if no_projects {
                    "Add a folder to start. It runs on the account it is bound to."
                } else {
                    "Start one with New thread."
                }
                .into(),
                button: no_projects.then(|| {
                    (
                        SharedString::from("Add project"),
                        Box::new(AddProject) as Box<_>,
                    )
                }),
                ..section("Threads", Vec::new())
            });
        }
        self.sidebar.update(cx, |sidebar, cx| {
            sidebar.marks = marks;
            sidebar.archived = Some(archived);
            sidebar.searchable = true;
            sidebar.scope = Some(scope);
            sidebar.sections = sections;
            sidebar.sync_tick(cx);
            cx.notify();
        });
    }

    /// Reloads the rows' previews (each thread's last reply, one query) and each project folder's branch.
    pub(super) fn load_previews(&mut self, cx: &mut Context<Self>) {
        let store = self.backend.store.clone();
        let folders: Vec<(i64, std::path::PathBuf)> = self
            .projects
            .iter()
            .map(|p| (p.id, p.path.clone()))
            .collect();
        let task = cx.background_spawn(async move {
            let replies = lock(&store)?.last_replies()?;
            let branches = folders
                .into_iter()
                .filter_map(|(id, path)| Some((id, skyelab_git::current_branch(&path).ok()??)))
                .collect();
            anyhow::Ok((replies, branches))
        });
        self.previews._load = Some(cx.spawn(async move |this, cx| {
            let loaded = task.await;
            let _ = this.update(cx, |this, cx| {
                match loaded {
                    Ok((replies, branches)) => {
                        this.previews.replies = replies;
                        this.previews.branches = branches;
                    }
                    Err(e) => this.store_error = Some(format!("{e:#}").into()),
                }
                this.refresh_sidebar(cx);
            });
        }));
    }

    /// The listed threads in the sidebar's three groups: pinned, working (running, starting or waiting on an approval),
    /// the rest. Each keeps the newest-first order.
    fn thread_groups(&self, cx: &gpui::App) -> [Vec<&Thread>; 3] {
        let live = self.live_rows(cx);
        let mut groups: [Vec<&Thread>; 3] = Default::default();
        for t in self.listed_threads().0 {
            let at = if t.pinned_at.is_some() {
                0
            } else if row_state(&live, t).working() {
                1
            } else {
                2
            };
            groups[at].push(t);
        }
        groups
    }

    /// The listed threads (in the scope, matching the filter) as (not archived, archived), newest first.
    pub(super) fn listed_threads(&self) -> (Vec<&Thread>, Vec<&Thread>) {
        // A title is checked live (rows change after a search ran); message matches come from the last search,
        // which reruns whenever the thread list reloads.
        let matches = |t: &&Thread| match &self.thread_search {
            None => true,
            Some((q, ids)) => {
                t.title.to_lowercase().contains(&q.to_lowercase())
                    || ids.as_ref().is_some_and(|ids| ids.contains(&t.id))
            }
        };
        let in_scope = |t: &&Thread| self.scope.is_empty() || self.scope.contains(&t.project_id);
        self.threads
            .iter()
            .filter(in_scope)
            .filter(matches)
            .partition(|t| t.archived_at.is_none())
    }

    /// The scope filter's check row: the project joins the filter, or leaves it.
    pub(super) fn toggle_project_scope(
        &mut self,
        a: &ToggleProjectScope,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match self.scope.iter().position(|&id| id == a.id) {
            Some(at) => {
                self.scope.remove(at);
            }
            None => self.scope.push(a.id),
        }
        // Narrowed to one project: the workspace (status bar, terminals, project actions) follows it.
        if let [id] = self.scope[..] {
            return self.select(id, cx);
        }
        self.refresh_sidebar(cx);
    }

    pub(super) fn show_all_projects(
        &mut self,
        _: &ShowAllProjects,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.scope.clear();
        self.refresh_sidebar(cx);
    }

    /// The order thread keys walk (previous/next, 1–9): the sidebar's, archived ones left out.
    pub(super) fn thread_order(&self, cx: &gpui::App) -> Vec<i64> {
        self.thread_groups(cx)
            .concat()
            .iter()
            .map(|t| t.id)
            .collect()
    }

    /// The thread list changed (a load, a project switch, a finished turn): run the filter's search again.
    pub(super) fn rerun_search(&mut self, cx: &mut Context<Self>) {
        if let Some((q, _)) = &self.thread_search {
            let q = SharedString::from(q.clone());
            self.search_threads(q, cx);
        }
    }

    /// The sidebar's filter changed: search titles and message text of every project's threads.
    pub(super) fn search_threads(&mut self, query: SharedString, cx: &mut Context<Self>) {
        let query = query.trim().to_owned();
        if query.is_empty() {
            self.thread_search = None;
            self._search = None;
            return self.refresh_sidebar(cx);
        }
        // Keep the last answer until the new one lands, so rows don't flicker on a rerun.
        let kept = match self.thread_search.take() {
            Some((q, ids)) if q == query => ids,
            _ => None,
        };
        self.thread_search = Some((query.clone(), kept));
        self.refresh_sidebar(cx);
        let store = self.backend.store.clone();
        let task =
            cx.background_spawn(async move { lock(&store)?.search_threads(&query, None, 500) });
        self._search = Some(cx.spawn(async move |this, cx| {
            let found = task.await;
            let _ = this.update(cx, |this, cx| {
                match found {
                    Ok(found) => {
                        if let Some((_, ids)) = this.thread_search.as_mut() {
                            *ids = Some(found.into_iter().map(|t| t.id).collect());
                        }
                    }
                    Err(e) => this.store_error = Some(format!("{e:#}").into()),
                }
                this.refresh_sidebar(cx);
            });
        }));
    }
}

/// What the sidebar's thread rows show besides the thread itself, loaded apart from the thread list.
#[derive(Default)]
pub(super) struct Previews {
    /// Thread id → its agent's last text, as stored (markdown).
    pub replies: HashMap<i64, String>,
    /// Project id → the branch its folder has checked out.
    pub branches: HashMap<i64, String>,
    /// Project id → the bound folder that decides its account (resolved with the accounts).
    pub bindings: HashMap<i64, std::path::PathBuf>,
    /// The running reload; a newer one replaces (cancels) it.
    _load: Option<gpui::Task<()>>,
}

/// A titled section with no empty state and no button.
pub(super) fn plain_section(title: &str, items: Vec<SidebarItem>) -> SidebarSection {
    SidebarSection {
        title: SharedString::from(title.to_owned()),
        items,
        empty_title: "".into(),
        empty_hint: "".into(),
        button: None,
    }
}

/// The flat list's sections: Pinned, Working, then `rest` by the day it last changed (each row's `updated_ms`),
/// every section keeping the order given; empty ones are left out. Shared by this machine's sidebar and another's.
pub(super) fn flat_sections(
    pinned: Vec<SidebarItem>,
    working: Vec<SidebarItem>,
    rest: Vec<SidebarItem>,
) -> Vec<SidebarSection> {
    let starts = day_starts();
    let mut days: [Vec<SidebarItem>; 4] = Default::default();
    for item in rest {
        days[day_bucket(starts, item.updated_ms.unwrap_or_default())].push(item);
    }
    let shelves = [("Pinned", pinned), ("Working", working)];
    shelves
        .into_iter()
        .chain(DAYS.into_iter().zip(days))
        .filter(|(_, items)| !items.is_empty())
        .map(|(title, items)| plain_section(title, items))
        .collect()
}

/// The date sections under Pinned and Working, T3's and Synara's: indexes [`day_bucket`] returns.
const DAYS: [&str; 4] = ["Today", "Yesterday", "Previous 7 days", "Older"];

/// Which of [`DAYS`] a thread last changed in, given where today, yesterday and the 7-day window start (local
/// midnights, ms since the epoch; see [`day_starts`]).
pub(super) fn day_bucket(starts: [i64; 3], updated_ms: i64) -> usize {
    starts
        .iter()
        .position(|&start| updated_ms >= start)
        .unwrap_or(3)
}

/// Local midnight today, yesterday and six days before today, in ms since the epoch: each its own local midnight, so
/// a daylight-saving change doesn't shift a section by an hour.
fn day_starts() -> [i64; 3] {
    use chrono::{Days, Local, TimeZone as _};
    let today = Local::now().date_naive();
    [0, 1, 6].map(|back| {
        today
            .checked_sub_days(Days::new(back))
            .and_then(|d| d.and_hms_opt(0, 0, 0))
            .and_then(|m| Local.from_local_datetime(&m).earliest())
            .map_or(i64::MIN, |m| m.timestamp_millis())
    })
}

/// A reply as one plain line for the sidebar: markdown marks dropped, whitespace collapsed.
pub(super) fn preview_text(markdown: &str) -> String {
    let mut out = String::with_capacity(markdown.len());
    for line in markdown.lines() {
        let line = line.trim();
        // Code fences and rules carry no words; headings, quotes and bullets keep theirs.
        if line.starts_with("```")
            || line
                .chars()
                .all(|c| matches!(c, '-' | '*' | '_' | '|' | ':' | ' '))
        {
            continue;
        }
        // A heading's marks ("## "), a quote's ("> "); "#24 merged" keeps its number.
        let line = line.trim_start_matches(['>', ' ']);
        let line = match line.trim_start_matches('#') {
            rest if rest.starts_with(' ') => rest.trim_start(),
            _ => line,
        };
        let line = line
            .strip_prefix("- ")
            .or_else(|| line.strip_prefix("* "))
            .unwrap_or(line);
        if !out.is_empty() {
            out.push(' ');
        }
        out.extend(
            link_text(line)
                .chars()
                .filter(|c| !matches!(c, '*' | '`' | '|')),
        );
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// `[text](url)` and `![alt](url)` as just their text; a `[` that isn't a link stays.
fn link_text(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut rest = line;
    while let Some(open) = rest.find('[') {
        let after = &rest[open + 1..];
        let link = after.find("](").and_then(|close| {
            let url = &after[close + 2..];
            url.find(')').map(|end| (&after[..close], &url[end + 1..]))
        });
        match link {
            Some((text, tail)) => {
                out.push_str(rest[..open].strip_suffix('!').unwrap_or(&rest[..open]));
                out.push_str(text);
                rest = tail;
            }
            None => {
                out.push_str(&rest[..=open]);
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// "#24" for a pull request URL ending in its number, else "PR".
pub(super) fn pr_label(url: &str) -> String {
    match url.trim_end_matches('/').rsplit_once('/') {
        Some((_, n)) if !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()) => format!("#{n}"),
        _ => "PR".into(),
    }
}
