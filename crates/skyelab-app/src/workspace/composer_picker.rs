//! The composer's model and effort picker, a popover over the card's right side.

use super::thread::ThreadView;
use super::{hint, popover};
use crate::ui;
use crate::ui::Checkbox;
use crate::ui::SkyeIcon;
use gpui::{AnyElement, Context, FontWeight, SharedString, div, prelude::*, px};
use gpui_component::{ActiveTheme as _, Icon, Sizable as _, h_flex, v_flex};
use skyelab_agent::{CLAUDE_EFFORTS, CLAUDE_MODELS};
use skyelab_core::Provider;
use skyelab_store::ModelChoice;

/// A picker row: the model id, its label and the efforts it takes.
type PickerModel = (String, String, Vec<String>);

/// "high" → "High".
pub(in crate::workspace) fn title_case(s: &str) -> String {
    let mut c = s.chars();
    c.next()
        .map(|f| f.to_uppercase().chain(c).collect())
        .unwrap_or_default()
}

impl ThreadView {
    /// The model and effort choices for the thread's provider.
    pub(in crate::workspace) fn render_picker(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let provider = self.composer_account().map(|a| a.provider);
        let pick = self.composer.pick.clone();
        let claude_efforts: Vec<String> = CLAUDE_EFFORTS.iter().map(|e| (*e).to_owned()).collect();
        let (models, error): (Vec<PickerModel>, Option<SharedString>) = match provider {
            Some(Provider::Codex) => match &self.composer.codex_models {
                Some(Ok(list)) => (
                    list.iter()
                        .map(|m| (m.id.clone(), m.label.clone(), m.efforts.clone()))
                        .collect(),
                    list.is_empty().then(|| "Listing Codex's models…".into()),
                ),
                Some(Err(e)) => (Vec::new(), Some(e.clone())),
                None => (Vec::new(), None),
            },
            _ => (
                CLAUDE_MODELS
                    .iter()
                    .map(|(id, label)| {
                        (
                            (*id).to_owned(),
                            (*label).to_owned(),
                            claude_efforts.clone(),
                        )
                    })
                    .collect(),
                None,
            ),
        };
        if let Some(c) = &self.composer.compare {
            return self.render_compare_picker(c, &models, error, cx);
        }
        // A draft can go to several models at once (T3's multiple-model send); a thread with a row can't.
        let several = (self.thread.is_none() && models.len() > 1).then(|| {
            div()
                .pt_1()
                .mt_1()
                .border_t_1()
                .border_color(theme.border)
                .child(
                    h_flex()
                        .id("compare-start")
                        .h(px(28.))
                        .px_2()
                        .gap_2()
                        .rounded(ui::RADIUS_SM)
                        .text_size(ui::BODY)
                        .hover(|this| this.bg(theme.accent))
                        .child(
                            Icon::new(crate::ui::SkyeIcon::Columns2)
                                .xsmall()
                                .text_color(theme.muted_foreground),
                        )
                        .child("Send to several models…")
                        .on_click(cx.listener(|this, _, _, cx| this.start_compare(cx))),
                )
        });
        let efforts: Vec<String> = match &pick.model {
            Some(id) => models
                .iter()
                .find(|(m, ..)| m == id)
                .map(|(.., e)| e.clone())
                .unwrap_or_default(),
            // The account's default model: Claude takes any effort; Codex's are per model.
            None if provider != Some(Provider::Codex) => claude_efforts.clone(),
            None => Vec::new(),
        };
        let option = |id: String, label: String, on: bool, choice: ModelChoice| {
            h_flex()
                .id(SharedString::from(id))
                .h(px(28.))
                .px_2()
                .gap_2()
                .rounded(ui::RADIUS_SM)
                .text_size(ui::BODY)
                .when(on, |this| this.bg(theme.foreground.opacity(0.09)))
                .hover(|this| this.bg(theme.accent))
                .child(div().flex_1().child(label))
                .when(on, |this| this.child(Icon::new(SkyeIcon::Check).xsmall()))
                .on_click(cx.listener(move |this, _, _, cx| this.pick_model(choice.clone(), cx)))
        };
        let default_row = format!("Account default ({})", self.default_model_name());
        let model_rows = std::iter::once((None, default_row))
            .chain(
                models
                    .iter()
                    .map(|(id, label, _)| (Some(id.clone()), label.clone())),
            )
            .map(|(id, label)| {
                let on = pick.model == id;
                let model_takes_effort = |e: &String| match &id {
                    Some(id) => models.iter().any(|(m, _, es)| m == id && es.contains(e)),
                    // The account's default model: any Claude effort; Codex's default shows no effort choice.
                    None => provider != Some(Provider::Codex),
                };
                let choice = ModelChoice {
                    model: id.clone(),
                    effort: pick.effort.clone().filter(model_takes_effort),
                };
                option(
                    format!("pick-model-{}", id.as_deref().unwrap_or("default")),
                    label,
                    on,
                    choice,
                )
                .into_any_element()
            })
            .collect::<Vec<_>>();
        let effort_rows = (!efforts.is_empty()).then(|| {
            std::iter::once(None)
                .chain(efforts.iter().cloned().map(Some))
                .map(|e| {
                    let label = e.as_deref().map_or("Default effort".into(), title_case);
                    let choice = ModelChoice {
                        model: pick.model.clone(),
                        effort: e.clone(),
                    };
                    option(
                        format!("pick-effort-{}", e.as_deref().unwrap_or("default")),
                        label,
                        pick.effort == e,
                        choice,
                    )
                    .into_any_element()
                })
                .collect::<Vec<_>>()
        });
        let section = |t: &'static str| {
            div()
                .px_2()
                .pt_1()
                .text_size(ui::META)
                .font_weight(FontWeight::MEDIUM)
                .text_color(theme.muted_foreground)
                .child(t)
        };
        popover(cx)
            .id("model-picker-menu")
            .right_0()
            .w(px(260.))
            .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                this.composer.picker_open = false;
                cx.notify();
            }))
            .child(section("Model"))
            .children(model_rows)
            .children(error.map(|e| hint(e, cx)))
            .children(effort_rows.map(|rows| {
                v_flex()
                    .pt_1()
                    .mt_1()
                    .border_t_1()
                    .border_color(theme.border)
                    .child(section("Effort"))
                    .children(rows)
            }))
            .children(several)
            .children(self.session.is_some().then(|| {
                hint(
                    "A new effort applies when the agent starts again; a new model, from the next message.",
                    cx,
                )
            }))
            .into_any_element()
    }
}

