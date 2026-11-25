//! Comments on the Changes tab's lines (T3's `DiffCommentAnnotation`): a click on a line's gutter, or a drag across
//! gutters, opens a draft under the lines; saved comments show under theirs; "Send N comments" puts them in the
//! thread's composer as one chip. They live on the thread view (`Composer::review`), in memory only.

use super::ChangesView;
use crate::ui;
use crate::ui::Button;
use crate::ui::SkyeIcon;
use crate::workspace::diff_comments::{DiffComment, DiffComments};
use crate::workspace::thread::composer::composer_context::ChipKind;
use gpui::{
    AnyElement, App, Context, Entity, FontWeight, MouseButton, MouseMoveEvent, Subscription,
    Window, div, prelude::*, px,
};
use gpui_component::{
    ActiveTheme as _, Icon, Sizable as _, h_flex,
    input::{InputEvent, Textarea, TextareaState},
    v_flex,
};

/// The editor under the lines being commented on. `anchor` holds them as an unsaved comment, so a reload moves
/// the draft with its lines or drops it with them.
pub(in crate::workspace) struct Draft {
    pub(super) anchor: DiffComments,
    pub(in crate::workspace) input: Entity<TextareaState>,
    _sub: Subscription,
}

/// A gutter drag in progress, in unified rows. `left`: it started on the split view's old-line gutter, so it stays
/// on that side.
#[derive(Clone, Copy)]
pub(in crate::workspace) struct Drag {
    start: usize,
    end: usize,
    left: bool,
}

impl Draft {
    /// The unified rows it covers.
    pub(in crate::workspace) fn rows(&self) -> std::ops::Range<usize> {
        self.anchor.items.first().map_or(0..0, |c| c.rows.clone())
    }
}

impl ChangesView {
    /// The comments: a PR tab's own pending review, else the thread's; `None` when there is nowhere to send them.
    pub(super) fn review<'a>(&'a self, cx: &'a App) -> Option<&'a DiffComments> {
        if let Some(pr) = &self.pr {
            return Some(&pr.review);
        }
        let thread = self.thread.as_ref()?.upgrade()?;
        Some(&thread.read(cx).composer.review)
    }

    pub(super) fn update_review<R>(
        &mut self,
        cx: &mut Context<Self>,
        f: impl FnOnce(&mut DiffComments) -> R,
    ) -> Option<R> {
        if let Some(pr) = &mut self.pr {
            return Some(f(&mut pr.review));
        }
        let thread = self.thread.as_ref()?.upgrade()?;
        Some(thread.update(cx, |t, _| f(&mut t.composer.review)))
    }

    /// Comments or a draft show: rows are measured then, since an annotated row is taller.
    fn has_annotations(&self, cx: &App) -> bool {
        self.draft.is_some() || self.review(cx).is_some_and(|r| r.count(self.scope) > 0)
    }

    /// Lays the diff out again after comments changed, keeping the reader's place.
    pub(super) fn comments_changed(&mut self, cx: &mut Context<Self>) {
        self.annotated = self.has_annotations(cx);
        let top = self.top_row();
        self.relayout();
        self.scroll_to_row(top);
        cx.notify();
    }

    /// A freshly loaded diff: the thread's comments and the draft find their lines in it, or are dropped.
    pub(in crate::workspace) fn reanchor_comments(
        &mut self,
        changes: &super::changes_model::Changes,
        cx: &mut Context<Self>,
    ) {
        let scope = self.scope;
        self.update_review(cx, |r| r.reanchor(changes, scope));
        if let Some(d) = &mut self.draft {
            d.anchor.reanchor(changes, scope);
            if d.anchor.items.first().is_none_or(|c| c.scope != scope) {
                self.draft = None;
            }
        }
        self.annotated = self.has_annotations(cx);
    }

    /// A press on unified row `row`'s gutter (`left`: the split view's old side) starts picking lines.
    pub(super) fn gutter_down(&mut self, row: usize, left: bool, cx: &mut Context<Self>) {
        if self.commentable() {
            let (start, end) = (row, row);
            self.drag = Some(Drag { start, end, left });
            cx.notify();
        }
    }

    /// The pointer moved over an item showing unified rows `old` and `new` (the same row outside a split pair): a
    /// held drag reaches the one on its side; a drag whose button was let go elsewhere ends where it got to.
    pub(super) fn drag_over(
        &mut self,
        (old, new): (Option<usize>, Option<usize>),
        e: &MouseMoveEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(drag) = self.drag else {
            return;
        };
        if e.pressed_button != Some(MouseButton::Left) {
            return self.finish_drag(window, cx);
        }
        let row = if drag.left { old.or(new) } else { new.or(old) };
        if let Some(row) = row.filter(|&r| r != drag.end) {
            self.drag = Some(Drag { end: row, ..drag });
            cx.notify();
        }
    }

    /// The button came up: open a draft on the picked lines.
    pub(super) fn finish_drag(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(d) = self.drag.take() {
            self.open_draft(d.start, d.end, window, cx);
        }
    }

