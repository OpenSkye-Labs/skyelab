//! Whether each account is signed in to its agent CLI, shared by Settings → Accounts and the setup page. Checked off
//! the UI thread with the account's launch env only (`skyelab_agent::login_status`), cached, and checked again
//! after a Log in terminal ends.

use gpui::{App, AppContext as _, Context, Entity, Global, Task};
use skyelab_agent::Login;
use skyelab_core::{Account, Provider};
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

/// A cached answer this young is shown without asking again (Settings opening, the setup page syncing).
const FRESH: Duration = Duration::from_secs(60);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Status {
    Checking,
    SignedIn(Option<String>),
    SignedOut,
    /// The CLI couldn't answer (not installed, an old version, a broken config).
    Unknown(String),
}

impl Status {
    pub fn signed_out(status: Option<&Self>) -> bool {
        status == Some(&Self::SignedOut)
    }

    /// What an account card says: the words, their color, and the CLI's error (a tooltip) when it couldn't answer.
    pub fn label(
        status: Option<&Self>,
        theme: &gpui_component::Theme,
    ) -> (gpui::SharedString, gpui::Hsla, Option<gpui::SharedString>) {
        let ink = |c| crate::ui::ink(theme, c);
        match status {
            Some(Self::SignedIn(Some(who))) => (
                format!("Signed in as {who}").into(),
                ink(theme.success),
                None,
            ),
            Some(Self::SignedIn(None)) => ("Signed in".into(), ink(theme.success), None),
            Some(Self::SignedOut) => ("Not signed in".into(), ink(theme.warning), None),
            Some(Self::Unknown(e)) => (
                "Sign-in unknown".into(),
                theme.muted_foreground,
                Some(e.clone().into()),
            ),
            Some(Self::Checking) | None => {
                ("Checking sign-in…".into(), theme.muted_foreground, None)
            }
        }
    }
}

/// One login: a renamed account keeps its status, a moved config dir doesn't.
type Key = (String, Provider, PathBuf);

fn key(a: &Account) -> Key {
    (a.id.clone(), a.provider, a.config_dir.clone())
}

/// The agent CLIs to ask: the workspace's (fakes in debug builds and tests).
#[derive(Clone)]
pub struct Bins {
    pub claude: PathBuf,
    pub codex: PathBuf,
    pub cursor: PathBuf,
}

#[derive(Default)]
pub struct LoginStatus {
    statuses: HashMap<Key, (Status, Instant)>,
    tasks: HashMap<Key, Task<()>>,
}

struct Shared(Entity<LoginStatus>);

impl Global for Shared {}

impl LoginStatus {
    /// The app's one cache, made on first use.
    pub fn shared(cx: &mut App) -> Entity<Self> {
        if let Some(s) = cx.try_global::<Shared>() {
            return s.0.clone();
        }
        let entity = cx.new(|_| Self::default());
        cx.set_global(Shared(entity.clone()));
        entity
    }

    pub fn get(&self, a: &Account) -> Option<&Status> {
        self.statuses.get(&key(a)).map(|(s, _)| s)
    }

    /// Checks every account whose answer is missing or older than [`FRESH`].
    pub fn refresh(&mut self, accounts: &[Account], bins: &Bins, cx: &mut Context<Self>) {
        for a in accounts {
            let fresh = self
                .statuses
                .get(&key(a))
                .is_some_and(|(s, at)| *s != Status::Checking && at.elapsed() < FRESH);
            if !fresh && !self.tasks.contains_key(&key(a)) {
                self.check(a, bins, cx);
            }
        }
    }

    /// Asks `a`'s CLI now (after a Log in), keeping the last answer on screen until the new one lands.
    pub fn check(&mut self, a: &Account, bins: &Bins, cx: &mut Context<Self>) {
        let k = key(a);
        self.statuses
            .entry(k.clone())
            .or_insert((Status::Checking, Instant::now()));
        let bin = match a.provider {
            Provider::Claude => bins.claude.clone(),
            Provider::Codex => bins.codex.clone(),
            Provider::Cursor => bins.cursor.clone(),
        };
        let account = a.clone();
        let asked = cx.background_spawn(async move {
            match skyelab_agent::login_status(&bin, &account) {
                Ok(Login::SignedIn(who)) => Status::SignedIn(who),
                Ok(Login::SignedOut) => Status::SignedOut,
                Err(e) => Status::Unknown(format!("{e:#}")),
            }
        });
        let task_key = k.clone();
        let task = cx.spawn(async move |this, cx| {
            let status = asked.await;
            let _ = this.update(cx, |this, cx| {
                this.tasks.remove(&task_key);
                let changed = this.statuses.get(&task_key).map(|(s, _)| s) != Some(&status);
                this.statuses.insert(task_key, (status, Instant::now()));
                if changed {
                    cx.notify();
                }
            });
        });
        self.tasks.insert(k, task);
        cx.notify();
    }
}

impl super::Workspace {
    pub(super) fn login_bins(&self, cx: &App) -> Bins {
        let codex = super::github::codex_bin(cx);
        let cursor = super::github::cursor_bin(cx);
        // A test that set no fake Codex or Cursor must never run the real CLI.
        #[cfg(test)]
        let codex = match cx.has_global::<super::github::CodexBin>() {
            true => codex,
            false => PathBuf::from("/nonexistent/codex"),
        };
        #[cfg(test)]
        let cursor = match cx.has_global::<super::github::CursorBin>() {
            true => cursor,
            false => PathBuf::from("/nonexistent/cursor-agent"),
        };
        Bins {
            claude: self.backend.claude_bin.clone(),
            codex,
            cursor,
        }
    }

    /// Asks each account's CLI whether it is signed in, unless a recent answer is cached.
    pub(super) fn refresh_logins(&self, cx: &mut Context<Self>) {
        let (accounts, bins) = (self.config.accounts.clone(), self.login_bins(cx));
        LoginStatus::shared(cx).update(cx, |l, cx| l.refresh(&accounts, &bins, cx));
    }

    /// Asks again once the Log in terminal `tab` for `account` ends (or is closed): its agent sign-in, and Settings'
    /// GitHub logins (the tab may have been a `gh auth login`).
    pub(super) fn recheck_after_login(
        &self,
        tab: &Entity<super::terminal_panel::TerminalPanel>,
        account: Account,
        cx: &mut Context<Self>,
    ) {
        let bins = self.login_bins(cx);
        let tab = tab.downgrade();
        cx.spawn(async move |this, cx| {
            until_exited(&tab, cx).await;
            cx.update(|cx| {
                LoginStatus::shared(cx).update(cx, |l, cx| l.check(&account, &bins, cx));
            });
            let _ = this.update(cx, |ws, cx| {
                if let Some(settings) = &ws.settings {
                    settings.update(cx, |s, cx| s.check_gh_logins(cx));
                }
            });
        })
        .detach();
    }
}

/// Waits (polling each second) until every pane of `tab` has exited, or the tab is gone; `Some(true)` when every
/// pane exited with success.
pub(super) async fn until_exited(
    tab: &gpui::WeakEntity<super::terminal_panel::TerminalPanel>,
    cx: &mut gpui::AsyncApp,
) -> Option<bool> {
    loop {
        cx.background_executor().timer(Duration::from_secs(1)).await;
        let exits = tab
            .read_with(cx, |t, cx| {
                t.panes
                    .iter()
                    .map(|p| p.view.read(cx).terminal().exit_status())
                    .collect::<Option<Vec<_>>>()
            })
            .ok()?;
        if let Some(exits) = exits {
            return Some(exits.iter().all(|e| e.success()));
        }
    }
}
