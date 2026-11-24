//! A permission request's card (T3's pending-approval row): the tool, what it wants to do, then the answers. A
//! file change shows as a diff, anything else as its description or escaped JSON (`ask_detail.rs`).

use super::ask_cards::waiting_row;
use super::ask_detail::{Detail, PREVIEW_LINES, Preview, detail, patch};
use super::changes::diff_line_wrapped;
use super::thread_items::{ItemsView, clip};
use super::thread_model::{ApprovalItem, Item};
use crate::ui;
use crate::ui::Button;
use crate::ui::SkyeIcon;
use gpui::{
    AnyElement, App, Context, FontWeight, IntoElement, SharedString, Window, div, prelude::*, px,
};
use gpui_component::diff::{Diff, DiffFile, DiffState};
use gpui_component::{ActiveTheme as _, Icon, WindowExt as _, h_flex, v_flex};
use skyelab_agent::Decision;

/// How tall an approval's diff or JSON gets before it scrolls (T3's `max-h-40`), so the answers stay in view.
const DETAIL_MAX_H: f32 = 160.;

/// The label a decided request keeps.
fn decided(d: Decision) -> &'static str {
    match d {
        Decision::AllowOnce => "Allowed once",
        Decision::AllowThread => "Allowed for this thread",
        Decision::Deny => "Denied",
        Decision::Withdrawn => "Withdrawn",
    }
}

