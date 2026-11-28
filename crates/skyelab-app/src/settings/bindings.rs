//! Settings → Folder bindings: folder bindings as folder → account rows, a stale binding explained in one
//! sentence with Re-confirm, and a field that tells which account a folder runs on.

use super::widgets::{path_line, path_text, section};
use super::*;
use crate::ui;
use crate::ui::PopupMenuItem;
use crate::ui::SkyeIcon;
use crate::ui::TextField;
use crate::ui::{Icon, h_flex, v_flex};

/// Why a stale binding stops agents, in one sentence.
fn stale_reason(confirmed: bool, resolves: bool) -> &'static str {
    match (confirmed, resolves) {
        (_, false) => {
            "This folder can't be found now (moved, or a broken link). Agents won't start here until you fix or remove it."
        }
        (false, true) => {
            "An older SkyeLab bound this folder without confirming it. Re-confirm to let agents start here."
        }
        (true, true) => {
            "This path now leads to a different folder than the one you confirmed. Re-confirm if that's intended."
        }
    }
}

impl Settings {
    pub(super) fn remove_binding(
        &mut self,
        a: &RemoveBinding,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.apply(|c| c.remove_binding(&a.path), window, cx);
    }

    pub(super) fn reconfirm_binding(
        &mut self,
        a: &ReconfirmBinding,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.apply(|c| c.reconfirm_binding(&a.path, &a.now), window, cx);
    }

    pub(super) fn render_projects(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        v_flex()
            .gap_8()
            .child(self.render_bindings(cx))
            .child(self.render_test(cx))
            .into_any_element()
    }

