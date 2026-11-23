//! The Settings tab: opening it, and what its events do (login terminals, saved accounts).

use super::*;

impl Workspace {
    pub(super) fn open_settings(
        &mut self,
        _: &OpenSettings,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(settings) = self.settings.as_ref().and_then(|s| s.upgrade()) {
            settings.update(cx, |s, cx| s.check_gh_logins(cx)); // a login may have finished since
            let id = settings.entity_id().into();
            self.dock
                .update(cx, |dock, cx| dock.select_panel(id, window, cx));
            settings.read(cx).focus_handle(cx).focus(window, cx);
            return;
        }
        self.reload_accounts();
        self.push_config(cx); // orchestration and open threads check against what's on disk now
        let (path, config, error) = (
            self.accounts_path.clone(),
            self.config.clone(),
            self.load_error.clone(),
        );
        let settings = cx.new(|cx| Settings::new(path, config, error, window, cx));
        self.dock.update(cx, |dock, cx| {
            dock.add_panel_view(
                panel_handle(settings.clone()),
                DockPlacement::Center,
                None,
                window,
                cx,
            );
        });
        settings.read(cx).focus_handle(cx).focus(window, cx);
        // Settings keeps its own copy and saves to disk, so re-read the file whenever it changes.
        self._settings_saved = Some(cx.observe(&settings, |this, _, cx| {
            this.reload_accounts();
            this.push_config(cx);
            cx.notify();
        }));
        self._settings_events = Some(cx.subscribe_in(
            &settings,
            window,
            |this, _, e: &SettingsEvent, window, cx| {
                let (opened, dir) = match e {
                    SettingsEvent::GhLogin { dir } => (gh_login_options(&gh_bin(cx), dir), dir),
                    SettingsEvent::CodexLogin { dir } => (
                        github::codex_login_options(&github::codex_bin(cx), dir),
                        dir,
                    ),
                };
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
        self.settings = Some(settings.downgrade());
        cx.notify();
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
