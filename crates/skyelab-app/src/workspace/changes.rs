//! The Changes tab: a folder's changed files (as a folder tree) and their diff against `HEAD`, or against the
//! commit a worktree thread started from; unified or split, clipped or wrapped, with find (T3's DiffPanel).

#[cfg(test)]
#[path = "diff_comment_view_test.rs"]
mod comment_view_test;
#[path = "changes_comments.rs"]
mod comments;
#[path = "changes_find.rs"]
mod find;
#[path = "changes_rows.rs"]
mod rows;
#[cfg(test)]
#[path = "diff_comment_split_click_test.rs"]
mod split_click_test;

use super::changes_model::{self, Changes, Scope};
use super::changes_tree::{self, SplitRow, TreeRow};
use super::thread::ThreadView;
use crate::ui;
use crate::ui::Button;
use crate::ui::SkyeIcon;
use gpui::{
    App, Context, Entity, FocusHandle, Focusable, FontWeight, ListAlignment, ListOffset, ListState,
    MouseButton, SharedString, Task, Window, actions, div, list, prelude::*, px, uniform_list,
};
use gpui_component::{
    ActiveTheme as _, h_flex,
    input::{InputState, TextareaState},
    v_flex,
};
use std::collections::HashSet;
use std::path::PathBuf;

pub(super) use find::bind_keys;
pub(super) use rows::diff_line;
#[cfg(test)]
pub(super) use rows::file_rows_built;

actions!(skyelab, [RefreshChanges]);

const CONTEXT: &str = "Changes";
/// A diff row's height while lines are clipped (wrapping measures each row).
const ROW_HEIGHT: f32 = 20.;

pub struct ChangesView {
    focus: FocusHandle,
    pub(super) dir: PathBuf,
    /// `None` diffs against `HEAD`.
    pub(super) base: Option<String>,
    pub(super) title: SharedString,
    pub(super) changes: Option<Changes>,
    pub(super) error: Option<SharedString>,
    /// The diff's rows (unified) or `split` (side by side).
    pub(super) list: ListState,
    /// The file tree's visible rows, rebuilt when the diff loads or a folder folds.
    pub(super) tree: Vec<TreeRow>,
    /// Folded folders, by repo-relative path; kept across reloads.
    collapsed: HashSet<PathBuf>,
    /// The rows side by side, when `prefs.split`.
    pub(super) split: Vec<SplitRow>,
    pub(super) prefs: skyelab_core::DiffSettings,
    pub(super) find: super::thread_find::Find,
    /// The thread whose account commits, pushes and opens PRs here.
    pub(super) thread: Option<gpui::WeakEntity<ThreadView>>,
    /// A subject, then a blank line and a body when there is one.
    pub(super) commit_message: Entity<TextareaState>,
    pub(super) pr_title: Entity<InputState>,
    pub(super) pr_body: Entity<TextareaState>,
    /// Files ticked for the next commit; none ticked commits everything.
    pub(super) selected: HashSet<PathBuf>,
    /// The file last jumped to (sidebar click or a terminal link), highlighted in the sidebar.
    pub(super) active_file: Option<PathBuf>,
    /// A file to scroll to once the diff loads (a timeline path opened this tab).
    pub(super) reveal_after_load: Option<PathBuf>,
    pub(super) publish_error: Option<SharedString>,
    pub(super) publish_note: Option<SharedString>,
    /// A commit, push or PR is running; another click is ignored.
    pub(super) busy: bool,
    pub(super) _publish: Option<Task<()>>,
    /// A commit message or PR being generated (Changes → sparkle).
    pub(super) generating: Option<super::changes_generate::Gen>,
    pub(super) _generate: Option<Task<()>>,
    /// Working tree, the whole thread, or one turn.
    pub(super) scope: Scope,
    /// A rewind shown for confirming.
    pub(super) rewind: Option<super::changes_turns::PendingRewind>,
    pub(super) drag: Option<comments::Drag>,
    /// The comment being written (`changes_comments.rs`).
    pub(super) draft: Option<comments::Draft>,
    /// Comments or a draft show, so rows are measured.
    pub(super) annotated: bool,
    _load: Option<Task<()>>,
    /// Reloads when the folder changes on disk (`folder_watch`).
    _watch: Option<Task<()>>,
}

