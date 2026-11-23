//! Multi-model send (T3's `useMultipleModelSend`): one prompt to 2–4 models of the account the folder is bound to.
//! Each model runs as its own thread; the threads share a comparison group, fold into one sidebar row and are read
//! side by side on the Compare page (`compare_view.rs`).

use super::center::Page;
use super::thread::ThreadView;
use super::{OpenThread, Workspace};
use gpui::{Action, AppContext as _, Context, Entity, ParentElement as _, SharedString, Window};
use skyelab_core::Account;
use skyelab_store::ModelChoice;
use std::collections::HashSet;

/// The most models one send goes to.
pub(crate) const MAX_MODELS: usize = 4;
/// The most comparison threads running at once in a window; a send that would pass it is refused.
pub(crate) const MAX_RUNNING: usize = 4;

/// The composer's model set while "Send to several models" is on.
#[derive(Clone, Debug, PartialEq)]
pub struct Compare {
    /// Model ids, in the order they were ticked: the draft runs the first.
    pub models: Vec<String>,
    /// Each model in its own worktree, so parallel edits don't collide.
    pub worktrees: bool,
}

/// Opens the Compare page of a comparison group.
#[derive(Clone, PartialEq, Action)]
#[action(namespace = skyelab, no_json)]
pub struct OpenCompare {
    pub group: i64,
}

/// Shows or hides a comparison group's threads under its sidebar row.
#[derive(Clone, PartialEq, Action)]
#[action(namespace = skyelab, no_json)]
pub struct ToggleCompareGroup {
    pub group: i64,
}

/// "Keep this one" on a Compare column: asks whether to archive the other threads.
#[derive(Clone, PartialEq, Action)]
#[action(namespace = skyelab, no_json)]
pub struct KeepCompare {
    pub id: i64,
}

/// The workspace's comparison state: the page's view and the groups expanded in the sidebar.
#[derive(Default)]
pub struct State {
    pub view: Option<Entity<super::compare_view::CompareView>>,
    pub open: HashSet<i64>,
}

impl ThreadView {
    /// "Send to several models…": the current model is the first of the set.
    pub(in crate::workspace) fn start_compare(&mut self, cx: &mut Context<Self>) {
        self.composer.compare = Some(Compare {
            models: self.composer.pick.model.clone().into_iter().collect(),
            // Default on in a git repo, where the footer found a branch.
            worktrees: self.composer.footer.branch.is_some(),
        });
        cx.notify();
    }

    /// Back to one model: the first ticked one, if any.
    pub(in crate::workspace) fn stop_compare(&mut self, cx: &mut Context<Self>) {
        if let Some(first) = self
            .composer
            .compare
            .take()
            .and_then(|c| c.models.into_iter().next())
        {
            self.composer.pick.model = Some(first);
        }
        cx.notify();
    }

    /// Ticks or unticks a model; a fifth is refused.
    pub(in crate::workspace) fn toggle_compare_model(
        &mut self,
        id: String,
        cx: &mut Context<Self>,
    ) {
        let Some(c) = self.composer.compare.as_mut() else {
            return;
        };
        match c.models.iter().position(|m| *m == id) {
            Some(at) => _ = c.models.remove(at),
            None if c.models.len() < MAX_MODELS => c.models.push(id),
            None => {}
        }
        cx.notify();
    }

    pub(in crate::workspace) fn toggle_compare_worktrees(&mut self, cx: &mut Context<Self>) {
        if let Some(c) = self.composer.compare.as_mut() {
            c.worktrees = !c.worktrees;
            cx.notify();
        }
    }

    /// Whether a send goes to several models: two or more ticked, in a draft.
    pub(in crate::workspace) fn comparing(&self) -> bool {
        self.thread.is_none()
            && self
                .composer
                .compare
                .as_ref()
                .is_some_and(|c| c.models.len() > 1)
    }
}

