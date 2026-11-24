//! Settings → Accounts: one card per account (provider mark in the account's color, its name, editable in place,
//! and its config folder in small type), and "Add account" as a card that opens into a short form.

use super::widgets::{heading, path_line};
use super::*;
use crate::ui;
use crate::ui::SkyeIcon;
use crate::ui::TextField;
use gpui::AnyElement;
use gpui_component::{Icon, h_flex, v_flex};

/// An id made from a name: lowercase letters, digits and dashes ("Acme Team" → `acme-team`).
pub(super) fn slug(name: &str) -> String {
    let mut out = String::new();
    for c in name.trim().chars().flat_map(char::to_lowercase) {
        if c.is_ascii_alphanumeric() {
            out.push(c);
        } else if !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
    }
    out.trim_end_matches('-').to_owned()
}

/// The provider's mark: a 32 px tile tinted with the account's color.
pub(crate) fn provider_mark(provider: Provider, color: gpui::Hsla) -> gpui::Div {
    let icon = match provider {
        Provider::Claude => Icon::new(SkyeIcon::Claude),
        Provider::Codex => Icon::new(SkyeIcon::Codex),
    };
    div()
        .flex_shrink_0()
        .size_8()
        .flex()
        .items_center()
        .justify_center()
        .rounded(ui::RADIUS)
        .bg(color.opacity(0.14))
        .text_color(color)
        .child(icon.size_4())
}

impl Settings {
    pub(super) fn save_account(
        &mut self,
        _: &SaveAccount,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let value = |input: &Entity<InputState>| input.read(cx).value().trim().to_string();
        let label = value(&self.label_input);
        let typed_id = value(&self.id_input);
        let id = self.editing.clone().unwrap_or_else(|| {
            if typed_id.is_empty() {
                slug(&label)
            } else {
                typed_id
            }
        });
        let typed_dir = value(&self.dir_input);
        // A new account with no folder typed gets `~/.claude-<id>` (or `.codex-`); the login creates it.
        let config_dir = match dirs::home_dir() {
            Some(home) if typed_dir.is_empty() && self.editing.is_none() && !id.is_empty() => {
                skyelab_core::onboarding::default_config_dir(&home, self.provider, &id)
            }
            _ => PathBuf::from(typed_dir),
        };
        let account = Account {
            id,
            label,
            config_dir,
            provider: self.provider,
            gh_config_dir: Some(value(&self.gh_input))
                .filter(|v| !v.is_empty())
                .map(PathBuf::from),
            ssh_agent: false,
        };
        let adding = self.editing.is_none();
        let saved = self.apply(
            |c| {
                if adding {
                    c.add_account(account)
                } else {
                    // Not in the form (accounts.toml only): an edit keeps what the file says now.
                    let ssh_agent = c
                        .accounts
                        .iter()
                        .find(|a| a.id == account.id)
                        .is_some_and(|a| a.ssh_agent);
                    c.update_account(Account {
                        ssh_agent,
                        ..account
                    })
                }
            },
            window,
            cx,
        );
        if saved {
            self.reset_form(window, cx);
        }
    }

    /// Esc: closes the add card or an edit; with neither open, it closes the modal.
    pub(super) fn cancel_edit(
        &mut self,
        _: &CancelEdit,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.section != Section::Accounts || (self.editing.is_none() && !self.adding) {
            return cx.emit(SettingsEvent::Closed);
        }
        self.reset_form(window, cx);
    }

    /// Closes the add card or edit and empties the form.
    fn reset_form(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.editing = None;
        self.adding = false;
        self.provider = Provider::Claude;
        for input in [
            &self.id_input,
            &self.label_input,
            &self.dir_input,
            &self.gh_input,
        ] {
            input.update(cx, |s, cx| s.set_value("", window, cx));
        }
        cx.notify();
    }

    pub(super) fn edit_account(
        &mut self,
        action: &EditAccount,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(account) = self
            .config
            .accounts
            .iter()
            .find(|a| a.id == action.id)
            .cloned()
        else {
            return;
        };
        self.adding = false;
        self.editing = Some(account.id.clone());
        self.provider = account.provider;
        let dir = account.config_dir.display().to_string();
        let gh = account
            .gh_config_dir
            .as_ref()
            .map(|d| d.display().to_string())
            .unwrap_or_default();
        for (input, text) in [
            (&self.id_input, account.id.clone()),
            (&self.label_input, account.label.clone()),
            (&self.dir_input, dir),
            (&self.gh_input, gh),
        ] {
            input.update(cx, |s, cx| s.set_value(text, window, cx));
        }
        self.label_input.update(cx, |s, cx| s.focus(window, cx));
        cx.notify();
    }

    pub(super) fn remove_account(
        &mut self,
        a: &RemoveAccount,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.apply(|c| c.remove_account(&a.id), window, cx);
    }

