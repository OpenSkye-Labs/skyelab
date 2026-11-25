//! Sign in to one MCP server from the details card: its agent CLI's own login (`claude mcp login`, `codex mcp login`)
//! runs in a terminal in the thread's folder, under the account that folder is bound to, then the workspace offers to
//! restart the agent so the server reconnects.

use super::thread::RestartAgentSession;
use super::{ThreadView, Workspace, github, worktree};
use crate::ui::toast::Toast;
use anyhow::Context as _;
use gpui::{AppContext as _, Context, Entity, Window};

impl Workspace {
    pub(super) fn mcp_sign_in(
        &mut self,
        view: Entity<ThreadView>,
        server: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let v = view.read(cx);
        let (cwd, account_id) = (
            v.cwd.clone(),
            v.thread.as_ref().map(|t| t.account_id.clone()),
        );
        let failed = |view: &Entity<ThreadView>, e: anyhow::Error, cx: &mut gpui::App| {
            view.update(cx, |v, cx| {
                v.error = Some(format!("{:#}", e.context("signing in to the MCP server")).into());
                cx.notify();
            })
        };
        if self.is_remote() {
            let e = anyhow::anyhow!("sign in on the machine the thread runs on");
            return failed(&view, e, cx);
        }
        let Some(account_id) = account_id else {
            return failed(&view, anyhow::anyhow!("the thread has no account yet"), cx);
        };
        let (config, claude, codex) = (
            self.accounts_job(cx),
            self.backend.claude_bin.clone(),
            github::codex_bin(cx),
        );
        let name = server.clone();
        // The folder's binding, read fresh, must still be the thread's account: never sign in under another one.
        let job = cx.background_spawn(async move {
            let config = config();
            worktree::resolves_to(&config, &cwd, &account_id)?;
            let account = (config.accounts.iter().find(|a| a.id == account_id).cloned())
                .with_context(|| format!("no account {account_id}"))?;
            let bin = match account.provider {
                skyelab_core::Provider::Claude => claude,
                skyelab_core::Provider::Codex => codex,
                skyelab_core::Provider::Antigravity => {
                    anyhow::bail!("MCP sign-in is not supported for Antigravity accounts")
                }
            };
            let mut options =
                github::agent_login_options(&bin, &github::mcp_login_args(&name), &account)?;
            // Project and local servers are found from the folder, as the agent found them.
            options.cwd = cwd;
            anyhow::Ok((options, account))
        });
        cx.spawn_in(window, async move |this, cx| {
            let job = job.await;
            let _ = this.update_in(cx, |this, window, cx| {
                let tab = job.and_then(|(o, a)| this.add_login_terminal(o, Some(a), window, cx));
                match tab {
                    Ok(tab) => {
                        tab.update(cx, |t, cx| {
                            t.name = Some(format!("Sign in: {server}").into());
                            cx.notify();
                        });
                        this.offer_restart(tab, view, server, window, cx);
                    }
                    Err(e) => failed(&view, e, cx),
                }
            });
        })
        .detach();
    }

    /// Once the sign-in terminal exits with success, a notice offers to restart the agent: a running one loaded its
    /// MCP servers at start, so the new sign-in reaches it only in a new process.
    fn offer_restart(
        &mut self,
        tab: Entity<super::terminal_panel::TerminalPanel>,
        view: Entity<ThreadView>,
        server: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let tab = tab.downgrade();
        cx.spawn_in(window, async move |this, cx| {
            if super::login_status::until_exited(&tab, cx).await != Some(true) {
                return; // a failed login shows its error in the terminal
            }
            let _ = this.update_in(cx, |_ws, window, cx| {
                let text =
                    format!("Signed in to {server}. Restart the agent session so it reconnects.");
                #[cfg(test)]
                _ws.notices.push(text.clone().into());
                let view = view.downgrade();
                let note = Toast::new(text).action("Restart", move |window, cx| {
                    let _ = view.update(cx, |v, cx| {
                        v.restart_session(&RestartAgentSession, window, cx)
                    });
                });
                crate::ui::toast::push(window, note, cx);
            });
        })
        .detach();
    }
}