impl ChangesView {
    pub fn new(
        dir: PathBuf,
        base: Option<String>,
        title: SharedString,
        thread: Option<&Entity<ThreadView>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let input = |placeholder: &'static str, window: &mut Window, cx: &mut Context<Self>| {
            cx.new(|cx| InputState::new(window, cx).placeholder(placeholder))
        };
        let commit_message = cx.new(|cx| {
            TextareaState::new(window, cx)
                .auto_grow(1, 6)
                .placeholder("Commit message")
        });
        let pr_title = input("Pull request title", window, cx);
        let pr_body = cx.new(|cx| {
            TextareaState::new(window, cx)
                .auto_grow(2, 6)
                .placeholder("Pull request description (optional)")
        });
        let mut this = Self {
            focus: cx.focus_handle(),
            dir,
            base,
            title,
            changes: None,
            error: None,
            list: ListState::new(0, ListAlignment::Top, px(400.)),
            tree: Vec::new(),
            collapsed: HashSet::new(),
            split: Vec::new(),
            prefs: super::thread_extras::prefs::get_diff(cx),
            find: Default::default(),
            thread: thread.map(|t| t.downgrade()),
            commit_message,
            pr_title,
            pr_body,
            selected: HashSet::new(),
            active_file: None,
            reveal_after_load: None,
            publish_error: None,
            publish_note: None,
            busy: false,
            _publish: None,
            generating: None,
            _generate: None,
            scope: Scope::Working,
            rewind: None,
            drag: None,
            draft: None,
            annotated: false,
            _load: None,
            _watch: None,
        };
        this._watch = super::folder_watch::watch(&this.dir, cx, Self::reload_if_changed);
        this.refresh(cx);
        this
    }

    pub fn title(&self) -> SharedString {
        format!("Changes · {}", self.title).into()
    }

    /// Re-reads status and diff off the UI thread. A newer refresh drops the older one.
    pub fn refresh(&mut self, cx: &mut Context<Self>) {
        self.load(false, cx);
    }

    /// The folder changed on disk (a folder watch): like [`Self::refresh`], but a read that finds the diff shown
    /// changes nothing and draws nothing.
    pub(super) fn reload_if_changed(&mut self, cx: &mut Context<Self>) {
        self.load(true, cx);
    }

    fn load(&mut self, skip_same: bool, cx: &mut Context<Self>) {
        let (dir, base, scope) = (self.dir.clone(), self.base.clone(), self.scope);
        let (key, ws) = (self.checkpoint_key(cx), self.prefs.ignore_whitespace);
        let task = cx.background_spawn(async move {
            changes_model::load_scope(&dir, base.as_deref(), scope, key.as_deref(), ws)
        });
        self._load = Some(cx.spawn(async move |this, cx| {
            let loaded = task.await;
            let _ = this.update(cx, |this, cx| {
                match loaded {
                    // A reload that found what is shown (an edit to an ignored file, a watch firing twice) draws nothing.
                    Ok(c)
                        if skip_same
                            && this.error.is_none()
                            && this.reveal_after_load.is_none()
                            && this.changes.as_ref() == Some(&c) =>
                    {
                        return;
                    }
                    Ok(c) => {
                        // Ticks are for the working tree's commit: a turn's list must not clear them.
                        if this.scope == Scope::Working {
                            this.selected
                                .retain(|p| c.files.iter().any(|f| &f.path == p));
                        }
                        this.reanchor_comments(&c, cx);
                        this.set_loaded(c);
                        if let Some(path) = this.reveal_after_load.take() {
                            this.reveal(&path, cx);
                        }
                    }
                    Err(e) => {
                        this.reveal_after_load = None;
                        this.error = Some(format!("{e:#}").into());
                    }
                }
                cx.notify();
            });
        }));
    }

    /// Scrolls the diff to `path` (absolute) when this tab lists it; `false` when it doesn't.
    pub(super) fn reveal(&mut self, path: &std::path::Path, cx: &mut Context<Self>) -> bool {
        // The listed paths are relative to the repo root, which can be above the project folder.
        let Some(root) = self.dir.ancestors().find(|a| a.join(".git").exists()) else {
            return false;
        };
        let Ok(rel) = path.strip_prefix(root) else {
            return false;
        };
        let Some(f) = self
            .changes
            .as_ref()
            .and_then(|c| c.files.iter().find(|f| f.path == rel))
        else {
            return false;
        };
        let (row, path) = (f.first_row, f.path.clone());
        self.scroll_to_row(row);
        self.active_file = Some(path);
        cx.notify();
        true
    }

    /// Shows a freshly loaded diff: rebuilds the tree and the layout, and counts an open find again.
    pub(super) fn set_loaded(&mut self, changes: Changes) {
        // A reload (a turn ended, a commit) keeps the reader's place.
        let top = self.top_row();
        (self.changes, self.error) = (Some(changes), None);
        self.rebuild_tree();
        if self.find.input.is_some() {
            self.recount(self.find.query.clone());
        }
        self.relayout();
        let rows = self.changes.as_ref().map_or(0, |c| c.rows.len());
        if top > 0 && top < rows {
            self.scroll_to_row(top);
        }
    }

    /// The unified row at the top of the diff now, in either layout.
    pub(super) fn top_row(&self) -> usize {
        let ix = self.list.logical_scroll_top().item_ix;
        match self.split.get(ix) {
            Some(SplitRow::Full(r)) if self.prefs.split => *r,
            Some(SplitRow::Pair(l, r)) if self.prefs.split => l.or(*r).unwrap_or(0),
            _ if self.prefs.split => 0,
            _ => ix,
        }
    }

    fn rebuild_tree(&mut self) {
        let files = self.changes.as_ref().map_or(&[][..], |c| &c.files[..]);
        self.tree = changes_tree::tree_rows(files, &self.collapsed);
    }

    /// Recomputes the split rows and resets the list for the current layout (clipped rows share one height;
    /// wrapped or commented ones are measured).
    pub(super) fn relayout(&mut self) {
        let rows = self.changes.as_ref().map_or(&[][..], |c| &c.rows[..]);
        self.split = if self.prefs.split {
            changes_tree::split_rows(rows)
        } else {
            Vec::new()
        };
        let n = if self.prefs.split {
            self.split.len()
        } else {
            rows.len()
        };
        if self.prefs.wrap || self.annotated {
            self.list.reset(n);
        } else {
            self.list.reset_with_uniform_height(n, px(ROW_HEIGHT));
        }
    }

    /// Scrolls the diff so row `row` (of the unified rows) is at the top, in either layout.
    pub(super) fn scroll_to_row(&mut self, row: usize) {
        let item_ix = if self.prefs.split {
            self.split.iter().position(|s| s.shows(row)).unwrap_or(0)
        } else {
            row
        };
        self.list.scroll_to(ListOffset {
            item_ix,
            offset_in_item: px(0.),
        });
    }

    fn toggle_folder(&mut self, path: &std::path::Path, cx: &mut Context<Self>) {
        if !self.collapsed.remove(path) {
            self.collapsed.insert(path.to_path_buf());
        }
        self.rebuild_tree();
        cx.notify();
    }

    fn on_refresh(&mut self, _: &RefreshChanges, _: &mut Window, cx: &mut Context<Self>) {
        self.refresh(cx);
    }
}

