//! The Changes tab's Commit / Push / Open PR. Each runs only while the folder still resolves to the
//! thread's recorded account; Push and Open PR also need that account's GitHub login.

use super::changes::ChangesView;
use super::changes_generate::Gen;
use super::thread::GitAction;
use super::worktree::push_account;
use super::{SharedStore, lock};
use crate::ui;
use anyhow::{Context as _, Result, bail};
use gpui::Focusable as _;
use gpui::{AnyElement, App, Context, Window, div, prelude::*};
use gpui_component::{
    ActiveTheme as _, Disableable as _, Sizable as _,
    button::{Button, ButtonVariants as _},
    h_flex,
    input::{Input, Textarea},
    v_flex,
};
use skyelab_core::AccountsConfig;
use skyelab_git::GitHub;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

/// What an action runs against, read from the thread at click time.
pub(super) struct Target {
    pub(super) dir: PathBuf,
    pub(super) config: AccountsConfig,
    pub(super) account_id: String,
    pub(super) thread_id: i64,
    pub(super) store: SharedStore,
    /// The account's `GH_CONFIG_DIR`, when the action needs GitHub.
    gh_dir: Option<PathBuf>,
}

impl ChangesView {
    pub(super) fn target(&self, cx: &App) -> Result<Target> {
        let view = self
            .thread
            .as_ref()
            .and_then(|t| t.upgrade())
            .context("the thread's tab is closed")?;
        let view = view.read(cx);
        let row = view
            .thread
            .as_ref()
            .context("send a prompt first: the thread has no account yet")?;
        if row.cwd != self.dir {
            bail!(
                "this tab shows {}, but the thread runs in {}; open Changes from the thread again",
                self.dir.display(),
                row.cwd.display()
            );
        }
        // The account checks run when the action does, against accounts.toml as saved then (`run`).
        Ok(Target {
            dir: self.dir.clone(),
            config: view.config.clone(),
            account_id: row.account_id.clone(),
            thread_id: row.id,
            store: view.backend.store.clone(),
            gh_dir: None,
        })
    }

    /// Checks the account, runs `work` off the UI thread, then `done` with its note (or shows the error).
    fn run(
        &mut self,
        github: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
        work: impl FnOnce(&Target, Option<GitHub>) -> Result<String> + Send + 'static,
        done: impl FnOnce(&mut Self, String, &mut Window, &mut Context<Self>) + 'static,
    ) {
        if self.busy {
            return;
        }
        self.publish_note = None;
        let target = match self.target(cx) {
            Ok(t) => t,
            Err(e) => {
                self.publish_error = Some(format!("{e:#}").into());
                cx.notify();
                return;
            }
        };
        let gh_bin = super::gh_bin(cx);
        let accounts_file = cx.try_global::<super::AccountsFile>().map(|f| f.0.clone());
        self.busy = true;
        self.publish_error = None;
        cx.notify();
        let task = cx.background_spawn(async move {
            let mut target = target;
            // The binding as saved right now, like an agent launch: an outside edit may have moved the folder.
            target.config = super::accounts_now(accounts_file.as_deref(), &target.config);
            push_account(&target.config, &target.dir, &target.account_id)?;
            if github {
                target.gh_dir = Some(target.config.github_config_dir(&target.dir)?);
            }
            let gh = target.gh_dir.as_deref().map(|config_dir| GitHub {
                gh: &gh_bin,
                config_dir,
            });
            work(&target, gh)
        });
        self._publish = Some(cx.spawn_in(window, async move |this, cx| {
            let result = task.await;
            let _ = this.update_in(cx, |this, window, cx| {
                this.busy = false;
                match result {
                    Ok(note) => done(this, note, window, cx),
                    Err(e) => this.publish_error = Some(format!("{e:#}").into()),
                }
                this.refresh(cx);
                cx.notify();
            });
        }));
    }