    /// Opens the editor under rows `from..=to` (cut at the end of `from`'s file); rows without lines open nothing.
    pub(in crate::workspace) fn open_draft(
        &mut self,
        from: usize,
        to: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(changes) = &self.changes else {
            return;
        };
        let mut anchor = DiffComments::default();
        if anchor.add(changes, self.scope, from, to, "").is_none() {
            return cx.notify();
        }
        let input = cx.new(|cx| {
            TextareaState::new(window, cx)
                .auto_grow(2, 6)
                .placeholder("Add a comment…")
        });
        let sub = cx.subscribe_in(&input, window, |this, _, e: &InputEvent, window, cx| {
            if let InputEvent::PressEnter {
                secondary: true, ..
            } = e
            {
                this.save_draft(window, cx);
            }
        });
        input.update(cx, |i, cx| i.focus(window, cx));
        self.draft = Some(Draft {
            anchor,
            input,
            _sub: sub,
        });
        self.comments_changed(cx);
    }

    /// Keeps the draft's text as a comment on its lines. Empty text keeps the draft open.
    pub(in crate::workspace) fn save_draft(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(d) = &self.draft else {
            return;
        };
        let note = d.input.read(cx).value().trim().to_owned();
        let rows = d.rows();
        if note.is_empty() || rows.is_empty() {
            return;
        }
        let (scope, Some(changes)) = (self.scope, self.changes.clone()) else {
            return;
        };
        self.update_review(cx, |r| {
            r.add(&changes, scope, rows.start, rows.end - 1, &note)
        });
        self.draft = None;
        self.focus.focus(window, cx);
        self.comments_changed(cx);
    }

    fn cancel_draft(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.draft = None;
        self.focus.focus(window, cx);
        self.comments_changed(cx);
    }

    /// Lines can be commented: there is a thread to send the comments to, or a pull request to review.
    fn commentable(&self) -> bool {
        self.thread.is_some() || self.pr.is_some()
    }

    /// "Send N comments": this diff's comments go to the thread's composer as one chip, and leave the diff. On a PR
    /// tab they go to GitHub as one review instead, through the PR actions dialog.
    pub(in crate::workspace) fn send_comments(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.pr.is_some() {
            self.submit_review(window, cx);
            return;
        }
        let Some(thread) = self.thread.as_ref().and_then(|t| t.upgrade()) else {
            return;
        };
        let scope = self.scope;
        let refused = thread.update(cx, |t, cx| {
            let n = t.composer.review.count(scope);
            if n == 0 {
                return None;
            }
            if t.read_only {
                return Some("This thread can't take a message right now.");
            }
            let Some(text) = t.composer.review.chip_text(scope) else {
                return Some(
                    "These comments are too long for one message: send or clear some first.",
                );
            };
            let label = match n {
                1 => "1 diff comment".to_owned(),
                n => format!("{n} diff comments"),
            };
            t.push_context(ChipKind::Review, label.into(), text, None, cx);
            t.composer.review.clear(scope);
            None
        });
        if let Some(why) = refused {
            self.publish_note = Some(why.into());
        }
        self.comments_changed(cx);
    }

    /// Whether unified row `row` is being picked or has the draft open, for its tint.
    pub(super) fn picked(&self, row: usize) -> bool {
        let dragging = self
            .drag
            .is_some_and(|d| (d.start.min(d.end)..=d.start.max(d.end)).contains(&row));
        dragging || self.draft.as_ref().is_some_and(|d| d.rows().contains(&row))
    }

