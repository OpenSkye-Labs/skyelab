//! When the Welcome page shows first-run setup, and the writes its buttons ask for.

use super::onboarding::{OnboardingEvent, Sync};
use super::*;
use anyhow::Context as _;

/// Whether setup shows: on its own while accounts or projects are missing (once projects have loaded), until
/// skipped; always after "Set up accounts" until closed.
#[derive(Default)]
pub(super) struct SetupFlow {
    pub loaded: bool,
    pub forced: bool,
    pub dismissed: bool,
    pub _events: Option<Subscription>,
}

impl Workspace {
    pub(super) fn watch_setup(&mut self, cx: &mut Context<Self>) {
        let setup = self.welcome.read(cx).setup.clone();
        self.setup._events = Some(cx.subscribe(&setup, |this, _, e, cx| this.setup_event(e, cx)));
    }

    /// Shows or hides setup for the workspace as it is now. Call after projects or accounts change.
    pub(super) fn sync_setup(&mut self, cx: &mut Context<Self>) {
        let missing = self.config.accounts.is_empty() || self.projects.is_empty();
        let s = &self.setup;
        let first = !s.dismissed && !onboarding::first_run::done(cx) && missing;
        let show = !self.is_remote() && s.loaded && (s.forced || first);
        // Once open it stays until Done or Skip, even after the last missing piece is added.
        self.setup.forced |= show;
        let sync = Sync {
            config: self.config.clone(),
            projects: self.projects.iter().map(|p| p.path.clone()).collect(),
            load_error: self.load_error.clone(),
            claude_bin: self.backend.claude_bin.clone(),
            codex_bin: github::codex_bin(cx),
            cursor_bin: github::cursor_bin(cx),
            antigravity_bin: github::antigravity_bin(cx),
            gh_bin: github::gh_bin(cx),
        };
        if show {
            self.refresh_logins(cx);
        }
        let appears = show && !self.welcome.read(cx).show_setup;
        self.welcome.update(cx, |w, cx| {
            w.setup.update(cx, |o, cx| o.sync(show, sync, cx));
            if w.show_setup != show {
                w.show_setup = show;
                cx.notify();
            }
        });
        if appears {
            // The terminal drawer cut the card off and hid Continue: setup gets the whole page.
            let (handle, this) = (self.window_handle, cx.entity().downgrade());
            cx.defer(move |cx| {
                let _ = handle.update(cx, |_, window, cx| {
                    let _ = this.update(cx, |this, cx| this.hide_docks_for_setup(window, cx));
                });
            });
        }
    }

    /// Hides the terminal drawer and the right panel, which would cover or squeeze onboarding.
    fn hide_docks_for_setup(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        for at in [DockPlacement::Bottom, DockPlacement::Right] {
            if self.dock.read(cx).is_dock_open(at) {
                self.toggle(at, window, cx);
            }
        }
    }

    pub(super) fn open_onboarding(
        &mut self,
        _: &OpenOnboarding,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.is_remote() {
            return; // setup reads and binds this machine's folders, not the one shown
        }
        self.setup.forced = true;
        self.setup.loaded = true; // asked for: no need to wait for the project list
        // Asked again while open: back to the first page.
        let setup = self.welcome.read(cx).setup.clone();
        setup.update(cx, |o, cx| o.go(onboarding::Step::Welcome, cx));
        self.reload_accounts();
        self.push_config(cx);
        self.sync_setup(cx);
        self.show(Page::Welcome, window, cx);
    }

    fn setup_event(&mut self, e: &OnboardingEvent, cx: &mut Context<Self>) {
        if self.is_remote() {
            return; // the page is hidden for another machine; never write this one's accounts from it
        }
        let result = match e {
            OnboardingEvent::Close => {
                (self.setup.forced, self.setup.dismissed) = (false, true);
                onboarding::first_run::set_done(cx);
                Ok(())
            }
            OnboardingEvent::AddAccounts(accounts) => self.edit_accounts(
                |c| accounts.iter().try_for_each(|a| c.add_account(a.clone())),
                cx,
            ),
            OnboardingEvent::Import(picks) => self
                .edit_accounts(
                    |c| {
                        picks.iter().try_for_each(|(dir, account)| match account {
                            Some(a) => skyelab_core::onboarding::bind_project(c, dir, a),
                            None => Ok(()),
                        })
                    },
                    cx,
                )
                .map(|()| {
                    self.add_projects(picks.iter().map(|(d, _)| d.clone()).collect(), cx);
                    let setup = self.welcome.read(cx).setup.clone();
                    setup.update(cx, |o, cx| o.imported(cx));
                }),
        };
        let setup = self.welcome.read(cx).setup.clone();
        setup.update(cx, |o, cx| {
            o.error = result.err().map(|e| format!("{e:#}"));
            cx.notify();
        });
        self.sync_setup(cx);
    }

    /// One edit of accounts.toml as it is on disk now, saved only if the whole edit succeeds. Refused while the file
    /// can't be read, so a broken file is never replaced.
    pub(super) fn edit_accounts(
        &mut self,
        edit: impl FnOnce(&mut AccountsConfig) -> anyhow::Result<()>,
        cx: &mut Context<Self>,
    ) -> anyhow::Result<()> {
        self.reload_accounts(); // the file may have been fixed since it failed to load
        if let Some(why) = &self.load_error {
            anyhow::bail!("accounts.toml could not be read, fix it first: {why}");
        }
        let path = self
            .accounts_path
            .clone()
            .context("no config directory on this OS, so nothing can be saved")?;
        let mut next = AccountsConfig::load(&path)?;
        edit(&mut next)?;
        next.save(&path)?;
        self.reload_accounts();
        self.push_config(cx);
        Ok(())
    }
}