    /// An action from the Details tab's Commit & push menu: the same buttons, with their refusals. Commit and Open
    /// PR need the text this tab holds, so with it empty they point at the field instead.
    pub(super) fn run_git(
        &mut self,
        action: GitAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (text, focus, empty) = match action {
            GitAction::Push => return self.push(window, cx),
            GitAction::Commit | GitAction::CommitPush => {
                let f = self.commit_message.read(cx);
                (
                    f.value(),
                    f.focus_handle(cx),
                    "Write a commit message, then commit.",
                )
            }
            GitAction::OpenPr => {
                let f = self.pr_title.read(cx);
                (
                    f.value(),
                    f.focus_handle(cx),
                    "Write a PR title, then open it.",
                )
            }
        };
        if text.trim().is_empty() {
            self.publish_note = None;
            self.publish_error = Some(empty.into());
            focus.focus(window, cx);
            cx.notify();
            return;
        }
        match action {
            GitAction::OpenPr => self.open_pr(window, cx),
            a => self.commit_then(a == GitAction::CommitPush, window, cx),
        }
    }

    pub(super) fn commit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.commit_then(false, window, cx)
    }

    /// Commits, then pushes when `push` and the commit worked.
    fn commit_then(&mut self, push: bool, window: &mut Window, cx: &mut Context<Self>) {
        let message = self.commit_message.read(cx).value().to_string();
        let (nothing, tree) = self
            .changes
            .as_ref()
            .map(|c| (c.files.is_empty(), c.tree.clone()))
            .unwrap_or((true, None));
        let paths = self.picked_paths();
        self.run(
            false,
            window,
            cx,
            move |t, _| {
                if nothing {
                    bail!("nothing to commit");
                }
                let tree = tree.context(
                    "the files were changing while the Changes tab loaded; refresh it, then commit",
                )?;
                let env = hook_env(&t.config, &t.dir)?;
                let sha = skyelab_git::commit(&t.dir, &message, &paths, &env, Some(&tree))?;
                Ok(format!("Committed {}", sha.get(..8).unwrap_or(&sha)))
            },
            move |this, note, window, cx| {
                this.publish_note = Some(note.into());
                this.selected.clear();
                this.commit_message
                    .update(cx, |s, cx| s.set_value("", window, cx));
                if push {
                    this.push(window, cx);
                }
            },
        );
    }

    /// The ticked files' paths. A ticked rename takes its old path along, so the deletion lands in the same commit.
    /// Nothing ticked is everything, which `commit` only does while the tree is exactly the one shown.
    pub(super) fn picked_paths(&self) -> Vec<PathBuf> {
        let files = self
            .changes
            .as_ref()
            .map(|c| c.files.as_slice())
            .unwrap_or_default();
        files
            .iter()
            .filter(|f| self.selected.contains(&f.path))
            .flat_map(|f| std::iter::once(f.path.clone()).chain(f.old_path.clone()))
            .collect()
    }

    pub(super) fn push(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.run(
            true,
            window,
            cx,
            |t, gh| {
                let env = hook_env(&t.config, &t.dir)?;
                let to = skyelab_git::push_with(&t.dir, &gh.context("no GitHub login")?, &env)?;
                Ok(format!("Pushed to {to}"))
            },
            |this, note, _, _| this.publish_note = Some(note.into()),
        );
    }

    pub(super) fn open_pr(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let title = self.pr_title.read(cx).value().to_string();
        let body = self.pr_body.read(cx).value().to_string();
        self.run(
            true,
            window,
            cx,
            move |t, gh| {
                let url =
                    skyelab_git::create_pr(&t.dir, &gh.context("no GitHub login")?, &title, &body)?;
                let saved =
                    lock(&t.store).and_then(|s| s.set_thread_pr_url(t.thread_id, Some(&url)));
                saved.map(|()| url.clone()).map_err(|e| {
                    e.context(format!(
                        "opened {url}, but saving the link on the thread failed"
                    ))
                })
            },
            |this, url, _, cx| {
                this.publish_note = Some(format!("Opened {url}").into());
                if let Some(view) = this.thread.as_ref().and_then(|t| t.upgrade()) {
                    view.update(cx, |t, cx| t.set_pr_url(Some(url), cx));
                }
            },
        );
    }

    pub(super) fn render_publish(&self, cx: &mut Context<Self>) -> AnyElement {
        let total = self.changes.as_ref().map_or(0, |c| c.files.len());
        let commit_label = commit_label(self.selected.len(), total);
        let button = |id: &'static str, label: String| {
            Button::new(id)
                .small()
                .label(label)
                .flex_shrink_0()
                .disabled(self.busy)
        };
        // Push and Open PR run under the thread's account (re-checked when they run); say which, up front.
        let account = self
            .thread
            .as_ref()
            .and_then(|t| t.upgrade())
            .map(|t| t.read(cx).account_badge().chip());
        let theme = cx.theme();
        v_flex()
            .px_4()
            .py_3()
            .gap_2()
            .border_b_1()
            .border_color(theme.border)
            .children(account.map(|badge| {
                h_flex()
                    .gap_2()
                    .text_size(ui::META)
                    .text_color(theme.muted_foreground)
                    .child("Pushes and opens PRs as")
                    .child(badge)
            }))
            .child(
                h_flex()
                    .items_start()
                    .gap_2()
                    .child(
                        div()
                            .flex_1()
                            .child(Textarea::new(&self.commit_message).small()),
                    )
                    .children(self.generate_button(Gen::Commit, cx))
                    .child(
                        button("commit", commit_label)
                            .primary()
                            .on_click(cx.listener(|this, _, window, cx| this.commit(window, cx))),
                    )
                    .child(
                        button("push", "Push".into())
                            .outline()
                            .on_click(cx.listener(|this, _, window, cx| this.push(window, cx))),
                    ),
            )
            .child(
                h_flex()
                    .gap_2()
                    .child(div().flex_1().child(Input::new(&self.pr_title).small()))
                    .children(self.generate_button(Gen::Pr, cx))
                    .child(
                        button("open-pr", "Open PR".into())
                            .outline()
                            .on_click(cx.listener(|this, _, window, cx| this.open_pr(window, cx))),
                    ),
            )
            .child(Textarea::new(&self.pr_body).small())
            .children(self.publish_error.clone().map(|e| {
                div()
                    .id("publish-error")
                    .text_xs()
                    .text_color(crate::ui::ink(theme, theme.danger))
                    .child(e)
            }))
            .children(self.publish_note.clone().map(|n| {
                div()
                    .id("publish-note")
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(n)
            }))
            .into_any_element()
    }
}

