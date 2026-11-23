//! The branch picker in a thread's header (T3's branch toolbar): the folder's local branches, a search, and
//! "Create branch" from the typed text. On a local thread a pick switches the folder's checkout; with uncommitted
//! changes it asks first (stash them, or take them along). A new thread set to run in a worktree picks the
//! worktree's base branch instead ("From <branch>"), and nothing in the folder changes.

use super::thread::{ThreadView, ThreadViewEvent};
use crate::ui;
use gpui::{
    AppContext as _, Context, Entity, FontWeight, SharedString, Task, WeakEntity, Window, div,
    prelude::*, px,
};
use gpui_component::{
    ActiveTheme as _, Disableable as _, Sizable as _,
    button::{Button, ButtonVariants as _},
    h_flex,
    input::{Input, InputState},
    v_flex,
};
use skyelab_git::{Branches, OnDirty, Status};
use std::path::PathBuf;

/// The most rows listed at once; the search narrows a longer list.
const MAX_ROWS: usize = 50;

pub struct BranchPicker {
    owner: WeakEntity<ThreadView>,
    pub(super) dir: PathBuf,
    pub(super) open: bool,
    pub(super) branches: Branches,
    query: Entity<InputState>,
    pub(super) error: Option<SharedString>,
    /// A switch the uncommitted changes held back: the branch, and the status the user is asked about.
    pub(super) dirty: Option<(String, Status)>,
    /// The worktree base picked for a new thread (`None`: the current branch).
    pub(super) base: Option<String>,
    busy: bool,
    _task: Option<Task<()>>,
}

impl BranchPicker {
    pub fn new(
        owner: WeakEntity<ThreadView>,
        dir: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let query =
            cx.new(|cx| InputState::new(window, cx).placeholder("Search or create a branch"));
        cx.observe(&query, |_, _, cx| cx.notify()).detach();
        let mut this = Self {
            owner,
            dir,
            open: false,
            branches: Branches::default(),
            query,
            error: None,
            dirty: None,
            base: None,
            busy: false,
            _task: None,
        };
        this.reload(cx);
        this
    }

    pub fn set_open(&mut self, open: bool, cx: &mut Context<Self>) {
        self.open = open;
        if open {
            self.error = None;
            self.reload(cx);
        } else {
            self.dirty = None;
        }
        cx.notify();
    }

    /// Picking sets the worktree base rather than switching: a new thread that will run in its own worktree.
    fn picks_base(&self, cx: &gpui::App) -> bool {
        self.owner.upgrade().is_some_and(|o| {
            let o = o.read(cx);
            o.thread.is_none() && o.wt.use_worktree
        })
    }

    /// The trigger's label: the checkout's branch, or the base a new worktree starts from.
    /// `picks_base` as [`Self::picks_base`] says, passed in since the owner is mid-render when this is asked.
    pub fn label(&self, picks_base: bool) -> String {
        let current = self.branches.current.as_deref().unwrap_or("detached HEAD");
        if picks_base {
            format!("From {}", self.base.as_deref().unwrap_or(current))
        } else {
            current.to_owned()
        }
    }

    fn reload(&mut self, cx: &mut Context<Self>) {
        let dir = self.dir.clone();
        let task = cx.background_spawn(async move { skyelab_git::branches(&dir) });
        self._task = Some(cx.spawn(async move |this, cx| {
            let loaded = task.await;
            let _ = this.update(cx, |this, cx| {
                match loaded {
                    Ok(b) => this.branches = b,
                    // Not a repo: the header shows no picker.
                    Err(_) => this.branches = Branches::default(),
                }
                cx.notify();
            });
        }));
    }

    /// The thread's agent must not be mid-turn while its folder's checkout changes under it.
    fn refuse_while_running(&mut self, cx: &mut Context<Self>) -> bool {
        let running = self
            .owner
            .upgrade()
            .is_some_and(|o| o.read(cx).session.is_some() || o.read(cx).starting);
        if running {
            self.error = Some("Stop the agent before switching branches.".into());
            cx.notify();
        }
        running
    }

