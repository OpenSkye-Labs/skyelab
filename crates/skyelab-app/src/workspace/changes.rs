//! The Changes tab: a folder's changed files and their unified diff against `HEAD`, or against
//! the commit a worktree thread started from.

use super::changes_model::{self, Changes, LineKind, Row, Scope};
use super::thread::ThreadView;
use crate::ui;
use gpui::{
    App, Context, Entity, FocusHandle, Focusable, FontWeight, ScrollStrategy, SharedString, Task,
    UniformListScrollHandle, Window, actions, div, prelude::*, px, uniform_list,
};
use gpui_component::{
    ActiveTheme as _, IconName, Sizable as _,
    button::{Button, ButtonVariants as _},
    checkbox::Checkbox,
    h_flex,
    input::{InputState, TextareaState},
    v_flex,
};
use skyelab_git::ChangeKind;
use std::collections::HashSet;
use std::ops::Range;
use std::path::PathBuf;

actions!(skyelab, [RefreshChanges]);

const ROW_HEIGHT: f32 = 20.;
/// The file list's rows, T3's 28.
const FILE_ROW_HEIGHT: f32 = 28.;

pub struct ChangesView {
    focus: FocusHandle,
    pub(super) dir: PathBuf,
    /// `None` diffs against `HEAD`.
    pub(super) base: Option<String>,
    title: SharedString,
    pub(super) changes: Option<Changes>,
    pub(super) error: Option<SharedString>,
    scroll: UniformListScrollHandle,
    /// The thread whose account commits, pushes and opens PRs here.
    pub(super) thread: Option<gpui::WeakEntity<ThreadView>>,
    pub(super) commit_message: Entity<InputState>,
    pub(super) pr_title: Entity<InputState>,
    pub(super) pr_body: Entity<TextareaState>,
    /// Files ticked for the next commit; none ticked commits everything.
    pub(super) selected: HashSet<PathBuf>,
    pub(super) publish_error: Option<SharedString>,
    pub(super) publish_note: Option<SharedString>,
    /// A commit, push or PR is running; another click is ignored.
    pub(super) busy: bool,
    pub(super) _publish: Option<Task<()>>,
    /// Working tree, the whole thread, or one turn.
    pub(super) scope: Scope,
    /// A rewind shown for confirming.
    pub(super) rewind: Option<super::changes_turns::PendingRewind>,
    _load: Option<Task<()>>,
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
        let commit_message = input("Commit message", window, cx);
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
            scroll: UniformListScrollHandle::new(),
            thread: thread.map(|t| t.downgrade()),
            commit_message,
            pr_title,
            pr_body,
            selected: HashSet::new(),
            publish_error: None,
            publish_note: None,
            busy: false,
            _publish: None,
            scope: Scope::Working,
            rewind: None,
            _load: None,
        };
        this.refresh(cx);
        this
    }

    pub fn title(&self) -> SharedString {
        format!("Changes · {}", self.title).into()
    }

    /// Re-reads status and diff off the UI thread. A newer refresh drops the older one.
    pub fn refresh(&mut self, cx: &mut Context<Self>) {
        let (dir, base, scope) = (self.dir.clone(), self.base.clone(), self.scope);
        let key = self.checkpoint_key(cx);
        let task = cx.background_spawn(async move {
            changes_model::load_scope(&dir, base.as_deref(), scope, key.as_deref())
        });
        self._load = Some(cx.spawn(async move |this, cx| {
            let loaded = task.await;
            let _ = this.update(cx, |this, cx| {
                match loaded {
                    Ok(c) => {
                        // Ticks are for the working tree's commit: a turn's list must not clear them.
                        if this.scope == Scope::Working {
                            this.selected
                                .retain(|p| c.files.iter().any(|f| &f.path == p));
                        }
                        (this.changes, this.error) = (Some(c), None);
                    }
                    Err(e) => this.error = Some(format!("{e:#}").into()),
                }
                cx.notify();
            });
        }));
    }

    fn on_refresh(&mut self, _: &RefreshChanges, _: &mut Window, cx: &mut Context<Self>) {
        self.refresh(cx);
    }

    fn render_rows(
        &mut self,
        range: Range<usize>,
        cx: &mut Context<Self>,
    ) -> Vec<gpui::AnyElement> {
        let theme = cx.theme();
        let Some(changes) = &self.changes else {
            return Vec::new();
        };
        let num = |n: Option<u32>| {
            div()
                .w(px(44.))
                .flex_none()
                .text_right()
                .pr_2()
                .text_color(theme.muted_foreground)
                .child(n.map(|n| n.to_string()).unwrap_or_default())
        };
        range
            .filter_map(|ix| changes.rows.get(ix).map(|r| (ix, r)))
            .map(|(ix, row)| {
                let base = h_flex()
                    .id(("diff-row", ix))
                    .h(px(ROW_HEIGHT))
                    .w_full()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .font_family(theme.mono_font_family.clone())
                    .text_xs();
                match row {
                    Row::File(i) => {
                        let f = &changes.files[*i];
                        let name = match &f.old_path {
                            Some(old) => format!("{} → {}", old.display(), f.path.display()),
                            None => f.path.display().to_string(),
                        };
                        // T3's file header: sans 12, medium, on the muted surface with a hairline.
                        base.px_3()
                            .gap_2()
                            .bg(theme.muted)
                            .border_t_1()
                            .border_color(theme.border)
                            .font_family(theme.font_family.clone())
                            .font_weight(FontWeight::MEDIUM)
                            .child(kind_badge(f.kind, cx))
                            .child(name)
                    }
                    Row::Hunk(h) => base
                        .px_3()
                        .font_family(theme.font_family.clone())
                        .text_size(px(11.))
                        .text_color(theme.muted_foreground.opacity(0.7))
                        .child(h.clone()),
                    Row::Placeholder(p) => base
                        .px_2()
                        .italic()
                        .text_color(theme.muted_foreground)
                        .child(*p),
                    Row::TooLarge(n) => base
                        .px_2()
                        .italic()
                        .text_color(theme.muted_foreground)
                        .child(format!(
                            "Diff too large: {n} lines. Open it in a terminal or editor."
                        )),
                    Row::Line {
                        old,
                        new,
                        kind,
                        text,
                    } => {
                        let (sign, bg) = match kind {
                            LineKind::Context => (" ", None),
                            LineKind::Added => ("+", Some(theme.success.opacity(0.12))),
                            LineKind::Removed => ("-", Some(theme.danger.opacity(0.12))),
                        };
                        base.when_some(bg, |b, bg| b.bg(bg))
                            .child(num(*old))
                            .child(num(*new))
                            .child(div().w(px(14.)).flex_none().child(sign))
                            .child(text.clone())
                    }
                }
                .into_any_element()
            })
            .collect()
    }

    /// The file sidebar's visible rows; a click scrolls the diff to that file.
    fn render_files(
        &mut self,
        range: Range<usize>,
        cx: &mut Context<Self>,
    ) -> Vec<gpui::AnyElement> {
        let theme = cx.theme();
        let Some(changes) = &self.changes else {
            return Vec::new();
        };
        range
            .filter_map(|ix| changes.files.get(ix).map(|f| (ix, f)))
            .map(|(ix, f)| {
                #[cfg(test)]
                FILE_ROWS_BUILT.with(|n| n.set(n.get() + 1));
                let row = f.first_row;
                let path = f.path.clone();
                let ticked = self.selected.contains(&f.path);
                let tick = Checkbox::new(SharedString::from(format!("pick-{}", f.path.display())))
                    .xsmall()
                    .checked(ticked)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if !this.selected.remove(&path) {
                            this.selected.insert(path.clone());
                        }
                        cx.notify();
                    }));
                h_flex()
                    .id(("file", ix))
                    .w_full()
                    .h(px(FILE_ROW_HEIGHT))
                    .px_2()
                    .gap_2()
                    .text_size(ui::META)
                    .rounded(ui::RADIUS_MD)
                    .cursor_pointer()
                    .hover(|s| s.bg(theme.accent))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.scroll.scroll_to_item(row, ScrollStrategy::Top);
                        cx.notify();
                    }))
                    .child(tick)
                    .child(kind_badge(f.kind, cx))
                    .child(div().truncate().child(f.path.display().to_string()))
                    .into_any_element()
            })
            .collect()
    }
}

