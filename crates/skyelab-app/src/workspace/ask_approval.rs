//! A permission request's card (T3's pending-approval row): the tool, what it wants to do, then the answers. A
//! file change shows as a diff, anything else as its description or escaped JSON (`ask_detail.rs`).

use super::ask_cards::waiting_row;
use super::ask_detail::{Detail, PREVIEW_LINES, Preview, detail};
use super::changes::diff_line_wrapped;
use super::thread_items::{ItemsView, clip};
use super::thread_model::ApprovalItem;
use crate::ui;
use crate::ui::Button;
use crate::ui::SkyeIcon;
use gpui::{AnyElement, Context, FontWeight, IntoElement, SharedString, div, prelude::*, px};
use gpui_component::{ActiveTheme as _, Icon, h_flex, v_flex};
use skyelab_agent::Decision;
use skyelab_agent::timeline::cards::DIFF_LINES;

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
                    .text_size(ui::META)
                    .text_color(color)
                    .child(decided(d)),
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
    let open = this.is_expanded(&a.id);
    let limit = if open { DIFF_LINES } else { PREVIEW_LINES };
    let shown = match detail(a, limit) {
        Detail::Diff(p) => Some(approval_diff(a, p, open, cx)),
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

/// A Write / Edit / MultiEdit as T3 shows a file change: its size, then the first lines as the edit card draws them,
/// with "+N more lines" to see the rest.
fn approval_diff<V: ItemsView>(
    a: &ApprovalItem,
    p: Preview,
    open: bool,
    cx: &mut Context<V>,
) -> AnyElement {
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
    let rows = p.lines.into_iter().enumerate().map(|(ix, (o, n, k, t))| {
        // Wrapped, never clipped: what a long line hides past the edge would still be written.
        let base = h_flex()
            .id(("approval-line", ix))
            .min_h(px(20.))
            .items_start()
            .w_full()
            .font_family(mono.clone())
            .text_xs();
        diff_line_wrapped(base, o, n, k, t.into(), theme)
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
    let toggle = (open || p.more > 0).then(|| {
        h_flex()
            .gap_2()
            .text_size(ui::META)
            .text_color(theme.muted_foreground)
            .child(
                Button::new(SharedString::from(format!("approval-more-{}", a.id)))
                    .debug_selector(|| "approval-more".into())
                    .ghost()
                    .xsmall()
                    .label(if open {
                        "Show less".to_owned()
                    } else {
                        format!("+{} more lines", p.more)
                    })
                    .on_click(cx.listener(move |this, _, _, cx| this.toggle_tool(&id, cx))),
            )
            // Expanded, a huge file still stops at the edit card's cap.
            .when(open && p.more > 0, |d| {
                d.child(format!("{} more lines not shown", p.more))
            })
    });
    v_flex()
        .gap_1()
        .child(size)
        .child(lines)
        .children(toggle)
        .into_any_element()
}
