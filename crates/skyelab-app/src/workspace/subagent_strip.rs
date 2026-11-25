//! T3's "Waiting on N subagents" strip over the composer: the running delegated children and in-turn subagents by
//! name, with Details (the thread details card) and Stop (stops them).

use super::super::Workspace;
use super::super::layout::ToggleThreadDetails;
use super::super::subagent::SubagentStatus;
use super::super::thread::{ChildLink, Interrupt, Stop, ThreadView};
use super::super::thread_model::Item;
use crate::ui::Button;
use crate::ui::SkyeIcon;
use crate::ui::{self, Status};
use gpui::{Action, AnyElement, Context, SharedString, Window, div, prelude::*};
use gpui_component::{ActiveTheme as _, Icon, h_flex};

/// Stops thread `id`'s running children: each delegated thread open in any window, and its turn when in-turn
/// subagents run.
#[derive(Clone, PartialEq, Action)]
#[action(namespace = skyelab, no_json)]
pub struct StopChildren {
    pub id: i64,
}

/// Who the thread waits on: running delegated children, then subagents still running in its live turn (a card left
/// running by an earlier turn waits on nothing).
pub(in crate::workspace) fn waiting_on(
    children: &[ChildLink],
    items: &[Item],
    turn_running: bool,
) -> Vec<SharedString> {
    let kids = running_children(children).map(|c| c.title.clone());
    let subagents = turn_subagents(items, turn_running).filter_map(|i| match i {
        Item::Subagent(s) if s.status == SubagentStatus::Running => Some(
            match s.description.is_empty() {
                true => s.kind.clone(),
                false => s.description.clone(),
            }
            .into(),
        ),
        _ => None,
    });
    kids.chain(subagents).collect()
}

fn running_children(children: &[ChildLink]) -> impl Iterator<Item = &ChildLink> {
    children
        .iter()
        .filter(|c| matches!(c.status, Status::Starting | Status::Running))
}

/// The live turn's running subagent cards: those after its prompt.
fn turn_subagents(items: &[Item], turn_running: bool) -> impl Iterator<Item = &Item> {
    let start = items
        .iter()
        .rposition(|i| matches!(i, Item::Prompt(_)))
        .map_or(0, |p| p + 1);
    items[start..].iter().filter(move |i| {
        turn_running && matches!(i, Item::Subagent(s) if s.status == SubagentStatus::Running)
    })
}

impl ThreadView {
    pub(super) fn render_subagent_strip(&self, cx: &Context<Self>) -> Option<AnyElement> {
        let names = waiting_on(&self.children, &self.items, self.turn_running());
        let id = self.thread.as_ref()?.id;
        if names.is_empty() {
            return None;
        }
        let theme = cx.theme();
        let label = match names.len() {
            1 => "Waiting on 1 subagent".to_string(),
            n => format!("Waiting on {n} subagents"),
        };
        let link = |name: &'static str| {
            Button::new(name)
                .ghost()
                .xsmall()
                .label(name)
                .text_color(theme.foreground)
        };
        Some(
            h_flex()
                .debug_selector(|| "subagent-strip".into())
                .w_full()
                .max_w(ui::CHAT_MAX_W)
                .mb_2()
                .px_1()
                .gap_1p5()
                .text_size(ui::META)
                .text_color(theme.muted_foreground)
                .child(Icon::new(SkyeIcon::LoaderCircle).size_3p5())
                .child(
                    div()
                        .flex_shrink_0()
                        .font_weight(gpui::FontWeight::MEDIUM)
                        .child(label),
                )
                .child(div().min_w_0().truncate().child(names.join(", ")))
                .child(link("Details").on_click(|_, window, cx| {
                    crate::workspace::dispatch(Box::new(ToggleThreadDetails), window, cx)
                }))
                .child(link("Stop").on_click(move |_, window, cx| {
                    crate::workspace::dispatch(Box::new(StopChildren { id }), window, cx)
                }))
                .into_any_element(),
        )
    }
}

impl Workspace {
    pub(in crate::workspace) fn stop_children(
        &mut self,
        a: &StopChildren,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(view) = self.open_view(a.id, cx) else {
            return;
        };
        let (kids, subagents) = {
            let v = view.read(cx);
            let kids: Vec<i64> = running_children(&v.children).map(|c| c.id).collect();
            (
                kids,
                turn_subagents(&v.items, v.turn_running()).next().is_some(),
            )
        };
        for kid in kids {
            let found = self.open_view(kid, cx).or_else(|| {
                self.other_windows(cx)
                    .into_iter()
                    .find_map(|(_, ws)| ws.upgrade()?.read(cx).open_view(kid, cx))
            });
            if let Some(kid) = found {
                kid.update(cx, |k, cx| k.stop(&Stop, window, cx));
            }
        }
        // In-turn subagents end with the turn that runs them.
        if subagents {
            view.update(cx, |v, cx| v.interrupt(&Interrupt, window, cx));
        }
    }
}

#[cfg(test)]
#[path = "subagent_strip_test.rs"]
mod tests;
