use super::widgets::{path_text, row, section};
use super::*;
use crate::ui;
use gpui_component::v_flex;

impl Settings {
    pub(super) fn render_accounts(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let form_title = match &self.editing {
            Some(id) => format!("Edit account '{id}'"),
            None => "Add account".into(),
        };
        let accounts = self.config.accounts.iter().enumerate().map(|(i, a)| {
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
                        .text_size(ui::META)
                        .text_color(theme.muted_foreground)
                        .child(div().flex_shrink_0().child("GitHub:"))
                        .child(path_text(format!("gh-{}", a.id), dir))
                        .child(div().flex_shrink_0().child(format!("· {login}")))
                });
                let log_in = GhLogIn { id: a.id.clone() };
                let has_gh = github.is_some();
                let codex_id = (a.provider == Provider::Codex).then(|| a.id.clone());
                row(i == 0, cx)
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap_0p5()
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .min_w_0()
                                    .child(
                                        div()
                                            .id(SharedString::from(format!("provider-{}", a.id)))
                                            .debug_selector({
                                                let s = format!("provider-{}", a.id);
                                                move || s
                                            })
                                            .min_w_0()
                                            .child(ui::AccountBadge::of(a).chip()),
                                    )
                                    .child(ui::mono(a.id.clone(), theme).text_size(ui::META)),
                            )
                            .child(
                                path_text(format!("dir-{}", a.id), &a.config_dir)
                                    .text_size(ui::META)
                                    .text_color(theme.muted_foreground),
                            )
                            .children(github),
                    )
                    .children(codex_id.map(|id| {
                        let selector = format!("codex-login-{id}");
                        div().debug_selector(move || selector).child(
                        Button::new(SharedString::from(format!("codex-login-{id}")))
                            .label("Log in to Codex")
                            .tooltip("Opens a terminal running codex login with this account's CODEX_HOME")
                            .flex_shrink_0()
                            .small()
                            .ghost()
                            .on_click(cx.listener(move |this, _, _, cx| this.codex_log_in(&id, cx))),
                        )
                    }))
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
                            .ghost()
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.remove_account(&remove, window, cx)
                            })),
                    )
                    .into_any_element()
        });
        let accounts: Vec<_> = accounts.collect();
        let empty = self.config.accounts.is_empty().then(|| {
            row(true, cx)
                .child(ui::EmptyState::new("No accounts yet").hint(
                    "Add one per Claude or Codex login: each gets its own config folder, so logins never mix.",
                ))
                .into_any_element()
        });
        let (muted, border) = (theme.muted, theme.border);
        let form = v_flex()
            .px_4()
            .py_3()
            .gap_2()
            .bg(muted)
            .when(!self.config.accounts.is_empty(), |d| {
                d.border_t_1().border_color(border)
            })
            .child(
                div()
                    .text_size(ui::BODY)
                    .font_weight(gpui::FontWeight::MEDIUM)
                    .child(form_title),
            )
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap_2()
                    // An account's provider is fixed once added (core refuses the change).
                    .when(self.editing.is_none(), |d| {
                        d.children(Provider::ALL.map(|p| self.provider_toggle(p, cx)))
                    })
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
            .into_any_element();
        section(
            "Accounts",
            accounts
                .into_iter()
                .chain(empty)
                .chain(std::iter::once(form)),
            cx,
        )
    }

    pub(super) fn render_bindings(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let badge_of = |id: &str| match self.config.accounts.iter().find(|a| a.id == id) {
            Some(a) => ui::AccountBadge::of(a).chip(),
            None => ui::AccountBadge::new(id.to_string(), id.to_string()).chip(),
        };
        let empty = self.config.bindings.is_empty().then(|| {
            row(true, cx)
                .child(ui::EmptyState::new("No folders bound yet").hint(
                    "Agents won't start anywhere until you bind one. Pick an account below, then a folder.",
                ))
                .into_any_element()
        });
        let bindings = self.config.bindings.iter().enumerate().map(|(i, b)| {
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
                        .text_size(ui::META)
                        .text_color(theme.muted_foreground)
                        .child(div().flex_shrink_0().child(label.to_string()))
                        .child(path_text(format!("{key}-{id}"), path))
                };
                row(i == 0, cx)
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap_0p5()
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .min_w_0()
                                    .child(path_text(format!("path-{}", b.path.display()), &b.path))
                                    .when(stale.is_some(), |d| {
                                        d.child(
                                            ui::Pill::new("Stale")
                                                .color(ui::Status::Stale.color(theme)),
                                        )
                                    }),
                            )
                            .children(warning.map(|why| {
                                div().text_size(ui::META).text_color(theme.warning).child(why)
                            }))
                            .children(now.as_deref().map(|now| labelled("Now:", "now", now)))
                            .children(
                                stale
                                    .and(b.resolved())
                                    .map(|was| labelled("Confirmed:", "was", was)),
                            ),
                    )
                    .child(div().flex_shrink_0().max_w_48().child(badge_of(&b.account)))
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
                            .ghost()
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.remove_binding(&remove, window, cx)
                            })),
                    )
                    .into_any_element()
        });
        let add = div()
            .px_4()
            .py_3()
            .bg(theme.muted)
            .when(!self.config.bindings.is_empty(), |d| {
                d.border_t_1().border_color(theme.border)
            })
            .child(
                div()
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
            .into_any_element();
        section(
            "Folder bindings",
            bindings.chain(empty).chain(std::iter::once(add)),
            cx,
        )
    }

    pub(super) fn render_test(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let result = match &self.test_result {
            None => None,
            Some(Some(a)) => Some(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .text_size(ui::META)
                    .text_color(theme.muted_foreground)
                    .child("Runs as")
                    .child(ui::AccountBadge::of(a).chip())
                    .child(path_text("test-dir".into(), &a.config_dir)),
            ),
            Some(None) => Some(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .text_size(ui::META)
                    .text_color(theme.muted_foreground)
                    .child(ui::AccountBadge::unbound().chip())
                    .child("Agents won't start here"),
            ),
        };
        section(
            "Test folder",
            [row(true, cx)
                .flex_col()
                .items_start()
                .child(div().w_full().child(Input::new(&self.test_input)))
                .children(result)
                .into_any_element()],
            cx,
        )
    }
}

impl Settings {
    /// One of the add form's provider buttons; the picked one is filled.
    fn provider_toggle(&self, p: Provider, cx: &mut Context<Self>) -> gpui::Div {
        let selector = format!("provider-pick-{}", p.label());
        let button = Button::new(SharedString::from(selector.clone()))
            .label(p.label())
            .tooltip("Which agent CLI this account logs in to")
            .on_click(cx.listener(move |this, _, _, cx| {
                this.provider = p;
                cx.notify();
            }));
        div()
            .flex_shrink_0()
            .debug_selector(move || selector)
            .child(if self.provider == p {
                button.primary()
            } else {
                button.ghost()
            })
    }
}