impl Workspace {
    /// Starts one thread per ticked model of `view`'s draft, all under `account`. The draft runs the first model; the
    /// others start in the background beside it. Refused, sending nothing, when the folder no longer resolves to
    /// `account` (a comparison never runs under another account) or when too many comparison threads already run.
    pub(super) fn compare_send(
        &mut self,
        view: &Entity<ThreadView>,
        account: &Account,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (project_id, compare, mode) = {
            let v = view.read(cx);
            (v.project_id, v.composer.compare.clone(), v.mode)
        };
        let Some(compare) = compare.filter(|c| c.models.len() > 1) else {
            return;
        };
        let refuse = |why: String, cx: &mut Context<Self>| {
            view.update(cx, |v, cx| {
                v.composer.notice = Some(why.into());
                cx.notify();
            })
        };
        let Some(project) = self.projects.iter().find(|p| p.id == project_id).cloned() else {
            return refuse("This thread's project is gone.".into(), cx);
        };
        match self.config.resolve(&project.path) {
            Some(now) if now.id == account.id => {}
            _ => {
                return refuse(
                    format!(
                        "{} isn't bound to {} any more, so nothing was sent.",
                        project.path.display(),
                        account.label
                    ),
                    cx,
                );
            }
        }
        let running = self
            .open_threads
            .iter()
            .filter(|v| {
                let v = v.read(cx);
                let grouped = v
                    .compare_group
                    .or(v.thread.as_ref().and_then(|t| t.compare_group));
                grouped.is_some() && v.working()
            })
            .count();
        if running + compare.models.len() > MAX_RUNNING {
            return refuse(
                format!(
                    "{running} compared threads are still running; at most {MAX_RUNNING} run at once. \
                     Wait for them, or tick fewer models."
                ),
                cx,
            );
        }
        // The send's time, kept unique in this process (two windows can send in the same millisecond).
        use std::sync::atomic::{AtomicI64, Ordering::Relaxed};
        static LAST: AtomicI64 = AtomicI64::new(0);
        let group = (LAST.load(Relaxed) + 1).max(crate::ui::now_ms());
        LAST.store(group, Relaxed); // every window runs on the UI thread, so load and store can't interleave
        let pick = |model: &String| ModelChoice {
            model: Some(model.clone()),
            effort: None,
        };
        let (first, rest) = compare.models.split_first().expect("checked: two or more");
        let (text, attachments) = view.update(cx, |v, cx| {
            let raw = v.prompt.read(cx).value().trim().to_string();
            let attachments = v.take_attachments(false);
            (v.take_contexts(raw, false), attachments)
        });
        let base = view.read(cx).branch_picker.read(cx).base.clone();
        // Resolved once, by the first column to make its worktree, so a branch that moves meanwhile can't split them.
        let pin = super::worktree::BasePin::default();
        let launch = |v: &mut ThreadView,
                      model: &String,
                      window: &mut Window,
                      cx: &mut Context<ThreadView>| {
            v.compare_group = Some(group);
            v.composer.compare = None;
            v.composer.pick = pick(model);
            v.composer.picked = true;
            v.composer.picker_open = false;
            v.wt.use_worktree = compare.worktrees;
            v.wt.pin = Some(pin.clone());
            v.branch_picker.update(cx, |b, _| b.base.clone_from(&base)); // every model from the same code
            v.required_account = Some(account.id.clone());
            v.mode = mode;
            v.launch(
                text.clone(),
                attachments.clone(),
                account.clone(),
                None,
                window,
                cx,
            );
        };
        view.update(cx, |v, cx| launch(v, first, window, cx));
        for model in rest {
            let (config, backend) = (self.config.clone(), self.backend.clone());
            let sibling = cx.new(|cx| ThreadView::new(&project, config, backend, window, cx));
            sibling.update(cx, |v, cx| launch(v, model, window, cx));
            self.add_thread_tab(sibling, false, window, cx);
        }
        self.show(Page::Compare(group), window, cx);
    }

