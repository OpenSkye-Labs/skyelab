//! The composer's model and effort picker, a popover over the card's right side.

use super::composer_picker_keys::{PICKER_CONTEXT, PickerJump};
use super::thread::ThreadView;
use super::{hint, popover};
use crate::ui;
use crate::ui::ActiveTheme as _;
use crate::ui::{Checkbox, Kbd, SkyeIcon, TextField};
use gpui::{AnyElement, Context, FontWeight, SharedString, Window, div, prelude::*, px};
use gpui_component::{Icon, Sizable as _, h_flex, v_flex};
use skyelab_agent::CLAUDE_EFFORTS;
use skyelab_core::Provider;

/// An effort as T3 names it: "high" → "High", "xhigh" → "Extra High".
pub(in crate::workspace) fn effort_label(s: &str) -> String {
    if s == "xhigh" {
        return "Extra High".into();
    }
    let mut c = s.chars();
    c.next()
        .map(|f| f.to_uppercase().chain(c).collect())
        .unwrap_or_default()
}

impl ThreadView {
    /// Why a dynamically listed model list is empty: still loading, or the error listing it gave.
    fn picker_error(&self) -> Option<SharedString> {
        match self.composer_account().map(|a| a.provider) {
            Some(Provider::Codex) => match &self.composer.codex_models {
                Some(Ok(l)) if l.is_empty() => Some("Listing Codex's models…".into()),
                Some(Err(e)) => Some(e.clone()),
                _ => None,
            },
            Some(Provider::Cursor) => match &self.composer.cursor_models {
                Some(Ok(l)) if l.is_empty() => Some("Listing Cursor's models…".into()),
                Some(Err(e)) => Some(e.clone()),
                _ => None,
            },
            Some(Provider::Antigravity) => match &self.composer.antigravity_models {
                Some(Ok(l)) if l.is_empty() => Some("Listing Antigravity's models…".into()),
                Some(Err(e)) => Some(e.clone()),
                _ => None,
            },
            _ => None,
        }
    }

    /// The efforts the current pick takes, in order.
    fn picker_efforts(&self) -> Vec<String> {
        let provider = self.composer_account().map(|a| a.provider);
        match (&self.composer.pick.model, provider) {
            (Some(id), Some(Provider::Codex | Provider::Antigravity)) => self
                .listed()
                .and_then(|l| l.iter().find(|m| m.id == *id))
                .map(|m| m.efforts.clone())
                .unwrap_or_default(),
            (None, Some(Provider::Antigravity)) => self
                .listed()
                .and_then(|l| l.iter().find(|m| m.is_default).or_else(|| l.first()))
                .map(|m| m.efforts.clone())
                .unwrap_or_default(),
            // Codex's default and Cursor models show no effort column.
            (None, Some(Provider::Codex)) | (_, Some(Provider::Cursor)) => Vec::new(),
            // Claude takes any effort on any model, the account's default too.
            _ => CLAUDE_EFFORTS.iter().map(|e| (*e).to_owned()).collect(),
        }
    }