    pub(super) fn pick(&mut self, name: String, cx: &mut Context<Self>) {
        if self.picks_base(cx) {
            self.base = (self.branches.current.as_ref() != Some(&name)).then_some(name);
            self.set_open(false, cx);
            return;
        }
        if self.branches.current.as_ref() == Some(&name) {
            self.set_open(false, cx);
            return;
        }
        self.switch(name, OnDirty::Refuse, None, cx);
    }

    /// Runs a switch. With `shown`, the status the user confirmed: the switch is refused, and asked again, if the
    /// folder's changes are no longer exactly those.
    pub(super) fn switch(
        &mut self,
        name: String,
        on_dirty: OnDirty,
        shown: Option<Status>,
        cx: &mut Context<Self>,
    ) {
        if self.busy || self.refuse_while_running(cx) {
            return;
        }
        self.busy = true;
        let dir = self.dir.clone();
        let target = name.clone();
        let task = cx.background_spawn(async move {
            let now = skyelab_git::status(&dir)?;
            if let Some(shown) = shown
                && shown != now
            {
                return Ok(Err(now)); // changed since the user looked
            }
            match skyelab_git::switch_branch(&dir, &target, on_dirty) {
                Err(skyelab_git::Error::Dirty(_)) => Ok(Err(now)),
                other => other.map(Ok),
            }
        });
        self.finish(name, task, cx);
    }

    pub(super) fn create(&mut self, name: String, cx: &mut Context<Self>) {
        if self.busy || self.refuse_while_running(cx) {
            return;
        }
        self.busy = true;
        let dir = self.dir.clone();
        let target = name.clone();
        let task =
            cx.background_spawn(async move { skyelab_git::create_branch(&dir, &target).map(Ok) });
        self.finish(name, task, cx);
    }

    /// `Ok(Err(status))`: held back by these changes, so the user is asked.
    fn finish(
        &mut self,
        name: String,
        task: Task<skyelab_git::Result<Result<(), Status>>>,
        cx: &mut Context<Self>,
    ) {
        self._task = Some(cx.spawn(async move |this, cx| {
            let done = task.await;
            let _ = this.update(cx, |this, cx| {
                this.busy = false;
                match done {
                    Ok(Ok(())) => {
                        this.set_open(false, cx);
                        this.reload(cx);
                        if let Some(owner) = this.owner.upgrade() {
                            owner.update(cx, |_, cx| cx.emit(ThreadViewEvent::FilesChanged));
                        }
                    }
                    Ok(Err(status)) => this.dirty = Some((name, status)),
                    Err(e) => this.error = Some(format!("{e:#}").into()),
                }
                cx.notify();
            });
        }));
    }
}