    fn render_bindings(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let badge_of = |id: &str| match self.config.accounts.iter().find(|a| a.id == id) {
            Some(a) => ui::AccountBadge::of(a).chip(),
            None => ui::AccountBadge::new(id.to_string(), id.to_string()).chip(),
        };
        let rows: Vec<_> = self
            .config
            .bindings
            .iter()
            .enumerate()
            .map(|(i, b)| {
                let theme = cx.theme();
                let remove = RemoveBinding {
                    path: b.path.clone(),
                };
                let stale = self.stale.iter().find(|(p, _)| *p == b.path);
                let now = stale.and_then(|(_, now)| now.clone());
                let reconfirm = now.clone().map(|now| ReconfirmBinding {
                    path: b.path.clone(),
                    now,
                });
                let id = b.path.display().to_string();
                let name = b
                    .path
                    .file_name()
                    .map_or_else(|| id.clone(), |n| n.to_string_lossy().into_owned());
                let warn = ui::ink(theme, theme.warning);
                let reconfirm = reconfirm.map(|reconfirm| {
                    Button::new(SharedString::from(format!("reconfirm-{id}")))
                        .label("Re-confirm")
                        .xsmall()
                        .warning()
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.reconfirm_binding(&reconfirm, window, cx)
                        }))
                });
                let theme = cx.theme();
                let details = stale.map(|_| {
                    v_flex()
                        .items_start()
                        .gap_1()
                        .pt_1()
                        .child(
                            div()
                                .text_size(ui::META)
                                .text_color(warn)
                                .child(stale_reason(b.resolved().is_some(), now.is_some())),
                        )
                        .children(now.as_deref().filter(|n| *n != b.path).map(|n| {
                            h_flex()
                                .gap_1()
                                .text_size(ui::META)
                                .text_color(theme.muted_foreground)
                                .child(div().flex_shrink_0().child("Now leads to"))
                                .child(path_text(format!("now-{id}"), n))
                        }))
                        .children(reconfirm)
                });
                super::widgets::row(i == 0, cx)
                    .items_start()
                    .gap_3()
                    .child(
                        div().pt_0p5().child(
                            Icon::new(SkyeIcon::Folder)
                                .size_4()
                                .text_color(theme.muted_foreground),
                        ),
                    )
                    .child(
                        v_flex()
                            .flex_1()
                            // Room for a short path ("~/work") before the account column gives way.
                            .min_w(gpui::px(120.))
                            .gap_0p5()
                            .child(
                                h_flex()
                                    .gap_2()
                                    .min_w_0()
                                    .child(
                                        div()
                                            .truncate()
                                            .font_weight(gpui::FontWeight::MEDIUM)
                                            .child(name),
                                    )
                                    .when(stale.is_some(), |d| {
                                        d.child(
                                            ui::Pill::new("Stale")
                                                .color(ui::Status::Stale.color(theme)),
                                        )
                                    }),
                            )
                            .child(path_line(format!("path-{id}"), &b.path, cx))
                            .children(details),
                    )
                    .child(
                        // A fixed column, so every row's arrow and account line up.
                        // It gives way on a narrow dialog before the folder's path does.
                        h_flex()
                            .w(gpui::px(200.))
                            .min_w(gpui::px(120.))
                            .gap_2()
                            .child(
                                Icon::new(SkyeIcon::ArrowRight)
                                    .size_3p5()
                                    .text_color(theme.muted_foreground),
                            )
                            .child(div().max_w_48().child(badge_of(&b.account))),
                    )
                    .child(
                        // Centred on the name and account line (20 px) rather than 4 px under it.
                        div().mt(gpui::px(-4.)).child(
                            Button::new(SharedString::from(format!("unbind-{id}")))
                                .icon(SkyeIcon::Unlink)
                                .tooltip("Remove binding")
                                .small()
                                .ghost()
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.remove_binding(&remove, window, cx)
                                })),
                        ),
                    )
                    .into_any_element()
            })
            .collect();
        let empty = rows.is_empty().then(|| {
            super::widgets::row(true, cx)
                .justify_center()
                .py_8()
                .child(ui::EmptyState::new("No folders bound yet").hint(
                    "Agents won't start anywhere until you bind one. Pick an account below, then a folder.",
                ))
                .into_any_element()
        });
        let choices = account_ids(&self.config);
        let picked = self.binding_account.clone();
        let account_label = picked
            .as_ref()
            .and_then(|id| choices.iter().find(|c| &c.id == id))
            .map(|c| c.label.clone())
            .unwrap_or_else(|| SharedString::from("Account"));
        let view = cx.entity().downgrade();
        let theme = cx.theme();
        let add = h_flex()
            .px_4()
            .py_3()
            .gap_2()
            .border_t_1()
            .border_color(theme.border)
            .child(
                div()
                    .text_size(ui::META)
                    .text_color(theme.muted_foreground)
                    .child("Bind a folder to"),
            )
            .child(
                Button::new("binding-account")
                    .outline()
                    .small()
                    .accessibility_label(format!("Account for new binding: {account_label}"))
                    .label(account_label)
                    .tooltip("Account for new binding")
                    .disabled(choices.is_empty())
                    .dropdown_caret(true)
                    .dropdown_menu(move |mut menu, _, _| {
                        for choice in choices.clone() {
                            let view = view.clone();
                            let on = picked.as_ref() == Some(&choice.id);
                            let id = choice.id.clone();
                            menu =
                                menu.item(PopupMenuItem::new(choice.label).checked(on).on_click(
                                    move |_, _, cx| {
                                        let id = id.clone();
                                        let _ = view.update(cx, |this, cx| {
                                            this.binding_account = Some(id);
                                            cx.notify();
                                        });
                                    },
                                ));
                        }
                        menu
                    }),
            )
            .child(div().flex_1())
            .child(
                Button::new("bind-folder")
                    .label("Choose folder…")
                    .icon(SkyeIcon::FolderOpen)
                    .small()
                    .primary()
                    .on_click(
                        cx.listener(|this, _, window, cx| {
                            this.bind_folder(&BindFolder, window, cx)
                        }),
                    ),
            )
            .into_any_element();
        section(
            "Folder bindings",
            "Every folder under a binding runs on that account. The deepest binding wins; nothing falls back to a default.",
            rows.into_iter().chain(empty).chain(std::iter::once(add)),
            cx,
        )
    }

    fn render_test(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let result = match &self.test_result {
            None => None,
            Some(Some(a)) => Some(
                h_flex()
                    .gap_2()
                    .text_size(ui::META)
                    .text_color(theme.muted_foreground)
                    .child("Runs as")
                    .child(ui::AccountBadge::of(a).chip()),
            ),
            Some(None) => Some(
                h_flex()
                    .gap_2()
                    .text_size(ui::META)
                    .text_color(theme.muted_foreground)
                    .child(ui::AccountBadge::unbound().chip())
                    .child("Agents won't start here until you bind it"),
            ),
        };
        section(
            "Test a folder",
            "Type a path to see which account its agents would run on.",
            [super::widgets::row(true, cx)
                .flex_col()
                .items_start()
                .gap_2()
                .child(div().w_full().child(
                    TextField::new(&self.test_input).accessibility_label("Folder path to test"),
                ))
                .children(result)
                .into_any_element()],
            cx,
        )
    }
}
