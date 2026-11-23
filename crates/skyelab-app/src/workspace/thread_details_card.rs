//! The details card's look: its sections and pages, and the Environment, Actions and Editor rows.

use super::super::thread::ThreadView;
use super::{On, OpenInEditorWith, Page, editor_name, is_running};
use crate::ui;
use gpui::{
    AnyElement, App, ClickEvent, Context, FontWeight, SharedString, Window, div, prelude::*, px,
};
use gpui_component::{
    ActiveTheme as _, Icon, Sizable as _, Theme,
    button::{Button, ButtonVariants as _},
    h_flex, v_flex,
};
use gpui_kit_assets::IconName;

impl ThreadView {
    /// The card's content: a picker page when one is open, else its titled sections with `usage` (every account's,
    /// drawn by the details card) among them.
    /// Every field read here (and in the helpers it calls) is in `DetailsSig`, or the card won't redraw for it.
    pub(in crate::workspace) fn render_details(
        &self,
        usage: AnyElement,
        on: &On,
        cx: &App,
    ) -> AnyElement {
        let page = v_flex().w_full();
        if self.branch_picker.read(cx).open {
            let picker = self.branch_picker.clone();
            return page
                .child(self.back_row("Branch", on, cx, move |_, cx| {
                    picker.update(cx, |p, cx| p.set_open(false, cx))
                }))
                .child(self.branch_picker.clone())
                .into_any_element();
        }
        if self.scripts.read(cx).open {
            let scripts = self.scripts.clone();
            return page
                .child(self.back_row("Project scripts", on, cx, move |_, cx| {
                    scripts.update(cx, |m, cx| m.set_open(false, cx))
                }))
                .child(self.scripts.clone())
                .into_any_element();
        }
        match self.details.page {
            Page::Editors => page
                .child(self.back_row("Open in", on, cx, |v, cx| v.details_page(Page::Main, cx)))
                .children(self.render_editor_choices(on, cx))
                .into_any_element(),
            Page::Git => page
                .child(self.back_row("Commit & push", on, cx, |v, cx| {
                    v.details_page(Page::Main, cx)
                }))
                .children(self.render_git_menu(on, cx))
                .into_any_element(),
            Page::LinkPr => page
                .child(self.back_row("Link pull request", on, cx, |v, cx| {
                    v.details_page(Page::Main, cx)
                }))
                .children(self.render_link_pr(on, cx))
                .into_any_element(),
            Page::Previous => page
                .child(self.back_row("Previous agents", on, cx, |v, cx| {
                    v.details_page(Page::Main, cx)
                }))
                .children(
                    self.children
                        .iter()
                        .filter(|c| !is_running(c.status))
                        .map(|c| self.agent_row(c, on, cx)),
                )
                .into_any_element(),
            Page::Main => {
                let lineage = self.render_lineage(on, cx);
                // T3's card names only its lists (Lineage); the rows say what they are.
                let titled = |s: Option<(SharedString, AnyElement)>| s.map(|(t, b)| (Some(t), b));
                let sections = [
                    Some((None, self.render_environment(on, cx))),
                    Some((None, self.render_actions(on, cx))),
                    titled(lineage),
                    titled(self.render_side_chats(on, cx)),
                    Some((None, usage)),
                    self.render_pr(on, cx)
                        .map(|pr| (Some("Pull request".into()), pr)),
                    Some((None, self.render_editor(on, cx))),
                ];
                let theme = cx.theme();
                page.children(
                    sections
                        .into_iter()
                        .flatten()
                        .enumerate()
                        .map(|(i, (title, body))| section(i == 0, title, body, theme)),
                )
                .into_any_element()
            }
        }
    }