#[cfg(test)]
thread_local! {
    static FILE_ROWS_BUILT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// How many file sidebar rows have been built on this thread, so tests can check only visible ones are.
#[cfg(test)]
pub(super) fn file_rows_built() -> usize {
    FILE_ROWS_BUILT.with(|n| n.get())
}

fn kind_badge(kind: ChangeKind, cx: &App) -> impl IntoElement {
    let theme = cx.theme();
    let (letter, color) = match kind {
        ChangeKind::Added => ("A", theme.success),
        ChangeKind::Deleted => ("D", theme.danger),
        ChangeKind::Renamed => ("R", theme.info),
        ChangeKind::Copied => ("C", theme.info),
        // T3 gives modified files a muted blue so added and deleted ones lead.
        ChangeKind::Modified | ChangeKind::TypeChanged => ("M", theme.info.opacity(0.75)),
        ChangeKind::Unmerged => ("U", theme.danger),
    };
    div()
        .w(px(12.))
        .flex_none()
        .font_weight(FontWeight::BOLD)
        .text_color(color)
        .child(letter)
}

impl Focusable for ChangesView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for ChangesView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let file_count = self.changes.as_ref().map_or(0, |c| c.files.len());
        let files = uniform_list(
            "changed-files",
            file_count,
            cx.processor(|this, range, _, cx| this.render_files(range, cx)),
        )
        .w(px(200.))
        .flex_none()
        .h_full()
        .border_r_1()
        .border_color(cx.theme().border)
        .py_1()
        .into_any_element();
        let theme = cx.theme();
        let count = self.changes.as_ref().map(|c| c.rows.len()).unwrap_or(0);
        let empty = self.changes.as_ref().is_some_and(|c| c.files.is_empty());
        let header = ui::header(theme)
            .h(px(40.))
            .px_4()
            .child(
                h_flex()
                    .flex_1()
                    .min_w_0()
                    .gap_2()
                    .child(
                        div()
                            .flex_shrink_0()
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
            .child(
                Button::new("refresh-changes")
                    .ghost()
                    .xsmall()
                    .icon(IconName::RefreshCw)
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
                    uniform_list(
                        "diff-rows",
                        count,
                        cx.processor(|this, range, _, cx| this.render_rows(range, cx)),
                    )
                    .track_scroll(&self.scroll)
                    .flex_1()
                    .h_full(),
                )
                .into_any_element()
        };
        v_flex()
            .id("changes")
            .key_context("Changes")
            .track_focus(&self.focus)
            .on_action(cx.listener(Self::on_refresh))
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
            .child(body)
    }
}