impl ThreadView {
    /// The picker while several models are ticked: a check row per model, the worktree option, and the way back.
    fn render_compare_picker(
        &self,
        c: &crate::workspace::compare::Compare,
        models: &[PickerModel],
        error: Option<SharedString>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = cx.theme();
        let row = |id: SharedString| {
            h_flex()
                .id(id)
                .h(px(28.))
                .px_2()
                .gap_2()
                .rounded(ui::RADIUS_SM)
                .text_size(ui::BODY)
                .hover(|this| this.bg(theme.accent))
        };
        // The handler is on the row only: a click on the checkbox reaches the row too (docs/gpui.md).
        let check =
            |id: String, on: bool| Checkbox::new(SharedString::from(id)).xsmall().checked(on);
        let full = c.models.len() >= crate::workspace::compare::MAX_MODELS;
        let model_rows = models.iter().map(|(id, label, _)| {
            let on = c.models.contains(id);
            let model = id.clone();
            row(format!("compare-model-{id}").into())
                .child(check(format!("compare-check-{id}"), on))
                .child(div().flex_1().child(label.clone()))
                .when(full && !on, |this| this.opacity(0.5))
                .on_click(
                    cx.listener(move |this, _, _, cx| this.toggle_compare_model(model.clone(), cx)),
                )
        });
        let count = c.models.len();
        let status = if count < 2 {
            "Tick 2 to 4 models. Each runs as its own thread on this folder's account.".to_owned()
        } else {
            format!("Send starts {count} threads, one per model, on this folder's account.")
        };
        popover(cx)
            .id("model-picker-menu")
            .right_0()
            .w(px(280.))
            .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                this.composer.picker_open = false;
                cx.notify();
            }))
            .child(
                div()
                    .px_2()
                    .pt_1()
                    .text_size(ui::META)
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(theme.muted_foreground)
                    .child("Send to several models"),
            )
            .children(model_rows)
            .children(error.map(|e| hint(e, cx)))
            .child(
                v_flex()
                    .pt_1()
                    .mt_1()
                    .border_t_1()
                    .border_color(theme.border)
                    .child(
                        row("compare-worktrees".into())
                            .child(check("compare-worktrees-check".into(), c.worktrees))
                            .child(div().flex_1().child("In worktrees"))
                            .on_click(
                                cx.listener(|this, _, _, cx| this.toggle_compare_worktrees(cx)),
                            ),
                    )
                    .child(
                        row("compare-stop".into())
                            .child(
                                Icon::new(SkyeIcon::Close)
                                    .xsmall()
                                    .text_color(theme.muted_foreground),
                            )
                            .child("One model")
                            .on_click(cx.listener(|this, _, _, cx| this.stop_compare(cx))),
                    ),
            )
            .child(hint(status, cx))
            .into_any_element()
    }
}