impl Render for BranchPicker {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let typed = self.query.read(cx).value().trim().to_owned();
        let picks_base = self.picks_base(cx);
        let needle = typed.to_lowercase();
        let rows: Vec<String> = self
            .branches
            .local
            .iter()
            .filter(|b| b.to_lowercase().contains(&needle))
            .take(MAX_ROWS)
            .cloned()
            .collect();
        let exists = self.branches.local.contains(&typed);
        let chosen = if picks_base {
            self.base.clone().or(self.branches.current.clone())
        } else {
            self.branches.current.clone()
        };
        let caption = |text: &'static str| {
            div()
                .px_2p5()
                .pt_1()
                .text_size(ui::META)
                .font_weight(FontWeight::MEDIUM)
                .text_color(theme.muted_foreground)
                .child(text)
        };
        let list = v_flex()
            .gap_0p5()
            .children(rows.into_iter().enumerate().map(|(i, name)| {
                let selected = chosen.as_ref() == Some(&name);
                let current = self.branches.current.as_ref() == Some(&name);
                let picked = name.clone();
                ui::row(("branch", i), selected, theme)
                    .on_click(cx.listener(move |this, _, _, cx| this.pick(picked.clone(), cx)))
                    .child(div().flex_1().min_w_0().truncate().child(name))
                    .when(current, |r| {
                        r.child(
                            div()
                                .text_size(ui::META)
                                .text_color(theme.muted_foreground)
                                .child("current"),
                        )
                    })
            }));
        let dirty = self.dirty.clone().map(|(name, status)| {
            let files = super::worktree::changed_files(&status).len();
            let plural = if files == 1 { "" } else { "s" };
            let (stash_to, carry_to) = ((name.clone(), status.clone()), (name.clone(), status));
            v_flex()
                .id("branch-dirty")
                .p_2()
                .gap_1p5()
                .rounded(ui::RADIUS)
                .border_1()
                .border_color(theme.warning)
                .child(div().text_size(ui::META).child(format!(
                    "{files} uncommitted change{plural}. Switching to {name}:"
                )))
                .child(
                    h_flex()
                        .gap_1()
                        .child(
                            Button::new("branch-stash")
                                .xsmall()
                                .outline()
                                .label("Stash and switch")
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    let (n, s) = stash_to.clone();
                                    this.switch(n, OnDirty::Stash, Some(s), cx)
                                })),
                        )
                        .child(
                            Button::new("branch-carry")
                                .xsmall()
                                .outline()
                                .label("Take changes along")
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    let (n, s) = carry_to.clone();
                                    this.switch(n, OnDirty::Carry, Some(s), cx)
                                })),
                        )
                        .child(
                            Button::new("branch-cancel")
                                .xsmall()
                                .ghost()
                                .label("Cancel")
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.dirty = None;
                                    cx.notify();
                                })),
                        ),
                )
        });
        v_flex()
            .id("branch-picker")
            .w(px(280.))
            .gap_1()
            .child(Input::new(&self.query).small())
            .child(caption(if picks_base {
                "Base for the new worktree"
            } else {
                "Branches"
            }))
            .child(list)
            .when(!picks_base && !typed.is_empty() && !exists, |v| {
                let name = typed.clone();
                v.child(
                    Button::new("branch-create")
                        .xsmall()
                        .ghost()
                        .label(format!("Create branch \"{typed}\""))
                        .disabled(self.busy)
                        .on_click(cx.listener(move |this, _, _, cx| this.create(name.clone(), cx))),
                )
            })
            .children(dirty)
            .children(self.error.clone().map(|e| {
                div()
                    .px_2p5()
                    .text_size(ui::META)
                    .text_color(crate::ui::ink(theme, theme.danger))
                    .child(e)
            }))
    }
}

impl ThreadView {
    /// The header's branch control: a worktree thread's own branch, or the picker on a repo folder.
    pub(super) fn render_branch(&self, cx: &mut Context<Self>) -> Option<gpui::AnyElement> {
        let icon = || {
            gpui_component::Icon::new(gpui_kit_assets::IconName::GitBranch)
                .size_3p5()
                .text_color(cx.theme().muted_foreground)
        };
        if let Some(branch) = self
            .thread
            .as_ref()
            .and_then(|t| t.worktree_path.as_ref().and(t.worktree_branch.clone()))
        {
            return Some(
                h_flex()
                    .id("worktree-branch")
                    .flex_shrink_0()
                    .gap_1()
                    .text_size(ui::META)
                    .text_color(cx.theme().muted_foreground)
                    .child(icon())
                    .child(branch)
                    .into_any_element(),
            );
        }
        let picker = self.branch_picker.read(cx);
        if picker.branches.local.is_empty() && picker.branches.current.is_none() {
            return None; // not a repo (or no commit yet)
        }
        let picks_base = self.thread.is_none() && self.wt.use_worktree;
        let (label, open) = (picker.label(picks_base), picker.open);
        let (toggle, content) = (self.branch_picker.clone(), self.branch_picker.clone());
        Some(
            gpui_component::popover::Popover::new("branch-popover")
                .open(open)
                .on_open_change(move |open, _, cx| toggle.update(cx, |p, cx| p.set_open(*open, cx)))
                .trigger(
                    Button::new("branch-trigger")
                        .ghost()
                        .xsmall()
                        .icon(gpui_component::Icon::new(
                            gpui_kit_assets::IconName::GitBranch,
                        ))
                        .label(label),
                )
                .content(move |_, _, _| content.clone())
                .into_any_element(),
        )
    }
}