/// What the Commit button will commit: the ticked files, or every listed one.
pub(super) fn commit_label(selected: usize, total: usize) -> String {
    let files = |n: usize| {
        if n == 1 {
            "1 file".to_string()
        } else {
            format!("{n} files")
        }
    };
    match selected {
        0 => format!("Commit all {}", files(total)),
        n => format!("Commit {}", files(n)),
    }
}

/// The env the repo's hooks run with on Commit and Push: the bound account's launch env plus the git credential
/// binding its agents get (only its gh answers https, ssh only if the account allows it), and no prompts.
fn hook_env(config: &AccountsConfig, dir: &Path) -> Result<Vec<(OsString, OsString)>> {
    let account = config
        .resolve(dir)
        .with_context(|| format!("{} isn't bound to an account", dir.display()))?;
    let mut env: Vec<(OsString, OsString)> = account
        .launch_env()?
        .into_iter()
        .map(|(k, v)| (k.into(), v.into()))
        .collect();
    let git = skyelab_core::env::account_git_env(account, dir);
    env.extend(git.into_iter().map(|(k, v)| (k.into(), v.into())));
    // ssh commit signing needs the agent; only for accounts that let their agents use it too, since the repo's
    // hooks (agent-writable) see the same env.
    if account.ssh_agent
        && let Some(sock) = std::env::var_os("SSH_AUTH_SOCK")
    {
        env.push(("SSH_AUTH_SOCK".into(), sock));
    }
    Ok(env)
}