/// A permission request, T3's pending-approval row: what the tool wants to do in mono, then the answers. Once
/// decided it shrinks to one line saying what was decided.
pub(super) fn render_approval<V: ItemsView>(
    this: &V,
    a: &ApprovalItem,
    docked: bool,
    cx: &mut Context<V>,
) -> AnyElement {
    if a.decision.is_none() && !docked && this.docks_asks(&a.id) {
        return waiting_row(SkyeIcon::TriangleAlert, &a.tool, cx);
    }
    let theme = cx.theme();
    let summary = div()
        .flex_1()
        .min_w_0()
        .truncate()
        .font_family(theme.mono_font_family.clone())
        .text_size(ui::META)
        .text_color(theme.muted_foreground)
        .child(a.summary.clone());
    if let Some(d) = a.decision {
        let (icon, color) = match d {
            Decision::AllowOnce | Decision::AllowThread => {
                (SkyeIcon::Check, theme.muted_foreground)
            }
            Decision::Deny => (SkyeIcon::CircleAlert, theme.danger),
            Decision::Withdrawn => (SkyeIcon::CircleAlert, theme.muted_foreground),
        };
        return h_flex()
            .min_h(px(28.))
            .gap_1p5()
            .px_1()
            .text_size(ui::BODY)
            .child(
                div()
                    .w(px(24.))
                    .flex()
                    .justify_center()
                    .flex_shrink_0()
                    .child(Icon::new(icon).size_4().text_color(color)),
            )
            .child(
                div()
                    .flex_shrink_0()
                    .font_weight(FontWeight::MEDIUM)
                    .child(a.tool.clone()),
            )
            .child(summary)
            .child(
                div()
                    .flex_shrink_0()
                    .text_size(ui::META)
                    .text_color(color)
                    .child(match &a.rule {
                        // Which rule answered for the user, so an allow nobody clicked is never silent.
                        Some(rule) => format!("Allowed by project rule · {rule}"),
                        None => decided(d).to_owned(),
                    }),
            )
            .into_any_element();
    }
    let answerable = this.can_answer(cx);
    let theme = cx.theme();
    let button = |label: &'static str, key: &str, decision: Decision| {
        let id = a.id.clone();
        Button::new(SharedString::from(format!("{key}-{}", a.id)))
            .xsmall()
            .label(label)
            .on_click(
                cx.listener(move |this, _, window, cx| this.answer(&id, decision, window, cx)),
            )
    };
    let actions = if answerable {
        h_flex()
            .gap_2()
            .child(button("Allow once", "allow-once", Decision::AllowOnce).primary())
            .child(
                button(
                    "Allow for this thread",
                    "allow-thread",
                    Decision::AllowThread,
                )
                .outline(),
            )
            .when(this.offers_project_rule(&a.tool), |d| {
                let id = a.id.clone();
                d.child(
                    Button::new(SharedString::from(format!("allow-project-{}", a.id)))
                        .xsmall()
                        .outline()
                        .label("Allow for this project")
                        .tooltip(
                            "Save a rule for this project and account, shown before it is saved",
                        )
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.allow_for_project(&id, window, cx)
                        })),
                )
            })
            .child(button("Deny", "deny", Decision::Deny).ghost())
            .child(
                Button::new(SharedString::from(format!("interrupt-{}", a.id)))
                    .xsmall()
                    .ghost()
                    .label("Interrupt")
                    .tooltip("Stop this turn instead (⌘.)")
                    .on_click(cx.listener(|this, _, window, cx| this.interrupt_turn(window, cx))),
            )
            .into_any_element()
    } else {
        div()
            .text_size(ui::META)
            .text_color(theme.muted_foreground)
            .child(
                "The agent isn't running, so this can't be answered. Send a message to continue.",
            )
            .into_any_element()
    };
    let shown = match detail(a, PREVIEW_LINES) {
        Detail::Diff(p) => Some(approval_diff(a, p, cx)),
        Detail::Json => Some(json_box(a, cx)),
        Detail::Note(note) => Some(
            div()
                .text_size(ui::META)
                .text_color(cx.theme().muted_foreground)
                .child(note)
                .into_any_element(),
        ),
        Detail::Nothing => None,
    };
    let theme = cx.theme();
    v_flex()
        .debug_selector(|| "pending-approval".into())
        .w_full()
        .gap_2()
        .px_3()
        .py_2p5()
        .rounded(ui::RADIUS_LG)
        .bg(theme.warning.opacity(0.08))
        .border_1()
        .border_color(theme.warning.opacity(0.35))
        .child(
            h_flex()
                .gap_1p5()
                .text_size(ui::BODY)
                .child(
                    Icon::new(SkyeIcon::TriangleAlert)
                        .size_4()
                        .text_color(ui::ink(theme, theme.warning)),
                )
                .child(div().font_weight(FontWeight::MEDIUM).child(a.tool.clone()))
                .child(
                    div()
                        .text_size(ui::META)
                        .text_color(if answerable {
                            ui::ink(theme, theme.warning)
                        } else {
                            theme.muted_foreground
                        })
                        // With its agent gone it waits on nothing; the line under it says how to go on.
                        .child(if answerable {
                            "Waiting for approval"
                        } else {
                            "Not answered"
                        }),
                ),
        )
        .child(
            div()
                .font_family(theme.mono_font_family.clone())
                .text_size(ui::BODY)
                .child(a.summary.clone()),
        )
        .children(shown)
        .child(actions)
        .into_any_element()
}

/// The input as escaped JSON, scrolling past a few lines so the answers stay in view.
fn json_box<V: ItemsView>(a: &ApprovalItem, cx: &Context<V>) -> AnyElement {
    let theme = cx.theme();
    div()
        .id(SharedString::from(format!("approval-json-{}", a.id)))
        .max_h(px(DETAIL_MAX_H))
        .overflow_y_scroll()
        .px_3()
        .py_2()
        .rounded(ui::RADIUS)
        .bg(theme.muted)
        .border_1()
        .border_color(theme.border)
        .font_family(theme.mono_font_family.clone())
        .text_size(ui::META)
        .child(clip(&a.input))
        .into_any_element()
}

