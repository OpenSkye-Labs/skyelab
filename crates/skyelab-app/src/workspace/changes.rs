//! The Changes tab: a folder's changed files and their unified diff against `HEAD`, or against
//! the commit a worktree thread started from.

use super::changes_model::{self, Changes, LineKind, Row};
use super::thread::{ThreadView, ThreadViewEvent};
use gpui::{
    App, Context, Entity, FocusHandle, Focusable, FontWeight, ScrollStrategy, SharedString,
    Subscription, Task, UniformListScrollHandle, Window, actions, div, prelude::*, px,
    uniform_list,
};
use gpui_component::{ActiveTheme as _, Sizable as _, button::Button, h_flex, v_flex};
use skyelab_git::ChangeKind;
use std::ops::Range;
use std::path::PathBuf;

actions!(skyelab, [RefreshChanges]);

const ROW_HEIGHT: f32 = 20.;

pub struct ChangesView {
    focus: FocusHandle,
    pub(super) dir: PathBuf,
    /// A worktree's base commit; `None` diffs against `HEAD`.
    base: Option<String>,
    title: SharedString,
    pub(super) changes: Option<Changes>,
    pub(super) error: Option<SharedString>,
    scroll: UniformListScrollHandle,
    _load: Option<Task<()>>,
    _thread: Option<Subscription>,
}

impl ChangesView {
    /// Shows `dir`'s changes, reloading whenever `thread`'s agent finishes a turn.
    pub fn new(
        dir: PathBuf,
        base: Option<String>,
        title: SharedString,
        thread: Option<&Entity<ThreadView>>,
        cx: &mut Context<Self>,
    ) -> Self {
        let _thread = thread.map(|t| {
            cx.subscribe(t, |this, _, e: &ThreadViewEvent, cx| {
                if matches!(e, ThreadViewEvent::TurnDone) {
                    this.refresh(cx);
                }
            })
        });
        let mut this = Self {
            focus: cx.focus_handle(),
            dir,
            base,
            title,
            changes: None,
            error: None,
            scroll: UniformListScrollHandle::new(),
            _load: None,
            _thread,
        };
        this.refresh(cx);
        this
    }

    pub fn title(&self) -> SharedString {
        format!("Changes · {}", self.title).into()
    }

    /// Re-reads status and diff off the UI thread. A newer refresh drops the older one.
    pub fn refresh(&mut self, cx: &mut Context<Self>) {
        let (dir, base) = (self.dir.clone(), self.base.clone());
        let task = cx.background_spawn(async move { changes_model::load(&dir, base.as_deref()) });
        self._load = Some(cx.spawn(async move |this, cx| {
            let loaded = task.await;
            let _ = this.update(cx, |this, cx| {
                match loaded {
                    Ok(c) => (this.changes, this.error) = (Some(c), None),
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
                        base.px_2()
                            .gap_2()
                            .bg(theme.secondary)
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(kind_badge(f.kind, cx))
                            .child(name)
                    }
                    Row::Hunk(h) => base
                        .px_2()
                        .text_color(theme.muted_foreground)
                        .bg(theme.muted)
                        .child(h.clone()),
                    Row::Placeholder(p) => base
                        .px_2()
                        .italic()
                        .text_color(theme.muted_foreground)
                        .child(*p),
                    Row::Line {
                        old,
                        new,
                        kind,
                        text,
                    } => {
                        let (sign, bg) = match kind {
                            LineKind::Context => (" ", None),
                            LineKind::Added => ("+", Some(theme.success.opacity(0.15))),
                            LineKind::Removed => ("-", Some(theme.danger.opacity(0.15))),
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

    fn render_files(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let files = self
            .changes
            .as_ref()
            .map(|c| c.files.as_slice())
            .unwrap_or_default();
        v_flex()
            .id("changed-files")
            .w(px(240.))
            .flex_none()
            .h_full()
            .overflow_y_scroll()
            .border_r_1()
            .border_color(theme.border)
            .py_1()
            .children(files.iter().map(|f| {
                let row = f.first_row;
                h_flex()
                    .id(SharedString::from(format!("file-{}", f.path.display())))
                    .px_2()
                    .py_0p5()
                    .gap_2()
                    .text_xs()
                    .cursor_pointer()
                    .hover(|s| s.bg(theme.secondary))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.scroll.scroll_to_item(row, ScrollStrategy::Top);
                        cx.notify();
                    }))
                    .child(kind_badge(f.kind, cx))
                    .child(div().truncate().child(f.path.display().to_string()))
            }))
    }
}

fn kind_badge(kind: ChangeKind, cx: &App) -> impl IntoElement {
    let theme = cx.theme();
    let (letter, color) = match kind {
        ChangeKind::Added => ("A", theme.success),
        ChangeKind::Deleted => ("D", theme.danger),
        ChangeKind::Renamed => ("R", theme.info),
        ChangeKind::Copied => ("C", theme.info),
        ChangeKind::Modified | ChangeKind::TypeChanged => ("M", theme.warning),
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
        let files = self.render_files(cx).into_any_element();
        let theme = cx.theme();
        let count = self.changes.as_ref().map(|c| c.rows.len()).unwrap_or(0);
        let empty = self.changes.as_ref().is_some_and(|c| c.files.is_empty());
        let header = h_flex()
            .px_4()
            .py_2()
            .gap_3()
            .border_b_1()
            .border_color(theme.border)
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(self.title()),
                    )
                    .child(
                        div()
                            .text_xs()
                            .truncate()
                            .text_color(theme.muted_foreground)
                            .child(self.dir.display().to_string()),
                    ),
            )
            .child(
                Button::new("refresh-changes")
                    .outline()
                    .xsmall()
                    .label("Refresh")
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.on_refresh(&RefreshChanges, window, cx)
                    })),
            );
        let body = if let Some(e) = &self.error {
            div()
                .p_4()
                .text_sm()
                .text_color(theme.danger)
                .child(e.clone())
                .into_any_element()
        } else if empty {
            div()
                .p_4()
                .text_sm()
                .text_color(theme.muted_foreground)
                .child(if self.base.is_some() {
                    "No changes since this worktree was created."
                } else {
                    "No changes against HEAD."
                })
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
            .child(body)
    }
}
