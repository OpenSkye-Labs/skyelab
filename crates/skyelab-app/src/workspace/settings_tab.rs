//! The Settings page: opening it, and what its events do (login terminals, saved accounts).

use super::*;

impl Workspace {
    pub(super) fn open_settings(
        &mut self,
        _: &OpenSettings,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let section = match &self.page {
            Page::Settings(section) => *section,
            _ => crate::settings::Section::Accounts,
        };
        if let Some(settings) = self.settings.clone() {
            // The page lives all session: show the file as it is now (an edit elsewhere, a login since).
            self.reload_accounts();
            self.push_config(cx);
            if self.load_error.is_none() {
                let config = self.config.clone();
                settings.update(cx, |s, cx| s.set_config(config, window, cx));
            }
        }
        self.show(Page::Settings(section), window, cx);
    }

    /// Settings → Usage, read from the store again each time it opens (T3's Usage entry).
    pub(super) fn open_usage(
        &mut self,
        _: &OpenUsage,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.show(Page::Settings(crate::settings::Section::Usage), window, cx);
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