/// A Write / Edit / MultiEdit as T3 shows a file change: its size, then the first lines as the edit card draws them
/// (a line between MultiEdit's edits), and when that is a part, "Showing N of M lines" with View all.
fn approval_diff<V: ItemsView>(a: &ApprovalItem, p: Preview, cx: &mut Context<V>) -> AnyElement {
    let theme = cx.theme();
    let mono = theme.mono_font_family.clone();
    let size = if a.tool == "Write" {
        h_flex().child(format!(
            "{} line{}",
            p.added,
            if p.added == 1 { "" } else { "s" }
        ))
    } else {
        h_flex()
            .gap_1()
            .child(
                div()
                    .text_color(ui::ink(theme, theme.success))
                    .child(format!("+{}", p.added)),
            )
            .child(
                div()
                    .text_color(ui::ink(theme, theme.danger))
                    .child(format!("−{}", p.removed)),
            )
    }
    .font_family(mono.clone())
    .text_size(ui::META)
    .text_color(theme.muted_foreground)
    .when(p.replace_all, |d| d.child("· replaces every match"));
    let (shown, total) = (p.lines.len(), p.lines.len() + p.more);
    let rows = p
        .lines
        .into_iter()
        .enumerate()
        .flat_map(|(ix, (o, n, k, t))| {
            let divider = p
                .breaks
                .contains(&ix)
                .then(|| div().h(px(1.)).w_full().bg(theme.border).into_any_element());
            // Wrapped, never clipped: what a long line hides past the edge would still be written.
            let base = h_flex()
                .id(("approval-line", ix))
                .min_h(px(20.))
                .items_start()
                .w_full()
                .font_family(mono.clone())
                .text_xs();
            divider.into_iter().chain(std::iter::once(
                diff_line_wrapped(base, o, n, k, t.into(), theme).into_any_element(),
            ))
        });
    let lines = div()
        .rounded(ui::RADIUS)
        .border_1()
        .border_color(theme.border)
        .overflow_hidden()
        .child(
            v_flex()
                .id(SharedString::from(format!("approval-diff-{}", a.id)))
                .debug_selector(|| "approval-diff".into())
                .max_h(px(DETAIL_MAX_H))
                .overflow_y_scroll()
                .bg(theme.background)
                .children(rows),
        );
    let id = a.id.clone();
    // Said plainly whenever the card holds only a part: the rest is one click away, never just "not shown".
    let part = (p.more > 0).then(|| {
        h_flex()
            .gap_2()
            .text_size(ui::META)
            .text_color(theme.muted_foreground)
            .child(format!(
                "Showing {} of {} lines",
                thousands(shown),
                thousands(total)
            ))
            .child(
                Button::new(SharedString::from(format!("approval-view-all-{}", a.id)))
                    .debug_selector(|| "approval-view-all".into())
                    .ghost()
                    .xsmall()
                    .label("View all")
                    .on_click(cx.listener(move |this, _, window, cx| {
                        let a = this.items().iter().find_map(|i| match i {
                            Item::Approval(a) if a.id == id => Some(a),
                            _ => None,
                        });
                        if let Some((title, patch)) =
                            a.and_then(|a| Some((a.summary.clone(), patch(a)?)))
                        {
                            view_all(title, &patch, window, cx);
                        }
                    })),
            )
    });
    v_flex()
        .gap_1()
        .child(size)
        .child(lines)
        .children(part)
        .into_any_element()
}

/// `2000` as `2,000`.
fn thousands(n: usize) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// The whole change in a dialog: gpui-component's read-only diff viewer, which lays out only the lines in view and
/// wraps long ones, so a 2,000-line Write is all there to read before it is allowed.
fn view_all(title: String, patch: &str, window: &mut Window, cx: &mut App) {
    let body = match DiffFile::parse(patch) {
        Ok(files) => {
            let state = cx.new(|cx| DiffState::new(files, cx));
            Ok(state)
        }
        Err(e) => Err(SharedString::from(format!(
            "Couldn't draw this change: {e}"
        ))),
    };
    let height = window.viewport_size().height * 0.7;
    window.open_dialog(cx, move |dialog, _, _| {
        let body = match &body {
            Ok(state) => Diff::new(state)
                .soft_wrap(true)
                .size_full()
                .into_any_element(),
            Err(why) => div().child(why.clone()).into_any_element(),
        };
        dialog.w(px(960.)).title(title.clone()).child(
            div()
                .debug_selector(|| "approval-view-all-dialog".into())
                .w_full()
                .h(height)
                .child(body),
        )
    });
}
