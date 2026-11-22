//! The three settings sections. Kept apart from the state and actions in `mod.rs`.

use super::widgets::{path_text, row, section};
use super::*;

impl Settings {
    pub(super) fn render_accounts(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let form_title = match &self.editing {
            Some(id) => format!("Edit account '{id}'"),
            None => "Add account".into(),
        };
        section("Accounts", cx)
            .children(self.config.accounts.iter().map(|a| {
                let edit = EditAccount { id: a.id.clone() };
                let remove = RemoveAccount { id: a.id.clone() };
                row(cx)
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .child(div().truncate().child(format!("{}  ({})", a.label, a.id)))
                            .child(
                                path_text(format!("dir-{}", a.id), &a.config_dir)
                                    .text_sm()
                                    .text_color(theme.muted_foreground),
                            ),
                    )
                    .child(
                        Button::new(SharedString::from(format!("edit-{}", a.id)))
                            .label("Edit")
                            .flex_shrink_0()
                            .small()
                            .ghost()
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.edit_account(&edit, window, cx)
                            })),
                    )
                    .child(
                        Button::new(SharedString::from(format!("remove-account-{}", a.id)))
                            .label("Remove")
                            .flex_shrink_0()
                            .small()
                            .danger()
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.remove_account(&remove, window, cx)
                            })),
                    )
            }))
            .child(div().pt_2().text_sm().child(form_title))
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap_2()
                    .when(self.editing.is_none(), |d| {
                        d.child(div().w_40().child(Input::new(&self.id_input)))
                    })
                    .child(div().w_56().child(Input::new(&self.label_input)))
                    .child(div().flex_1().min_w_64().child(Input::new(&self.dir_input)))
                    .child(
                        Button::new("save-account")
                            .label(if self.editing.is_some() {
                                "Update"
                            } else {
                                "Add"
                            })
                            .primary()
                            .flex_shrink_0()
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.save_account(&SaveAccount, window, cx)
                            })),
                    )
                    .when(self.editing.is_some(), |d| {
                        d.child(
                            Button::new("cancel-edit")
                                .label("Cancel")
                                .ghost()
                                .flex_shrink_0()
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.cancel_edit(&CancelEdit, window, cx)
                                })),
                        )
                    }),
            )
    }

    pub(super) fn render_bindings(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let label_of = |id: &str| {
            self.config
                .accounts
                .iter()
                .find(|a| a.id == id)
                .map_or(id.to_string(), |a| a.label.clone())
        };
        section("Folder bindings", cx)
            .when(self.config.bindings.is_empty(), |d| {
                d.child(
                    div().text_sm().text_color(theme.muted_foreground).child(
                        "No folders bound yet. Agents won't start anywhere until you bind one.",
                    ),
                )
            })
            .children(self.config.bindings.iter().map(|b| {
                let remove = RemoveBinding {
                    path: b.path.clone(),
                };
                let reconfirm = ReconfirmBinding {
                    path: b.path.clone(),
                };
                let stale = self.stale.contains(&b.path).then(|| {
                    let what = b.resolved().map_or(
                        "Not confirmed yet: an older SkyeLab wrote this binding.".into(),
                        |r| format!("Now points somewhere else. You confirmed {}.", r.display()),
                    );
                    format!("{what} Agents won't start under it until you re-confirm.")
                });
                row(cx)
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .child(path_text(format!("path-{}", b.path.display()), &b.path))
                            .children(
                                stale.clone().map(|why| {
                                    div().text_sm().text_color(theme.warning).child(why)
                                }),
                            ),
                    )
                    .child(
                        div()
                            .flex_shrink_0()
                            .max_w_48()
                            .truncate()
                            .text_color(theme.muted_foreground)
                            .child(format!("→  {}", label_of(&b.account))),
                    )
                    .when(stale.is_some(), |d| {
                        d.child(
                            Button::new(SharedString::from(format!(
                                "reconfirm-{}",
                                b.path.display()
                            )))
                            .label("Re-confirm")
                            .flex_shrink_0()
                            .small()
                            .warning()
                            .on_click(cx.listener(
                                move |this, _, window, cx| {
                                    this.reconfirm_binding(&reconfirm, window, cx)
                                },
                            )),
                        )
                    })
                    .child(
                        Button::new(SharedString::from(format!("unbind-{}", b.path.display())))
                            .label("Remove")
                            .flex_shrink_0()
                            .small()
                            .danger()
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.remove_binding(&remove, window, cx)
                            })),
                    )
            }))
            .child(
                div()
                    .pt_2()
                    .flex()
                    .gap_2()
                    .items_center()
                    .child(
                        div().w_56().child(
                            Select::new(&self.account_select)
                                .id("binding-account")
                                .placeholder("Account")
                                .accessibility_label("Account for new binding"),
                        ),
                    )
                    .child(
                        Button::new("bind-folder")
                            .label("Bind folder…")
                            .primary()
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.bind_folder(&BindFolder, window, cx)
                            })),
                    ),
            )
    }

    pub(super) fn render_test(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let result = match &self.test_result {
            None => None,
            Some(Some(a)) => Some(div().text_color(theme.success).child(format!(
                "Runs as {} ({})",
                a.label,
                a.config_dir.display()
            ))),
            Some(None) => Some(
                div()
                    .text_color(theme.warning)
                    .child("Unbound, agents won't start here"),
            ),
        };
        section("Test folder", cx)
            .child(Input::new(&self.test_input))
            .children(result)
    }
}