    /// Opens the folder picker for the form's config folder field.
    fn pick_config_dir(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let picked = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Use folder".into()),
        });
        self._pick = Some(cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(mut paths))) = picked.await else {
                return;
            };
            let Some(path) = paths.pop() else { return };
            let _ = this.update_in(cx, |this, window, cx| {
                let text = path.display().to_string();
                this.dir_input
                    .update(cx, |s, cx| s.set_value(text, window, cx));
            });
        }));
    }

    pub(super) fn render_accounts(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let cards: Vec<_> = self
            .config
            .accounts
            .iter()
            .map(|a| {
                if self.editing.as_deref() == Some(a.id.as_str()) {
                    self.render_form(Some(a), cx)
                } else {
                    self.render_card(a, cx)
                }
            })
            .collect();
        let add = if self.adding || self.config.accounts.is_empty() {
            self.render_form(None, cx)
        } else {
            self.render_add_card(cx)
        };
        v_flex()
            .gap_3()
            .child(heading(
                "Accounts",
                "One per Claude or Codex login. Each keeps its own config folder, so logins never mix, and every \
                 folder runs on the account it is bound to.",
                cx,
            ))
            .child(v_flex().gap_2p5().children(cards).child(add))
            .into_any_element()
    }

    fn render_card(&self, a: &Account, cx: &mut Context<Self>) -> gpui::AnyElement {
        let theme = cx.theme();
        let color = ui::account_color_in(cx, &a.id);
        let (edit, remove) = (
            EditAccount { id: a.id.clone() },
            RemoveAccount { id: a.id.clone() },
        );
        let (login_id, login_cmd) = (
            a.id.clone(),
            match a.provider {
                Provider::Claude => "claude auth login with this account's CLAUDE_CONFIG_DIR",
                Provider::Codex => "codex login with this account's CODEX_HOME",
            },
        );
        let login_key = format!("{}-login-{}", a.provider.label().to_lowercase(), a.id);
        let selector = format!("provider-{}", a.id);
        v_flex()
            .id(SharedString::from(format!("account-{}", a.id)))
            .gap_2()
            .p_4()
            .rounded(ui::RADIUS_LG)
            .border_1()
            .border_color(theme.border)
            .bg(theme.muted)
            .child(
                h_flex()
                    .gap_3()
                    .child(provider_mark(a.provider, color))
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .child(
                                div()
                                    .id(SharedString::from(format!("name-{}", a.id)))
                                    .truncate()
                                    .text_size(ui::BODY)
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .cursor_pointer()
                                    .child(a.label.clone())
                                    .on_click(cx.listener({
                                        let edit = edit.clone();
                                        move |this, _, window, cx| {
                                            this.edit_account(&edit, window, cx)
                                        }
                                    })),
                            )
                            .child(
                                h_flex()
                                    .debug_selector(move || selector)
                                    .gap_1p5()
                                    .text_size(ui::META)
                                    .text_color(theme.muted_foreground)
                                    .child(ui::StatusDot::new(color))
                                    .child(a.provider.label())
                                    .child("·")
                                    .child(ui::mono(a.id.clone(), theme).text_size(ui::META)),
                            ),
                    )
                    .child({
                        let selector = login_key.clone();
                        div().debug_selector(move || selector).child(
                            Button::new(SharedString::from(login_key))
                                .label("Log in")
                                .tooltip(format!("Opens a terminal running {login_cmd}"))
                                .small()
                                .outline()
                                .on_click(
                                    cx.listener(move |this, _, _, cx| this.log_in(&login_id, cx)),
                                ),
                        )
                    })
                    .child(
                        Button::new(SharedString::from(format!("edit-{}", a.id)))
                            .icon(SkyeIcon::PenLine)
                            .tooltip("Edit")
                            .small()
                            .ghost()
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.edit_account(&edit, window, cx)
                            })),
                    )
                    .child(
                        Button::new(SharedString::from(format!("remove-account-{}", a.id)))
                            .icon(SkyeIcon::Trash)
                            .tooltip("Remove account")
                            .small()
                            .ghost()
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.remove_account(&remove, window, cx)
                            })),
                    ),
            )
            .child(div().pl(gpui::px(44.)).child(path_line(
                format!("dir-{}", a.id),
                &a.config_dir,
                cx,
            )))
            .into_any_element()
    }

    fn render_add_card(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let theme = cx.theme();
        let hover = theme.accent;
        h_flex()
            .id("add-account")
            .gap_3()
            .p_4()
            .rounded(ui::RADIUS_LG)
            .border_1()
            .border_dashed()
            .border_color(theme.input)
            .cursor_pointer()
            .hover(move |s| s.bg(hover))
            .child(
                div()
                    .size_8()
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(ui::RADIUS)
                    .bg(theme.secondary)
                    .child(
                        Icon::new(SkyeIcon::Plus)
                            .size_4()
                            .text_color(theme.muted_foreground),
                    ),
            )
            .child(super::widgets::label(
                "Add account",
                "Another Claude or Codex login, with its own config folder.",
                cx,
            ))
            .on_click(cx.listener(|this, _, window, cx| {
                this.reset_form(window, cx); // an open edit must not take what is typed for the new account
                this.adding = true;
                this.label_input.update(cx, |s, cx| s.focus(window, cx));
                cx.notify();
            }))
            .into_any_element()
    }

    /// The add form (`editing` None) or an account's edit in place: provider, name, config folder, GitHub folder.
    fn render_form(&self, editing: Option<&Account>, cx: &mut Context<Self>) -> gpui::AnyElement {
        let muted = cx.theme().muted_foreground;
        let field = |title: &str, hint: &str, input: AnyElement| {
            v_flex()
                .gap_1p5()
                .child(
                    h_flex()
                        .gap_2()
                        .text_size(ui::META)
                        .child(
                            div()
                                .font_weight(gpui::FontWeight::MEDIUM)
                                .child(title.to_owned()),
                        )
                        .child(div().text_color(muted).child(hint.to_owned())),
                )
                .child(input)
        };
        let title = match editing {
            Some(a) => format!("Edit {}", a.label),
            None => "Add account".into(),
        };
        let providers = editing.is_none().then(|| {
            h_flex()
                .gap_2()
                .children(Provider::ALL.map(|p| self.provider_card(p, cx)))
        });
        let theme = cx.theme();
        v_flex()
            .id("account-form")
            .gap_4()
            .p_4()
            .rounded(ui::RADIUS_LG)
            .border_1()
            .border_color(theme.ring.opacity(0.5))
            .bg(theme.background)
            .child(
                div()
                    .text_size(ui::BODY)
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .child(title),
            )
            .children(providers)
            .child(
                h_flex()
                    .gap_3()
                    .child(div().flex_1().child(field(
                        "Name",
                        "e.g. Work, Personal",
                        TextField::new(&self.label_input).into_any_element(),
                    )))
                    .when(editing.is_none(), |d| {
                        d.child(div().w(gpui::px(180.)).child(field(
                            "Id",
                            "optional",
                            TextField::new(&self.id_input).into_any_element(),
                        )))
                    }),
            )
            .child(field(
                "Config folder",
                "where this login lives",
                h_flex()
                    .gap_2()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .child(TextField::new(&self.dir_input)),
                    )
                    .child(
                        Button::new("pick-config-dir")
                            .outline()
                            .icon(SkyeIcon::FolderOpen)
                            .label("Choose…")
                            .on_click(
                                cx.listener(|this, _, window, cx| this.pick_config_dir(window, cx)),
                            ),
                    )
                    .into_any_element(),
            ))
            .child(field(
                "GitHub folder",
                "optional: its own gh login for push and pull requests",
                TextField::new(&self.gh_input).into_any_element(),
            ))
            .child(
                h_flex()
                    .gap_2()
                    .justify_end()
                    .when(editing.is_some() || !self.config.accounts.is_empty(), |d| {
                        d.child(Button::new("cancel-edit").label("Cancel").ghost().on_click(
                            cx.listener(|this, _, window, cx| {
                                this.cancel_edit(&CancelEdit, window, cx)
                            }),
                        ))
                    })
                    .child(
                        Button::new("save-account")
                            .label(if editing.is_some() {
                                "Save"
                            } else {
                                "Add account"
                            })
                            .primary()
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.save_account(&SaveAccount, window, cx)
                            })),
                    ),
            )
            .into_any_element()
    }

    /// One of the add form's provider choices: a small card, outlined in the accent when picked.
    fn provider_card(&self, p: Provider, cx: &mut Context<Self>) -> gpui::Stateful<gpui::Div> {
        let theme = cx.theme();
        let on = self.provider == p;
        let selector = format!("provider-pick-{}", p.label());
        let about = match p {
            Provider::Claude => "Claude Code (CLAUDE_CONFIG_DIR)",
            Provider::Codex => "Codex CLI (CODEX_HOME)",
        };
        h_flex()
            .id(SharedString::from(selector.clone()))
            .debug_selector(move || selector)
            .flex_1()
            .gap_3()
            .p_3()
            .rounded(ui::RADIUS)
            .border_1()
            .border_color(if on { theme.primary } else { theme.border })
            .when(on, |d| d.bg(theme.primary.opacity(0.06)))
            .cursor_pointer()
            .child(provider_mark(
                p,
                if on {
                    theme.primary
                } else {
                    theme.muted_foreground
                },
            ))
            .child(super::widgets::label(p.label(), about, cx))
            .on_click(cx.listener(move |this, _, _, cx| {
                this.provider = p;
                cx.notify();
            }))
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn slug_makes_an_id_from_a_name() {
        assert_eq!(super::slug("Acme Team (Work)"), "acme-team-work");
        assert_eq!(super::slug("  Personal  "), "personal");
        assert_eq!(super::slug("!!"), "");
    }
}
