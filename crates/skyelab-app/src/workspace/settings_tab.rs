//! The Settings page: opening it, and what its events do (login terminals, saved accounts).

use super::*;

use gpui_component::WindowExt as _;

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
    /// One per window: each window's workspace has its own Settings view and dialog layer.
    pub(super) fn open_settings_at(
        &mut self,
        section: crate::settings::Section,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let settings = self.ensure_settings(window, cx);
        // The dialog's `on_close` runs for Esc and the backdrop, not for a programmatic close, so a live dialog is
        // what counts.
        let has_root = matches!(window.root::<gpui_component::Root>(), Some(Some(_)));
        self.settings_open &= !has_root || window.has_active_dialog(cx);
        if !self.settings_open {
            // The view lives all session: show the file as it is now (an edit elsewhere, a login since).
            self.reload_accounts();
            self.push_config(cx);
            if self.load_error.is_none() {
                let config = self.config.clone();
                settings.update(cx, |s, cx| s.set_config(config, window, cx));
            }
        }
        settings.update(cx, |s, cx| s.show(section, window, cx));
        if section == crate::settings::Section::Usage {
            self.reload_usage(cx);
        }
        if self.settings_open {
            settings.read(cx).focus_handle(cx).focus(window, cx);
            return;
        }
        self.settings_open = true;
        // A window without gpui-component's `Root` (some tests) has no dialog layer: only the flag is set, as
        // `manage::push_note` skips its toast.
        if !has_root {
            return cx.notify();
        }
        let ws = cx.entity().downgrade();
        let view = settings.clone();
        window.open_dialog(cx, move |dialog, window, _| {
            let size = window.viewport_size();
            let ws = ws.clone();
            // T3's modal: up to 1280 × 960 with 40 px of window around it, radius 16.
            dialog
                .w((size.width - gpui::px(80.)).min(gpui::px(1180.)))
                .h((size.height - gpui::px(80.)).min(gpui::px(900.)))
                .centered()
                .p_0()
                .rounded(crate::ui::RADIUS_XL)
                .overflow_hidden()
                .close_button(false)
                .on_close(move |_, _, cx| {
                    _ = ws.update(cx, |ws, cx| {
                        ws.settings_open = false;
                        cx.notify();
                    });
                })
                .child(view.clone())
        });
        settings.read(cx).focus_handle(cx).focus(window, cx);
        cx.notify();
    }

    /// Re-reads the Usage page's numbers; called whenever Settings → Usage is shown.
    pub(super) fn reload_usage(&mut self, cx: &mut Context<Self>) {
        let page = self
            .settings
            .as_ref()
            .and_then(|s| s.read(cx).usage.clone());
        if let Some(page) = page {
            let (store, config) = (self.backend.store.clone(), self.config.clone());
            page.update(cx, |p, cx| p.reload(store, &config, cx));
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
        let usage = cx.new(usage_page::UsagePage::new);
        let settings = cx.new(|cx| {
            let mut s = Settings::new(path, config, error, window, cx);
            s.remote = remote;
            s.usage = Some(usage);
            s
        });
        // Settings keeps its own copy and saves to disk, so re-read the file whenever it changes.
        self._settings_saved = Some(cx.observe(&settings, |this, _, cx| {
            this.reload_accounts();
            this.push_config(cx);
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
                    SettingsEvent::Closed => {
                        this.settings_open = false;
                        return cx.notify();
                    }
                    SettingsEvent::GhLogin { dir } => (gh_login_options(&gh_bin(cx), dir), dir),
                    SettingsEvent::CodexLogin { dir } => (
                        github::codex_login_options(&github::codex_bin(cx), dir),
                        dir,
                    ),
                };
                // The login terminal opens in the bottom dock, under the modal: close it so the terminal shows.
                if this.settings_open {
                    window.close_dialog(cx);
                    this.settings_open = false;
                }
                // The tab names the account being logged in to, not the folder it starts in.
                let account = this.config.accounts.iter().find(|a| {
                    a.gh_config_dir.as_deref() == Some(dir.as_path()) || a.config_dir == *dir
                });
                let account = account.cloned();
                let opened = opened.and_then(|o| this.add_terminal_with(o, account, window, cx));
                if let Err(e) = opened {
                    eprintln!("failed to open the login terminal: {e:#}");
                }
            },
        ));
        self.settings = Some(settings.clone());
        settings
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
