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
                let github = a.gh_config_dir.as_ref().map(|dir| {
                    let login = match (self.gh_logins.get(&a.id), self.gh_errors.get(&a.id)) {
                        (Some(Some(login)), _) => format!("logged in as {login}"),
                        (Some(None), _) => "not logged in".to_string(),
                        (None, Some(e)) => format!("couldn't check: {e}"),
                        (None, None) => "checking…".to_string(),
                    };
                    div()
                        .flex()
                        .gap_1()
                        .text_sm()
                        .text_color(theme.muted_foreground)
                        .child(div().flex_shrink_0().child("GitHub:"))
                        .child(path_text(format!("gh-{}", a.id), dir))
                        .child(div().flex_shrink_0().child(format!("· {login}")))
                });
                let log_in = GhLogIn { id: a.id.clone() };
                let has_gh = github.is_some();
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
                            )
                            .children(github),
                    )
                    .when(has_gh, |d| {
                        d.child(
                            Button::new(SharedString::from(format!("gh-login-{}", a.id)))
                                .label("Log in to GitHub")
                                .tooltip("Opens a terminal running gh auth login for this account")
                                .flex_shrink_0()
                                .small()
                                .ghost()
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.gh_log_in(&log_in, window, cx)
                                })),
                        )
                    })
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
                    .child(div().flex_1().min_w_64().child(Input::new(&self.gh_input)))
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
                let stale = self.stale.iter().find(|(p, _)| *p == b.path);
                let now = stale.and_then(|(_, now)| now.clone());
                let reconfirm = now.clone().map(|now| ReconfirmBinding {
                    path: b.path.clone(),
                    now,
                });
                let warning = stale.map(|_| match (b.resolved(), &now) {
                    (_, None) => "Can't be resolved now (a broken link?). Agents won't start under it; fix the folder or remove the binding.",
                    (None, Some(_)) => "Not confirmed yet: an older SkyeLab wrote this binding. Agents won't start under it until you re-confirm the folder below.",
                    (Some(_), Some(_)) => "Not the folder you confirmed any more. Agents won't start under it until you re-confirm the folder below.",
                });
                let id = b.path.display().to_string();
                let labelled = |label: &str, key: &str, path: &Path| {
                    div()
                        .flex()
                        .gap_1()
                        .text_sm()
                        .text_color(theme.muted_foreground)
                        .child(div().flex_shrink_0().child(label.to_string()))
                        .child(path_text(format!("{key}-{id}"), path))
                };
                row(cx)
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .child(path_text(format!("path-{}", b.path.display()), &b.path))
                            .children(
                                warning.map(|why| {
                                    div().text_sm().text_color(theme.warning).child(why)
                                }),
                            )
                            .children(now.as_deref().map(|now| labelled("Now:", "now", now)))
                            .children(
                                stale
                                    .and(b.resolved())
                                    .map(|was| labelled("Confirmed:", "was", was)),
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
                    .when_some(reconfirm, |d, reconfirm| {
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
