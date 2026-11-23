//! The details card's look: the header toggle, the card, its pages, and the workspace rows.

use super::super::thread::ThreadView;
use super::{OpenInEditorWith, Page, ToggleThreadDetails, editor_name, is_running};
use crate::ui;
use gpui::{
    AnyElement, ClickEvent, Context, FontWeight, SharedString, Window, div, prelude::*, px,
};
use gpui_component::{
    ActiveTheme as _, Icon, Sizable as _, Theme,
    button::{Button, ButtonVariants as _},
    h_flex,
    popover::Popover,
    v_flex,
};
use gpui_kit_assets::IconName;

impl ThreadView {
    /// The header's toggle and the card it opens.
    pub(in crate::workspace) fn render_details_toggle(&self, cx: &mut Context<Self>) -> AnyElement {
        let (open_owner, card_owner) = (cx.weak_entity(), cx.weak_entity());
        Popover::new("thread-details")
            .anchor(gpui::Anchor::TopRight)
            .open(self.details.open)
            .on_open_change(move |open, _, cx| {
                let _ = open_owner.update(cx, |v, cx| v.set_details(*open, cx));
            })
            .trigger(
                Button::new("thread-details-toggle")
                    .ghost()
                    .xsmall()
                    .icon(Icon::new(IconName::SquareMenu))
                    .toggled(self.details.open)
                    .tooltip_with_action(
                        "Thread details",
                        &ToggleThreadDetails,
                        Some(super::super::thread::CONTEXT),
                    ),
            )
            .w(px(320.))
            .p_1p5()
            .rounded(ui::RADIUS_XL)
            .content(move |_, window, cx| match card_owner.upgrade() {
                Some(v) => v.update(cx, |v, cx| v.render_details(window, cx)),
                None => div().into_any_element(),
            })
            .into_any_element()
    }

    fn render_details(&mut self, _: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let card = v_flex()
            .id("thread-details-card")
            .debug_selector(|| "thread-details-card".into())
            .w_full();
        if self.branch_picker.read(cx).open {
            let picker = self.branch_picker.clone();
            return card
                .child(self.back_row("Branch", cx, move |_, cx| {
                    picker.update(cx, |p, cx| p.set_open(false, cx))
                }))
                .child(self.branch_picker.clone())
                .into_any_element();
        }
        if self.scripts.read(cx).open {
            let scripts = self.scripts.clone();
            return card
                .child(self.back_row("Project scripts", cx, move |_, cx| {
                    scripts.update(cx, |m, cx| m.set_open(false, cx))
                }))
                .child(self.scripts.clone())
                .into_any_element();
        }
        match self.details.page {
            Page::Editors => card
                .child(self.back_row("Open in", cx, |v, cx| v.details_page(Page::Main, cx)))
                .children(self.render_editor_choices(cx))
                .into_any_element(),
            Page::Previous => card
                .child(self.back_row("Previous agents", cx, |v, cx| {
                    v.details_page(Page::Main, cx)
                }))
                .children(
                    self.children
                        .iter()
                        .filter(|c| !is_running(c.status))
                        .map(|c| self.agent_row(c, cx)),
                )
                .into_any_element(),
            Page::Main => {
                let sections: Vec<AnyElement> = [
                    Some(self.render_workspace_rows(cx)),
                    self.render_git_rows(cx),
                    self.render_lineage(cx),
                    self.render_usage(cx),
                ]
                .into_iter()
                .flatten()
                .collect();
                let border = cx.theme().border;
                card.children(sections.into_iter().enumerate().map(|(i, s)| {
                    div()
                        .when(i > 0, |d| {
                            d.mt_1p5().pt_1p5().border_t_1().border_color(border)
                        })
                        .child(s)
                }))
                .into_any_element()
            }
        }
    }