    /// The clickable gutter over a line's numbers (`width` px from the left of its row or half; `left`: the split
    /// view's old side, whose context rows have a gutter on each side).
    pub(super) fn gutter(
        &self,
        row: usize,
        width: f32,
        left: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let hover = cx.theme().primary.opacity(0.14);
        let id = if left { "diff-gutter-l" } else { "diff-gutter" };
        div()
            .id((id, row))
            .debug_selector(move || format!("{id}-{row}"))
            .absolute()
            .top_0()
            .left_0()
            .h_full()
            .w(px(width))
            .when(self.commentable(), |d| {
                d.cursor_pointer()
                    .hover(move |s| s.bg(hover))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _, _, cx| {
                            cx.stop_propagation();
                            this.gutter_down(row, left, cx);
                        }),
                    )
            })
            .into_any_element()
    }

    /// What shows under the item whose rows `shows` names: comments ending there, then the draft.
    pub(super) fn annotations(
        &self,
        shows: impl Fn(usize) -> bool,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let mut out: Vec<AnyElement> = Vec::new();
        if let Some(review) = self.review(cx) {
            let ends: Vec<DiffComment> = review
                .items
                .iter()
                .filter(|c| c.scope == self.scope && shows(c.rows.end.saturating_sub(1)))
                .cloned()
                .collect();
            out.extend(ends.into_iter().map(|c| self.comment(c, cx)));
        }
        if let Some(d) = &self.draft
            && shows(d.rows().end.saturating_sub(1))
        {
            let label = d.anchor.items.first().map(DiffComment::label);
            out.push(self.draft_editor(label.unwrap_or_default(), d.input.clone(), cx));
        }
        out
    }

    /// A saved comment: T3's primary hairline on the left, the lines it names and the note, Delete on the right.
    fn comment(&self, c: DiffComment, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let id = c.id;
        h_flex()
            .id(("diff-comment", id))
            .items_start()
            .gap_2p5()
            .px_3()
            .py_2p5()
            .border_l_2()
            .border_color(theme.primary.opacity(0.55))
            .bg(theme.primary.opacity(0.045))
            .font_family(theme.font_family.clone())
            .child(
                Icon::new(SkyeIcon::MessageCircle)
                    .size_3p5()
                    .mt_0p5()
                    .text_color(theme.primary.opacity(0.7)),
            )
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .gap_0p5()
                    .child(
                        div()
                            .text_size(ui::META)
                            .text_color(theme.muted_foreground)
                            .child(c.label()),
                    )
                    .child(div().text_size(ui::BODY).child(c.note)),
            )
            .child(
                Button::new(("delete-diff-comment", id))
                    .ghost()
                    .xsmall()
                    .icon(Icon::new(SkyeIcon::Trash))
                    .tooltip("Delete comment")
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.update_review(cx, |r| r.remove(id));
                        this.comments_changed(cx);
                    })),
            )
            .into_any_element()
    }

    /// The draft: the text field, then "⌘ Enter to comment", Cancel and Comment (T3's draft annotation).
    fn draft_editor(
        &self,
        label: String,
        input: Entity<TextareaState>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = cx.theme();
        let key = if crate::workspace::MAC { "⌘" } else { "Ctrl" };
        v_flex()
            .id("diff-comment-draft")
            .gap_1p5()
            .px_3()
            .py_2()
            .border_l_2()
            .border_color(theme.primary.opacity(0.55))
            .bg(theme.background)
            .font_family(theme.font_family.clone())
            .capture_key_down(cx.listener(|this, e: &gpui::KeyDownEvent, window, cx| {
                if e.keystroke.key == "escape" {
                    cx.stop_propagation();
                    this.cancel_draft(window, cx);
                }
            }))
            .child(
                div()
                    .text_size(ui::META)
                    .text_color(theme.muted_foreground)
                    .child(format!("Comment on {label}")),
            )
            .child(Textarea::new(&input).small())
            .child(
                h_flex()
                    .gap_1()
                    .child(
                        div()
                            .mr_auto()
                            .text_size(ui::MICRO)
                            .text_color(theme.muted_foreground)
                            .child(format!("{key} Enter to comment")),
                    )
                    .child(
                        Button::new("cancel-diff-comment")
                            .ghost()
                            .xsmall()
                            .label("Cancel")
                            .on_click(
                                cx.listener(|this, _, window, cx| this.cancel_draft(window, cx)),
                            ),
                    )
                    .child(
                        Button::new("save-diff-comment")
                            .primary()
                            .xsmall()
                            .label("Comment")
                            .on_click(
                                cx.listener(|this, _, window, cx| this.save_draft(window, cx)),
                            ),
                    ),
            )
            .into_any_element()
    }

    /// The strip over the diff while it has comments: how many, a note when a reload dropped some, Clear and Send.
    pub(super) fn render_comments_bar(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let review = self.review(cx)?;
        let (n, dropped) = (review.count(self.scope), review.dropped_note());
        if n == 0 && dropped.is_none() {
            return None;
        }
        let theme = cx.theme();
        let pr = self.pr.is_some();
        let count = match (n, pr) {
            (1, false) => "1 comment on this diff".to_owned(),
            (n, false) => format!("{n} comments on this diff"),
            (1, true) => "1 pending review comment".to_owned(),
            (n, true) => format!("{n} pending review comments"),
        };
        let scope = self.scope;
        Some(
            h_flex()
                .id("diff-comments-bar")
                .px_4()
                .py_1()
                .gap_2()
                .border_b_1()
                .border_color(theme.border)
                .text_size(ui::META)
                .child(
                    Icon::new(SkyeIcon::MessageCircle)
                        .xsmall()
                        .text_color(ui::ink(theme, theme.primary)),
                )
                .when(n > 0, |d| {
                    d.child(div().font_weight(FontWeight::MEDIUM).child(count))
                })
                .children(dropped.map(|note| {
                    div()
                        .min_w_0()
                        .truncate()
                        .text_color(theme.muted_foreground)
                        .child(note)
                }))
                .child(div().flex_1())
                .when(n > 0, |d| {
                    d.child(
                        Button::new("clear-diff-comments")
                            .ghost()
                            .xsmall()
                            .label("Clear")
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.update_review(cx, |r| r.clear(scope));
                                this.comments_changed(cx);
                            })),
                    )
                    .child(
                        Button::new("send-diff-comments")
                            .primary()
                            .xsmall()
                            .label(match (n, pr) {
                                (_, true) => "Submit review…".to_owned(),
                                (1, false) => "Send 1 comment".to_owned(),
                                (n, false) => format!("Send {n} comments"),
                            })
                            .on_click(
                                cx.listener(|this, _, window, cx| this.send_comments(window, cx)),
                            ),
                    )
                })
                .into_any_element(),
        )
    }
}
