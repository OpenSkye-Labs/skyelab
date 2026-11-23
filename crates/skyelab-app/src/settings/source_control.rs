//! Settings → Source control: each account's GitHub login (its own `GH_CONFIG_DIR`), used to push and open pull
//! requests from its folders, and the Codex login terminal.

use super::widgets::{path_line, section};
use super::*;
use crate::ui;
use gpui_component::{h_flex, v_flex};

impl Settings {
    /// Asks `gh` off the UI thread who is logged in to each account's GitHub config dir.
    pub fn check_gh_logins(&mut self, cx: &mut Context<Self>) {
        let dirs: Vec<(String, PathBuf)> = self
            .config
            .accounts
            .iter()
            .filter_map(|a| Some((a.id.clone(), a.gh_config_dir.clone()?)))
            .collect();
        let gh = crate::workspace::gh_bin(cx);
        let task = cx.background_spawn(async move {
            dirs.into_iter()
                .map(|(id, dir)| {
                    let login = skyelab_git::gh_login(&skyelab_git::GitHub {
                        gh: &gh,
                        config_dir: &dir,
                    });
                    (id, login.map_err(|e| e.to_string()))
                })
                .collect::<Vec<_>>()
        });
        self._gh_check = Some(cx.spawn(async move |this, cx| {
            let checked = task.await;
            let _ = this.update(cx, |this, cx| {
                this.gh_logins.clear();
                this.gh_errors.clear();
                for (id, login) in checked {
                    match login {
                        Ok(login) => {
                            this.gh_logins.insert(id, login);
                        }
                        Err(e) => {
                            this.gh_errors.insert(id, e);
                        }
                    }
                }
                cx.notify();
            });
        }));
    }

    pub(super) fn gh_log_in(&mut self, a: &GhLogIn, _: &mut Window, cx: &mut Context<Self>) {
        let dir = self
            .config
            .accounts
            .iter()
            .find(|acct| acct.id == a.id)
            .and_then(|acct| acct.gh_config_dir.clone());
        match dir {
            Some(dir) => cx.emit(SettingsEvent::GhLogin { dir }),
            None => {
                self.error = Some(format!("set a GitHub config dir for '{}' first", a.id));
                cx.notify();
            }
        }
    }

    /// Asks for a terminal running `codex login` in a Codex account's `CODEX_HOME`.
    pub(crate) fn codex_log_in(&mut self, id: &str, cx: &mut Context<Self>) {
        let dir = self
            .config
            .accounts
            .iter()
            .find(|a| a.id == id && a.provider == Provider::Codex)
            .map(|a| a.config_dir.clone());
        match dir {
            Some(dir) => cx.emit(SettingsEvent::CodexLogin { dir }),
            None => {
                self.error = Some(format!("'{id}' is not a Codex account"));
                cx.notify();
            }
        }
    }

    pub(super) fn render_github(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let rows: Vec<_> = self
            .config
            .accounts
            .iter()
            .enumerate()
            .map(|(i, a)| {
                let theme = cx.theme();
                let (dot, status) = match (
                    &a.gh_config_dir,
                    self.gh_logins.get(&a.id),
                    self.gh_errors.get(&a.id),
                ) {
                    (None, _, _) => (
                        theme.muted_foreground,
                        "No GitHub folder: edit the account to add one".to_owned(),
                    ),
                    (Some(_), Some(Some(login)), _) => {
                        (theme.success, format!("Signed in as {login}"))
                    }
                    (Some(_), Some(None), _) => (theme.warning, "Not signed in".to_owned()),
                    (Some(_), None, Some(_)) => {
                        (theme.danger, "Couldn't check the login".to_owned())
                    }
                    (Some(_), None, None) => (theme.muted_foreground, "Checking…".to_owned()),
                };
                let error = self.gh_errors.get(&a.id).cloned();
                let log_in = GhLogIn { id: a.id.clone() };
                let control = a.gh_config_dir.as_ref().map(|_| {
                    Button::new(SharedString::from(format!("gh-login-{}", a.id)))
                        .label("Log in")
                        .tooltip("Opens a terminal running gh auth login for this account")
                        .small()
                        .outline()
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.gh_log_in(&log_in, window, cx)
                        }))
                });
                let theme = cx.theme();
                super::widgets::row(i == 0, cx)
                    .gap_4()
                    .child(super::accounts::provider_mark(
                        a.provider,
                        ui::account_color_in(cx, &a.id),
                    ))
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .gap_0p5()
                            .child(
                                h_flex()
                                    .gap_2()
                                    .child(
                                        div()
                                            .font_weight(gpui::FontWeight::MEDIUM)
                                            .child(a.label.clone()),
                                    )
                                    .child(
                                        h_flex()
                                            .id(SharedString::from(format!("gh-status-{}", a.id)))
                                            .gap_1p5()
                                            .text_size(ui::META)
                                            .text_color(theme.muted_foreground)
                                            .child(ui::StatusDot::new(dot))
                                            .child(status)
                                            .when_some(error, |d, e| {
                                                d.tooltip(move |window, cx| {
                                                    gpui_component::tooltip::Tooltip::new(e.clone())
                                                        .build(window, cx)
                                                })
                                            }),
                                    ),
                            )
                            .children(
                                a.gh_config_dir
                                    .as_ref()
                                    .map(|dir| path_line(format!("gh-{}", a.id), dir, cx)),
                            ),
                    )
                    .children(control)
                    .into_any_element()
            })
            .collect();
        let empty = rows.is_empty().then(|| {
            super::widgets::row(true, cx)
                .justify_center()
                .py_8()
                .child(
                    ui::EmptyState::new("No accounts yet")
                        .hint("Add an account first. Each one can have its own GitHub login."),
                )
                .into_any_element()
        });
        section(
            "GitHub",
            "Pushes and pull requests from a folder use its account's own gh login, never another's.",
            rows.into_iter().chain(empty),
            cx,
        )
        .into_any_element()
    }
}