    fn render_workspace_rows(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let machine = self
            .details
            .machine
            .clone()
            .unwrap_or_else(|| "This machine".into());
        let cwd = self.cwd.clone();
        let editors = self.details.editors.as_ref();
        let open_in = match editors.and_then(|e| e.current.as_deref()) {
            Some(bin) => format!("Open in {}", editor_name(bin)),
            None if editors.is_some() => "No editor found".into(),
            None => "Open in editor".into(),
        };
        let can_choose = editors.is_some_and(|e| !e.choices.is_empty());
        let no_scripts = self.scripts.read(cx).scripts.is_empty();
        let scripts = self.scripts.clone();
        v_flex()
            .child(
                row("details-machine", IconName::Monitor, machine, &theme)
                    .cursor_default()
                    .child(
                        Button::new("details-open-folder")
                            .ghost()
                            .xsmall()
                            .icon(Icon::new(IconName::FolderOpen))
                            .tooltip("Open the thread's folder")
                            .on_click(move |_, _, cx| cx.open_with_system(&cwd)),
                    ),
            )
            .child(
                row("details-open-in", IconName::SquareCode, open_in, &theme)
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.set_details(false, cx);
                        window.dispatch_action(Box::new(super::super::attention::OpenInEditor), cx);
                    }))
                    .when(can_choose, |r| {
                        r.child(chevron_button(
                            "details-editor-choices",
                            IconName::ChevronDown,
                            cx.listener(|this, _, _, cx| this.details_page(Page::Editors, cx)),
                        ))
                    }),
            )
            .child(
                row(
                    "details-scripts",
                    if no_scripts {
                        IconName::Plus
                    } else {
                        IconName::Play
                    },
                    if no_scripts {
                        "Add project script"
                    } else {
                        "Run project script"
                    },
                    &theme,
                )
                .on_click(move |_, _, cx| scripts.update(cx, |m, cx| m.set_open(true, cx))),
            )
            .into_any_element()
    }

    fn render_editor_choices(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let theme = cx.theme().clone();
        let Some(editors) = &self.details.editors else {
            return Vec::new();
        };
        editors
            .choices
            .iter()
            .map(|bin| {
                let current = editors.current.as_deref() == Some(bin.as_str());
                let editor = bin.clone();
                row(
                    SharedString::from(format!("details-editor-{bin}")),
                    IconName::SquareCode,
                    editor_name(bin),
                    &theme,
                )
                .when(current, |r| {
                    r.child(
                        Icon::new(IconName::Check)
                            .size_4()
                            .text_color(theme.muted_foreground),
                    )
                })
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.set_details(false, cx);
                    window.dispatch_action(
                        Box::new(OpenInEditorWith {
                            editor: editor.clone(),
                        }),
                        cx,
                    );
                }))
                .into_any_element()
            })
            .collect()
    }

    fn back_row(
        &self,
        title: &'static str,
        cx: &mut Context<Self>,
        back: impl Fn(&mut ThreadView, &mut Context<ThreadView>) + 'static,
    ) -> AnyElement {
        let theme = cx.theme().clone();
        row("details-back", IconName::ChevronLeft, title, &theme)
            .on_click(cx.listener(move |this, _, _, cx| back(this, cx)))
            .into_any_element()
    }
}

/// T3's details row: 32 tall, a 16 px muted icon, a 13 px medium label; hover fills.
pub(super) fn row(
    id: impl Into<gpui::ElementId>,
    icon: IconName,
    label: impl Into<SharedString>,
    theme: &Theme,
) -> gpui::Stateful<gpui::Div> {
    h_flex()
        .id(id)
        .h(ui::ROW_H)
        .px_2p5()
        .gap_2p5()
        .rounded(ui::RADIUS)
        .cursor_pointer()
        .hover(|s| s.bg(theme.accent))
        .text_size(ui::CODE)
        .font_weight(FontWeight::MEDIUM)
        .text_color(theme.foreground.opacity(0.8))
        .child(
            Icon::new(icon)
                .size_4()
                .flex_shrink_0()
                .text_color(theme.muted_foreground),
        )
        .child(div().flex_1().min_w_0().truncate().child(label.into()))
}

pub(super) fn chevron(icon: IconName, theme: &Theme) -> Icon {
    Icon::new(icon).size_4().text_color(theme.muted_foreground)
}

fn chevron_button(
    id: &'static str,
    icon: IconName,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut gpui::App) + 'static,
) -> Button {
    Button::new(id)
        .ghost()
        .xsmall()
        .icon(Icon::new(icon))
        .on_click(move |e, window, cx| {
            cx.stop_propagation();
            on_click(e, window, cx)
        })
}