    /// T3's picker: a rail naming the bound account, the searchable model list, and the effort column beside it.
    pub(in crate::workspace) fn render_picker(
        &self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if let Some(c) = &self.composer.compare {
            return self.render_compare_picker(c, cx);
        }
        let Some(state) = self.composer.picker.as_ref() else {
            return div().into_any_element();
        };
        let theme = cx.theme();
        let account = self.composer_account();
        let provider = account.as_ref().map_or(Provider::Claude, |a| a.provider);
        let pick = self.composer.pick.clone();
        let rows = self.picker_rows(cx);
        let hi = state.hi;
        // The bound account only, as context: no click handler, since a thread never runs under another account.
        let rail = v_flex()
            .flex_shrink_0()
            .w(px(48.))
            .py_2()
            .items_center()
            .border_r_1()
            .border_color(theme.border)
            .children(account.map(|a| {
                let color = ui::account_color_in(cx, &a.id);
                div()
                    .id("picker-account")
                    .tooltip(ui::Tooltip::text(format!(
                        "{}: this folder's account",
                        a.label
                    )))
                    .child(crate::settings::provider_mark(a.provider, color, cx))
            }));
        let model_rows = rows.iter().enumerate().map(|(i, (id, label))| {
            let on = pick.model == *id;
            let index = i;
            let a11y_label = label.clone();
            h_flex()
                .id(SharedString::from(format!(
                    "pick-model-{}",
                    id.as_deref().unwrap_or("default")
                )))
                .role(gpui::accesskit::Role::MenuItem)
                .aria_label(a11y_label)
                .aria_selected(on)
                .h(px(30.))
                .px_2()
                .gap_2()
                .rounded(ui::RADIUS_SM)
                .text_size(ui::BODY)
                .when(i == hi, |this| this.bg(theme.accent))
                .hover(|this| this.bg(theme.accent))
                .child(ui::provider_logo(provider, px(16.), cx))
                .child(div().min_w_0().truncate().child(label.clone()))
                .when(on, |this| this.child(Icon::new(SkyeIcon::Check).xsmall()))
                .child(div().flex_1())
                .children(
                    (i < 9)
                        .then(|| {
                            Kbd::for_action(&PickerJump { index: i }, Some(PICKER_CONTEXT), window)
                        })
                        .flatten(),
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    if let Some((id, _)) = this.picker_rows(cx).into_iter().nth(index) {
                        let choice = this.model_choice(id);
                        this.pick_model(choice, cx);
                    }
                }))
                .into_any_element()
        });
        let empty = rows.is_empty().then(|| hint("No model matches.", cx));
        let middle = v_flex()
            .flex_1()
            .min_w_0()
            .p_1()
            .gap_0p5()
            .child(
                div().pb_1().child(
                    TextField::new(&state.search)
                        .small()
                        .accessibility_label("Filter models"),
                ),
            )
            .children(model_rows)
            .children(empty)
            .children(self.picker_error().map(|e| hint(e, cx)));
        let efforts = self.picker_efforts();
        let effort_col = (!efforts.is_empty()).then(|| {
            let rows = std::iter::once(None)
                .chain(efforts.into_iter().map(Some))
                .map(|e| {
                    let on = pick.effort == e;
                    let label = e.as_deref().map_or("Default".into(), effort_label);
                    h_flex()
                        .id(SharedString::from(format!(
                            "pick-effort-{}",
                            e.as_deref().unwrap_or("default")
                        )))
                        .role(gpui::accesskit::Role::MenuItem)
                        .aria_label(format!("Effort: {label}"))
                        .aria_selected(on)
                        .h(px(30.))
                        .px_2()
                        .gap_2()
                        .rounded(ui::RADIUS_SM)
                        .text_size(ui::BODY)
                        .hover(|this| this.bg(theme.accent))
                        .child(div().flex_1().child(label))
                        .when(on, |this| this.child(Icon::new(SkyeIcon::Check).xsmall()))
                        .on_click(
                            cx.listener(move |this, _, _, cx| this.pick_effort(e.clone(), cx)),
                        )
                });
            v_flex()
                .flex_shrink_0()
                .w(px(176.))
                .p_1()
                .gap_0p5()
                .border_l_1()
                .border_color(theme.border)
                .child(
                    h_flex()
                        .h(px(30.))
                        .px_2()
                        .gap_2()
                        .text_size(ui::META)
                        .text_color(theme.muted_foreground)
                        .child(
                            div()
                                .flex_1()
                                .font_weight(FontWeight::MEDIUM)
                                .child("Effort"),
                        )
                        .child(div().truncate().child(self.chip_label(true).0)),
                )
                .children(rows)
        });
        // A draft can go to several models at once (T3's multiple-model send); a thread with a row can't. Counted
        // unfiltered (the default row plus 2 or more models), so a search doesn't hide it.
        let several = (self.thread.is_none() && self.picker_models("").len() > 2).then(|| {
            h_flex()
                .id("compare-start")
                .role(gpui::accesskit::Role::Button)
                .aria_label("Send to several models…")
                .h(px(28.))
                .px_2()
                .gap_2()
                .rounded(ui::RADIUS_SM)
                .text_size(ui::BODY)
                .hover(|this| this.bg(theme.accent))
                .child(
                    Icon::new(SkyeIcon::Columns2)
                        .xsmall()
                        .text_color(theme.muted_foreground),
                )
                .child("Send to several models…")
                .on_click(cx.listener(|this, _, _, cx| this.start_compare(cx)))
        });
        let note = self.session.is_some().then(|| {
            hint(
                "A new effort applies when the agent starts again; a new model, from the next message.",
                cx,
            )
        });
        let footer = (several.is_some() || note.is_some()).then(|| {
            v_flex()
                .p_1()
                .border_t_1()
                .border_color(theme.border)
                .children(several)
                .children(note)
        });
        popover(cx)
            .id("model-picker-menu")
            .key_context(PICKER_CONTEXT)
            .right_0()
            .w(px(520.))
            .p_0()
            .overflow_hidden()
            .on_mouse_down_out(cx.listener(|this, _, _, cx| this.close_picker(cx)))
            .child(
                h_flex()
                    .items_stretch()
                    .min_h(px(200.))
                    .child(rail)
                    .child(middle)
                    .children(effort_col),
            )
            .children(footer)
            .into_any_element()
    }
}

impl ThreadView {
    /// The picker while several models are ticked: a check row per model, the worktree option, and the way back.
    fn render_compare_picker(
        &self,
        c: &crate::workspace::compare::Compare,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let models: Vec<(String, String)> = self
            .picker_rows(cx)
            .into_iter()
            .filter_map(|(id, label)| Some((id?, label)))
            .collect();
        let error = self.picker_error();
        let search = self.composer.picker.as_ref().map(|p| p.search.clone());
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
        let check = |id: String, on: bool| {
            Checkbox::new(SharedString::from(id))
                .xsmall()
                .decorative()
                .checked(on)
        };
        let full = c.models.len() >= crate::workspace::compare::MAX_MODELS;
        let model_rows = models.iter().map(|(id, label)| {
            let on = c.models.contains(id);
            let model = id.clone();
            row(format!("compare-model-{id}").into())
                .role(gpui::accesskit::Role::CheckBox)
                .aria_label(label.clone())
                .aria_toggled(on.into())
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
            .role(gpui::accesskit::Role::Menu)
            .aria_label("Send to several models")
            .key_context(PICKER_CONTEXT)
            .right_0()
            .w(px(280.))
            .on_mouse_down_out(cx.listener(|this, _, _, cx| this.close_picker(cx)))
            .children(search.map(|s| {
                div().pb_1().child(
                    TextField::new(&s)
                        .small()
                        .accessibility_label("Filter models"),
                )
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
                            .role(gpui::accesskit::Role::CheckBox)
                            .aria_label("In worktrees")
                            .aria_toggled(c.worktrees.into())
                            .child(check("compare-worktrees-check".into(), c.worktrees))
                            .child(div().flex_1().child("In worktrees"))
                            .on_click(
                                cx.listener(|this, _, _, cx| this.toggle_compare_worktrees(cx)),
                            ),
                    )
                    .child(
                        row("compare-stop".into())
                            .role(gpui::accesskit::Role::Button)
                            .aria_label("One model")
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