impl Focusable for ChangesView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for ChangesView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let _render = crate::debug_renders::count("ChangesView");
        let files = uniform_list(
            "changed-files",
            self.tree.len(),
            cx.processor(|this, range, _, cx| this.render_files(range, cx)),
        )
        .w(px(220.))
        .flex_none()
        .h_full()
        .border_r_1()
        .border_color(cx.theme().border)
        .py_1()
        .into_any_element();
        let (toggles, find_bar) = (self.render_toggles(cx), self.render_find(cx));
        let theme = cx.theme();
        let empty = self.changes.as_ref().is_some_and(|c| c.files.is_empty());
        let header = ui::header(theme)
            .h(px(40.))
            .px_4()
            .child(
                h_flex()
                    .flex_1()
                    .min_w_0()
                    .gap_2()
                    // The title keeps its room up to 60%; the folder path gives way first.
                    .child(
                        div()
                            .flex_shrink_0()
                            .max_w(gpui::relative(0.6))
                            .truncate()
                            .text_size(ui::BODY)
                            .font_weight(FontWeight::MEDIUM)
                            .child(self.title()),
                    )
                    .child(
                        div()
                            .min_w_0()
                            .truncate()
                            .text_size(ui::META)
                            .text_color(theme.muted_foreground)
                            .child(ui::short_path(&self.dir, dirs::home_dir().as_deref())),
                    ),
            )
            .child(div().flex_none().child(toggles))
            .child(
                Button::new("refresh-changes")
                    .ghost()
                    .xsmall()
                    .icon(SkyeIcon::RefreshCw)
                    .label("Refresh")
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.on_refresh(&RefreshChanges, window, cx)
                    })),
            );
        let body = if let Some(e) = &self.error {
            div()
                .p_4()
                .text_size(ui::BODY)
                .text_color(crate::ui::ink(theme, theme.danger))
                .child(e.clone())
                .into_any_element()
        } else if empty {
            div()
                .flex_1()
                .flex()
                .items_center()
                .justify_center()
                .child(
                    ui::EmptyState::new("Nothing to review")
                        .hint(match self.scope {
                            Scope::Turn(_) => "This turn changed no files.",
                            Scope::SinceStart => "No changes since the thread started.",
                            Scope::Working if self.base.is_some() => {
                                "No changes since this worktree was created."
                            }
                            Scope::Working => "No changes against HEAD.",
                        })
                        .page(),
                )
                .into_any_element()
        } else {
            h_flex()
                .flex_1()
                .min_h_0()
                .items_start()
                .child(files)
                .child(
                    div()
                        .relative()
                        .flex_1()
                        .h_full()
                        .min_w_0()
                        // A click in the diff focuses the tab, so mod+F finds in it.
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|this, _, window, cx| this.focus.focus(window, cx)),
                        )
                        .on_mouse_up(
                            MouseButton::Left,
                            cx.listener(|this, _, window, cx| this.finish_drag(window, cx)),
                        )
                        .child(list(self.list.clone(), cx.processor(Self::render_item)).size_full())
                        .children(find_bar),
                )
                .into_any_element()
        };
        v_flex()
            .id("changes")
            .key_context(CONTEXT)
            .track_focus(&self.focus)
            .on_action(cx.listener(Self::on_refresh))
            .on_action(cx.listener(Self::find_in_changes))
            .on_action(cx.listener(Self::find_next))
            .on_action(cx.listener(Self::find_previous))
            .on_action(cx.listener(Self::close_find))
            .size_full()
            .bg(theme.background)
            .child(header)
            .children(self.render_scopes(cx))
            .children(self.render_rewind_confirm(cx))
            .when(self.scope == Scope::Working, |v| {
                v.child(self.render_publish(cx))
            })
            .when(self.scope != Scope::Working, |v| {
                v.children(self.publish_error.clone().map(|e| {
                    div()
                        .px_4()
                        .py_1()
                        .text_xs()
                        .text_color(crate::ui::ink(cx.theme(), cx.theme().danger))
                        .child(e)
                }))
                .children(self.publish_note.clone().map(|n| {
                    div()
                        .px_4()
                        .py_1()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(n)
                }))
            })
            .children(self.render_comments_bar(cx))
            .child(body)
    }
}