    /// The threads of comparison `group` this window knows, oldest first: stored rows, and drafts whose row is
    /// still being saved.
    pub(super) fn group_members(&self, group: i64, cx: &gpui::App) -> Vec<i64> {
        let mut ids: Vec<i64> = self
            .threads
            .iter()
            .filter(|t| t.compare_group == Some(group) && t.archived_at.is_none())
            .map(|t| t.id)
            .collect();
        for v in &self.open_threads {
            if let Some(t) = v
                .read(cx)
                .thread
                .as_ref()
                .filter(|t| t.compare_group == Some(group))
                && !ids.contains(&t.id)
            {
                ids.push(t.id);
            }
        }
        ids.sort_unstable();
        ids
    }

    pub(super) fn open_compare(
        &mut self,
        a: &OpenCompare,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.show(Page::Compare(a.group), window, cx);
    }

    pub(super) fn toggle_compare_group(
        &mut self,
        a: &ToggleCompareGroup,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.compare.open.remove(&a.group) {
            self.compare.open.insert(a.group);
        }
        self.refresh_sidebar(cx);
    }

    /// "Keep this one": archive the group's other threads, or leave them. Asked in a small dialog.
    pub(super) fn keep_compare(
        &mut self,
        a: &KeepCompare,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        use gpui_component::{
            Sizable as _, WindowExt as _,
            button::{Button, ButtonVariants as _},
            dialog::DialogFooter,
        };
        let id = a.id;
        let others = self.compare_others(id, cx);
        if others.iter().any(|&o| self.working(o, cx)) {
            return self.say(
                "Some of these threads are still working. Stop them or wait, then keep one.",
                window,
                cx,
            );
        }
        if others.is_empty() {
            return self.open_thread(&OpenThread { id }, window, cx);
        }
        let ws = cx.entity().downgrade();
        let body: SharedString = format!(
            "Archive the other {} of this comparison, or leave them in the sidebar?",
            if others.len() == 1 {
                "thread".to_owned()
            } else {
                format!("{} threads", others.len())
            }
        )
        .into();
        window.open_dialog(cx, move |dialog, _, _| {
            let choose = |archive: bool| {
                let ws = ws.clone();
                move |_: &gpui::ClickEvent, window: &mut Window, cx: &mut gpui::App| {
                    window.close_dialog(cx);
                    let _ = ws.update(cx, |ws, cx| ws.finish_keep(id, archive, window, cx));
                }
            };
            dialog
                .w(gpui::px(420.))
                .title("Keep this one")
                .child(body.clone())
                .footer(
                    DialogFooter::new()
                        .child(crate::ui::dialog_cancel("keep-cancel"))
                        .child(
                            Button::new("keep-leave")
                                .outline()
                                .small()
                                .label("Leave them")
                                .on_click(choose(false)),
                        )
                        .child(
                            Button::new("keep-archive")
                                .primary()
                                .small()
                                .label("Archive others")
                                .on_click(choose(true)),
                        ),
                )
        });
    }

    /// The other threads of `id`'s comparison group.
    fn compare_others(&self, id: i64, cx: &gpui::App) -> Vec<i64> {
        let group = self
            .threads
            .iter()
            .find(|t| t.id == id)
            .and_then(|t| t.compare_group)
            .or_else(|| self.open_view(id, cx)?.read(cx).compare_group);
        group.map_or_else(Vec::new, |g| {
            self.group_members(g, cx)
                .into_iter()
                .filter(|&o| o != id)
                .collect()
        })
    }

    /// Keeps `id`: archives the group's other threads when `archive`, then opens it.
    pub(super) fn finish_keep(
        &mut self,
        id: i64,
        archive: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if archive {
            for other in self.compare_others(id, cx) {
                self.archive_thread(other, true, window, cx);
            }
        }
        self.open_thread(&OpenThread { id }, window, cx);
    }
}
