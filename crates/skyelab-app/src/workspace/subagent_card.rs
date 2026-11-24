//! How a subagent card looks: T3's work-log row for the Agent call (status icon, type, what it was asked, status and
//! time), opening to the calls it made and its report.

use super::{SubagentItem, SubagentStatus};
use crate::ui;
use crate::ui::SkyeIcon;
use crate::workspace::thread_items::{ItemsView, clip, tool_summary};
use gpui::{AnyElement, Context, FontWeight, SharedString, div, prelude::*, px};
use gpui_component::{ActiveTheme as _, Icon, h_flex, v_flex};

/// "Running · 4s", "Done · 1m 02s", "Failed".
pub fn status_text(s: &SubagentItem) -> String {
    let label = match s.status {
        SubagentStatus::Running => "Running",
        SubagentStatus::Done => "Done",
        SubagentStatus::Failed => "Failed",
        SubagentStatus::Stopped => "Stopped",
    };
    match s.duration_ms {
        Some(ms) => format!("{label} · {}", ui::elapsed(ms as i64)),
        None => label.to_owned(),
    }
}

pub fn render<V: ItemsView>(this: &V, s: &SubagentItem, cx: &mut Context<V>) -> AnyElement {
    // A running one whose agent is gone (it died, or was stopped) shows as stopped, not spinning.
    let cut;
    let s = if s.status == SubagentStatus::Running && !this.agent_alive() {
        cut = SubagentItem {
            status: SubagentStatus::Stopped,
            ..s.clone()
        };
        &cut
    } else {
        s
    };
    let theme = cx.theme();
    let open = this.is_expanded(&s.id);
    let (icon, color) = match s.status {
        SubagentStatus::Running => (SkyeIcon::LoaderCircle, theme.info),
        SubagentStatus::Done => (SkyeIcon::Check, theme.muted_foreground),
        SubagentStatus::Failed => (SkyeIcon::CircleAlert, theme.danger),
        SubagentStatus::Stopped => (SkyeIcon::CircleAlert, theme.muted_foreground),
    };
    let slot = |icon: SkyeIcon, color| {
        div()
            .w(px(24.))
            .flex()
            .justify_center()
            .flex_shrink_0()
            .child(Icon::new(icon).size_4().text_color(color))
    };
    let id = s.id.clone();
    let mono = theme.mono_font_family.clone();
    let header = h_flex()
        .id(SharedString::from(format!("subagent-{}", s.id)))
        .min_h(px(32.))
        .gap_1p5()
        .px_1()
        .rounded(ui::RADIUS_MD)
        .cursor_pointer()
        .text_size(ui::BODY)
        .hover(|st| st.bg(theme.accent))
        .on_click(cx.listener(move |this, _, _, cx| this.toggle_tool(&id, cx)))
        .child(slot(icon, color))
        .child(
            div()
                .flex_shrink_0()
                .font_weight(FontWeight::MEDIUM)
                .child(format!("Subagent · {}", s.kind)),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .text_color(theme.muted_foreground)
                .child(s.description.clone()),
        )
        .child(
            div()
                .flex_shrink_0()
                .text_size(ui::META)
                .text_color(match s.status {
                    SubagentStatus::Running => ui::ink(theme, theme.info),
                    SubagentStatus::Failed => ui::ink(theme, theme.danger),
                    SubagentStatus::Done | SubagentStatus::Stopped => theme.muted_foreground,
                })
                .child(status_text(s)),
        )
        .child(
            Icon::new(if open {
                SkyeIcon::ChevronDown
            } else {
                SkyeIcon::ChevronRight
            })
            .size_3()
            .text_color(theme.muted_foreground.opacity(0.7)),
        );
    // While it runs, what it is doing now; folded, how many calls it made.
    let activity = match (s.status, open) {
        (SubagentStatus::Running, false) => s.activity.clone(),
        (_, false) if !s.tools.is_empty() => Some(match s.tools.len() {
            1 => "1 tool call".to_owned(),
            n => format!("{n} tool calls"),
        }),
        _ => None,
    };
    let calls = s.tools.iter().map(|t| {
        let (icon, color) = match (&t.result, t.is_error) {
            (None, _) => (SkyeIcon::LoaderCircle, theme.info),
            (Some(_), true) => (SkyeIcon::CircleAlert, theme.danger),
            (Some(_), false) => (SkyeIcon::Check, theme.muted_foreground),
        };
        h_flex()
            .min_h(px(24.))
            .gap_1p5()
            .text_size(ui::META)
            .child(slot(icon, color))
            .child(
                div()
                    .flex_shrink_0()
                    .font_weight(FontWeight::MEDIUM)
                    .child(t.name.clone()),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .font_family(mono.clone())
                    .text_color(theme.muted_foreground)
                    .child(tool_summary(t)),
            )
    });
    let report = s
        .summary
        .as_deref()
        .filter(|_| s.status != SubagentStatus::Running)
        .map(|r| {
            div()
                .mt_1()
                .px_3()
                .py_2()
                .rounded(ui::RADIUS)
                .bg(theme.muted)
                .text_size(ui::META)
                .text_color(theme.muted_foreground)
                .child(clip(r))
        });
    v_flex()
        .debug_selector(|| "subagent-card".into())
        .w_full()
        .p_1()
        .rounded(ui::RADIUS_LG)
        .border_1()
        .border_color(theme.border)
        .child(header)
        .children(activity.map(|a| {
            div()
                .pl(px(34.))
                .pb_1()
                .truncate()
                .text_size(ui::META)
                .text_color(theme.muted_foreground)
                .child(a)
        }))
        .when(open, |el| {
            el.child(
                v_flex()
                    .debug_selector(|| "subagent-calls".into())
                    .pl_2()
                    .pb_1()
                    .children(calls)
                    .children(report),
            )
        })
        .into_any_element()
}
