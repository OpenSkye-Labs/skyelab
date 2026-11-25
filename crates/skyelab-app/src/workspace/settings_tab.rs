//! The Settings modal: opening and closing it, and what its events do (login terminals, saved accounts).

use super::*;
use gpui::AnimationExt as _;

impl Workspace {
    /// ⌘, and every other "open settings" path: the modal, on the section it showed last.
    pub(super) fn open_settings(
        &mut self,
        _: &OpenSettings,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let section = self.ensure_settings(window, cx).read(cx).section;
        self.open_settings_at(section, window, cx);
    }

    /// Settings → Usage, read from the store again each time it opens (T3's Usage entry).
    pub(super) fn open_usage(
        &mut self,
        _: &OpenUsage,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_settings_at(crate::settings::Section::Usage, window, cx);
    }

    /// Shows Settings as a modal over this window (T3's `AppRouteModal`) on `section`, or moves an open one there.
    /// One per window: each window's workspace has its own Settings view, drawn by [`Self::settings_modal`].
    pub(super) fn open_settings_at(
        &mut self,
        section: crate::settings::Section,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.close_env_menu(window, cx);
        let settings = self.ensure_settings(window, cx);
        if !self.settings_open {
            // The view lives all session: show the file as it is now (an edit elsewhere, a login since).
            self.reload_accounts();
            self.push_config(cx);
            if self.load_error.is_none() {
                let config = self.config.clone();
                settings.update(cx, |s, cx| s.set_config(config, window, cx));
            }
            self.refresh_logins(cx);
            let back = window.focused(cx);
            // Local projects only: a remote's folders are another machine's, and its menu can't open this page.
            let projects = if self.is_remote() {
                Vec::new()
            } else {
                self.projects
                    .iter()
                    .map(|p| (p.name.clone().into(), p.path.clone()))
                    .collect()
            };
            settings.update(cx, |s, cx| {
                s.return_focus = back;
                s.projects = projects;
                s.set_scope(None, window, cx);
            });
            self.settings_open = true;
        }
        settings.update(cx, |s, cx| s.show(section, window, cx));
        if section == crate::settings::Section::Usage {
            self.reload_usage(cx);
            self.poll_usage(cx);
        }
        if section == crate::settings::Section::Scheduled
            && let Some(page) = settings.read(cx).scheduled.clone()
        {
            page.update(cx, |p, cx| p.reload(cx));
        }
        settings.read(cx).focus_handle(cx).focus(window, cx);
        cx.notify();
    }