    /// This machine (with the folder button), then the branch and git rows on a repo.
    fn render_environment(&self, on: &On, cx: &App) -> AnyElement {
        let theme = cx.theme();
        let machine = self
            .details
            .machine
            .clone()
            .unwrap_or_else(|| "This machine".into());
        let cwd = self.cwd.clone();
        v_flex()
            .child(
                row("details-machine", IconName::Monitor, machine, theme)
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
            .children(self.render_git_rows(on, cx))
            .into_any_element()
    }

    /// "Side chats": each one reopens its pane, and "New side chat" starts one. Only on a stored local thread that
    /// isn't a side chat itself.
    fn render_side_chats(&self, on: &On, cx: &App) -> Option<(SharedString, AnyElement)> {
        use crate::workspace::side_chat::SideAsk;
        use crate::workspace::thread::ThreadViewEvent;
        self.thread.as_ref().filter(|t| t.side_of.is_none())?;
        let theme = cx.theme();
        let chats = self.details.side_chats.iter().map(|(id, title)| {
            let id = *id;
            row(
                ("details-side-chat", id as u64),
                IconName::MessagesSquare,
                title.clone(),
                theme,
            )
            .on_click(on.click(move |_, _, cx| cx.emit(ThreadViewEvent::Side(SideAsk::Open(id)))))
        });
        let new = row(
            "details-new-side-chat",
            IconName::Plus,
            "New side chat",
            theme,
        )
        .on_click(on.click(|_, _, cx| cx.emit(ThreadViewEvent::Side(SideAsk::New(None)))));
        let body = v_flex().children(chats).child(new).into_any_element();
        Some(("Side chats".into(), body))
    }

    fn render_actions(&self, on: &On, cx: &App) -> AnyElement {
        let theme = cx.theme();
        let no_scripts = self.scripts.read(cx).scripts.is_empty();
        let (icon, label) = if no_scripts {
            (IconName::Plus, "Add project script")
        } else {
            (IconName::Play, "Run project script")
        };
        row("details-scripts", icon, label, theme)
            .on_click(on.click(|this, _, cx| {
                this.scripts.update(cx, |m, cx| m.set_open(true, cx));
                cx.notify();
            }))
            .into_any_element()
    }

    fn render_editor(&self, on: &On, cx: &App) -> AnyElement {
        let theme = cx.theme();
        let editors = self.details.editors.as_ref();
        let open_in = match editors.and_then(|e| e.current.as_deref()) {
            Some(bin) => format!("Open in {}", editor_name(bin)),
            None if editors.is_some() => "No editor found".into(),
            None => "Open in editor".into(),
        };
        let can_choose = editors.is_some_and(|e| !e.choices.is_empty());
        row("details-open-in", IconName::SquareCode, open_in, theme)
            .on_click(on.click(|this, window, cx| {
                this.details_home(cx);
                window.dispatch_action(Box::new(super::super::attention::OpenInEditor), cx);
            }))
            .when(can_choose, |r| {
                r.child(chevron_button(
                    "details-editor-choices",
                    IconName::ChevronDown,
                    on.click(|this, _, cx| this.details_page(Page::Editors, cx)),
                ))
            })
            .into_any_element()
    }

    fn render_editor_choices(&self, on: &On, cx: &App) -> Vec<AnyElement> {
        let theme = cx.theme();
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
                    theme,
                )
                .when(current, |r| {
                    r.child(
                        Icon::new(IconName::Check)
                            .size_4()
                            .text_color(theme.muted_foreground),
                    )
                })
                .on_click(on.click(move |this, window, cx| {
                    this.details_home(cx);
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
        on: &On,
        cx: &App,
        back: impl Fn(&mut ThreadView, &mut Context<ThreadView>) + 'static,
    ) -> AnyElement {
        let theme = cx.theme();
        row("details-back", IconName::ChevronLeft, title, theme)
            .on_click(on.click(move |this, _, cx| back(this, cx)))
            .into_any_element()
    }
}

/// One card section, titled when it is a list; every one but the first starts with a hairline.
fn section(
    first: bool,
    title: Option<SharedString>,
    body: AnyElement,
    theme: &Theme,
) -> impl IntoElement {
    v_flex()
        .when(!first, |d| {
            d.mt_1p5().pt_1p5().border_t_1().border_color(theme.border)
        })
        .children(title.map(|title| {
            div()
                .h(px(28.))
                .px_2p5()
                .flex()
                .items_center()
                .text_size(ui::META)
                .font_weight(FontWeight::MEDIUM)
                .text_color(theme.muted_foreground)
                .child(title)
        }))
        .child(body)
}

/// T3's details row: 32 tall, a 16 px muted icon, a 13 px medium label; hover fills.
pub(super) fn row(
    id: impl Into<gpui::ElementId>,
    icon: IconName,
    label: impl Into<SharedString>,
    theme: &Theme,
) -> gpui::Stateful<gpui::Div> {
    tinted_row(id, icon, theme.muted_foreground, label, theme)
}

/// [`row`] with its icon in `color` (a PR's state, a check's result).
pub(super) fn tinted_row(
    id: impl Into<gpui::ElementId>,
    icon: IconName,
    color: gpui::Hsla,
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
        .child(Icon::new(icon).size_4().flex_shrink_0().text_color(color))
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