    /// Closes the Settings modal (Esc, the backdrop, ×, or moving to another page) and gives the focus back.
    pub(crate) fn close_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.settings_open {
            return;
        }
        self.settings_open = false;
        let Some(settings) = self.settings.clone() else {
            return cx.notify();
        };
        let back = settings.update(cx, |s, _| s.return_focus.take());
        // Closed because something else took the focus (the palette, F6): that keeps it.
        let inside = settings
            .read(cx)
            .focus_handle(cx)
            .contains_focused(window, cx);
        if inside || window.focused(cx).is_none() {
            match back {
                Some(f) => f.focus(window, cx),
                None => self.focus.focus(window, cx),
            }
        }
        cx.notify();
    }

    /// The modal: the dimmed backdrop over the whole window, and Settings centered on it, up to 1180 × 900 with 40 px
    /// of window around it, radius 16 (T3's is up to 1280 × 960). Drawn inside the workspace's tree, not as a gpui-component `Dialog`, so
    /// actions a section dispatches (Sharing's Allow, a scheduled task's thread link) still reach the workspace.
    pub(super) fn settings_modal(
        &self,
        window: &Window,
        cx: &Context<Self>,
    ) -> Option<impl IntoElement + use<>> {
        let settings = self.settings.clone().filter(|_| self.settings_open)?;
        let size = window.viewport_size();
        let theme = cx.theme();
        let dark = theme.is_dark();
        let border = if dark { theme.input } else { theme.border };
        Some(
            gpui::deferred(
                div()
                    .absolute()
                    .inset_0()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        div()
                            .id("settings-backdrop")
                            .debug_selector(|| "settings-backdrop".into())
                            .absolute()
                            .inset_0()
                            .bg(theme.overlay)
                            .occlude()
                            .on_mouse_down(
                                gpui::MouseButton::Left,
                                cx.listener(|this, _, window, cx| this.close_settings(window, cx)),
                            )
                            .with_animation(
                                "settings-backdrop-in",
                                gpui::Animation::new(crate::ui::motion::FADE_FAST)
                                    .with_easing(crate::ui::motion::ease_out_quad),
                                |el, t| el.opacity(t),
                            ),
                    )
                    .child(
                        div()
                            .w((size.width - px(80.)).min(px(1180.)))
                            .h((size.height - px(80.)).min(px(900.)))
                            .rounded(crate::ui::RADIUS_XL)
                            .border_1()
                            .border_color(border)
                            .overflow_hidden()
                            .occlude()
                            .child(settings)
                            .with_animation(
                                "settings-modal-in",
                                gpui::Animation::new(crate::ui::motion::FADE)
                                    .with_easing(crate::ui::motion::ease_out_quad),
                                move |el, t| {
                                    el.opacity(t)
                                        .shadow(crate::ui::motion::overlay_shadow(t * t * t, dark))
                                },
                            ),
                    ),
            )
            .with_priority(2),
        )
    }

    /// Re-reads the Usage page's numbers; called whenever Settings → Usage is shown.
    pub(super) fn reload_usage(&mut self, cx: &mut Context<Self>) {
        let page = self
            .settings
            .as_ref()
            .and_then(|s| s.read(cx).usage.clone());
        if let Some(page) = page {
            let (store, config) = (self.backend.store.clone(), self.config.clone());
            let errors = self.usage_poll.errors();
            page.update(cx, |p, cx| p.reload(store, &config, errors, cx));
        }
    }

    /// The Settings view, created on first use with the accounts as they are on disk now.
    pub(super) fn ensure_settings(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<Settings> {
        if let Some(settings) = &self.settings {
            return settings.clone();
        }
        self.reload_accounts();
        self.push_config(cx); // orchestration and open threads check against what's on disk now
        let (path, config, error) = (
            self.accounts_path.clone(),
            self.config.clone(),
            self.load_error.clone(),
        );
        let remote = self.remote_panel(cx);
        let sharing = self.sharing();
        let machines = self
            .env_picker()
            .map(|p| cx.new(|cx| machines_page::MachinesPage::new(p, sharing, cx)));
        let usage = cx.new(usage_page::UsagePage::new);
        let (store, accounts) = (self.backend.store.clone(), self.accounts_path.clone());
        let rules = cx.new(|cx| super::ProjectRulesPage::new(store.clone(), cx));
        let scheduled = cx.new(|cx| scheduled::ScheduledPage::new(store, accounts, cx));
        let settings = cx.new(|cx| {
            let mut s = Settings::new(path, config, error, window, cx);
            s.remote = remote;
            s.machines = machines;
            s.usage = Some(usage);
            s.scheduled = Some(scheduled);
            s.rules = Some(rules);
            s
        });
        // Settings keeps its own copy and saves to disk, so re-read the file whenever it changes.
        self._settings_saved = Some(cx.observe(&settings, |this, _, cx| {
            this.reload_accounts();
            this.push_config(cx);
            this.refresh_logins(cx); // an account added or its folder changed; cached answers aren't asked again
            this.poll_usage(cx); // Usage picked in the nav
            cx.notify();
            // Every window binds folders from the same file.
            this.each_other_window(cx, |ws, cx| {
                ws.reload_accounts();
                ws.push_config(cx);
                cx.notify();
            });
        }));
        self._settings_events = Some(cx.subscribe_in(
            &settings,
            window,
            |this, _, e: &SettingsEvent, window, cx| {
                let (opened, dir) = match e {
                    SettingsEvent::OpenKeysFile { path } => {
                        return this.open_keys_file(path, window, cx);
                    }
                    SettingsEvent::Closed => return this.close_settings(window, cx),
                    SettingsEvent::GhLogin { dir } => (gh_login_options(&gh_bin(cx), dir), dir),
                    SettingsEvent::AgentLogin { id } => {
                        return this.log_in_account(&LogInAccount { id: id.clone() }, window, cx);
                    }
                };
                // The login terminal opens in the bottom dock, under the modal: close it so the terminal shows.
                this.close_settings(window, cx);
                // The tab names the account being logged in to, not the folder it starts in.
                let account = this
                    .config
                    .accounts
                    .iter()
                    .find(|a| a.gh_config_dir.as_deref() == Some(dir.as_path()));
                let account = account.cloned();
                let opened = opened.and_then(|o| this.add_login_terminal(o, account, window, cx));
                if let Err(e) = opened {
                    eprintln!("failed to open the login terminal: {e:#}");
                }
            },
        ));
        self.settings = Some(settings.clone());
        settings
    }

    /// Opens a terminal tab that logs account `a.id` in to its agent, under that account's own config dir.
    pub(super) fn log_in_account(
        &mut self,
        a: &LogInAccount,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(account) = self.config.accounts.iter().find(|x| x.id == a.id).cloned() else {
            eprintln!("no account '{}' to log in", a.id);
            return;
        };
        let bin = match account.provider {
            skyelab_core::Provider::Claude => self.backend.claude_bin.clone(),
            skyelab_core::Provider::Codex => github::codex_bin(cx),
        };
        let options =
            github::agent_login_options(&bin, github::login_args(account.provider), &account);
        // The login terminal opens in the bottom dock, under the modal: close it so the terminal shows.
        self.close_settings(window, cx);
        if let Err(e) = options.and_then(|o| self.add_login_terminal(o, Some(account), window, cx))
        {
            eprintln!("failed to open the login terminal: {e:#}");
        }
    }

    pub(super) fn reload_accounts(&mut self) {
        let Some(path) = &self.accounts_path else {
            return;
        };
        match AccountsConfig::load(path) {
            Ok(config) => {
                (self.config, self.load_error) = (config, None);
                self.account = account_label(&self.config, &self.folder);
                self.account_id = self.config.resolve(&self.folder).map(|a| a.id.clone());
            }
            Err(e) => {
                self.load_error = Some(format!("{e:#}"));
                // A broken file binds nothing we can trust: terminals and threads then ask, too.
                self.config = AccountsConfig::default();
                self.account = None;
                self.account_id = None;
            }
        }
    }
}
