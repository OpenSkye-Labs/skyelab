//! The composer's `#` menu: this project's threads and their linked pull requests, picked as context chips. Only a
//! thread that ran on the composer's own account, in a folder still bound to it, is ever offered or read: another
//! account's thread stays out of this account's prompt.

use super::composer_context::ChipKind;
use super::composer_menu::{ContextPick, MenuItem};
use super::thread::ThreadView;
use gpui::{AppContext as _, Context, SharedString};
use skyelab_core::{AccountsConfig, terminal_context};
use skyelab_store::Thread;
use std::collections::HashMap;
use std::path::Path;

/// The folder whose binding decides `t`'s account: the project's for a worktree thread (its checkout sits outside
/// any binding), else the folder it ran in.
fn bound_folder<'a>(t: &'a Thread, project_path: &'a Path) -> &'a Path {
    match t.worktree_path {
        Some(_) => project_path,
        None => t.cwd.as_path(),
    }
}

/// Whether a chip in a prompt for `account` may carry `t`: it ran on `account`, and its folder is bound to `account`
/// now.
pub(crate) fn may_reference(
    t: &Thread,
    account: &str,
    config: &AccountsConfig,
    project_path: &Path,
) -> bool {
    t.account_id == account
        && config
            .resolve(bound_folder(t, project_path))
            .is_some_and(|a| a.id == account)
}

/// "PR #7" from a GitHub pull request link, else the link itself.
fn pr_label(url: &str) -> String {
    let n: String = url
        .rsplit_once("/pull/")
        .map(|(_, n)| n.chars().take_while(char::is_ascii_digit).collect())
        .unwrap_or_default();
    if n.is_empty() {
        url.to_owned()
    } else {
        format!("PR #{n}")
    }
}

/// What a `#` thread row shows beside its title so same-titled threads differ, like the sidebar row: how long
/// ago it was updated, its worktree branch and its last reply's first words. Project and account are the same for
/// every row (only this project's threads of this account are offered), so they aren't repeated.
fn thread_detail(t: &Thread, reply: Option<&String>, now_ms: i64) -> String {
    let mut parts = vec![crate::ui::ago(now_ms, t.updated_at)];
    parts.extend(t.worktree_branch.clone());
    parts.extend(
        reply
            .map(|r| skyelab_agent::thread_list::preview_text(r))
            .filter(|p| !p.is_empty()),
    );
    parts.join(" · ")
}

/// The `#` rows: each thread `may_reference` allows (but `current`), most recently updated first, and after it the
/// pull request it links, if any. `replies` holds each thread's last reply (`Store::last_replies`).
pub(crate) fn context_items(
    threads: &[Thread],
    current: Option<i64>,
    account: &str,
    config: &AccountsConfig,
    project_path: &Path,
    replies: &HashMap<i64, String>,
    now_ms: i64,
) -> Vec<MenuItem> {
    let mut out = Vec::new();
    for t in threads
        .iter()
        .filter(|t| Some(t.id) != current && may_reference(t, account, config, project_path))
    {
        out.push(MenuItem {
            label: t.title.clone(),
            detail: thread_detail(t, replies.get(&t.id), now_ms),
            context: Some(ContextPick::Thread(t.id)),
            ..Default::default()
        });
        if let Some(url) = &t.pr_url {
            out.push(MenuItem {
                label: format!("{} · {}", pr_label(url), t.title),
                detail: "Pull request".into(),
                context: Some(ContextPick::Pr(t.id)),
                ..Default::default()
            });
        }
    }
    out
}

impl ThreadView {
    /// The `#` rows for this composer, read off the UI thread. Unbound: none.
    pub(super) fn context_menu_items(
        &self,
    ) -> impl FnOnce() -> anyhow::Result<Vec<MenuItem>> + Send + 'static {
        let (store, config, project, current) = (
            self.backend.store.clone(),
            self.config.clone(),
            self.project_id,
            self.thread.as_ref().map(|t| t.id),
        );
        let account = self.composer_account().map(|a| a.id);
        move || {
            let Some(account) = account else {
                return Ok(Vec::new());
            };
            let s = super::lock(&store)?;
            let Some(p) = s.project(project)? else {
                return Ok(Vec::new());
            };
            let threads = s.list_threads(project)?;
            let replies = s.last_replies(Some(project))?;
            Ok(context_items(
                &threads,
                current,
                &account,
                &config,
                &p.path,
                &replies,
                crate::ui::now_ms(),
            ))
        }
    }

    /// Reads the picked thread (checked again: it may have moved since the menu opened) and adds its chip.
    pub(super) fn add_context_pick(&mut self, pick: ContextPick, cx: &mut Context<Self>) {
        let (ContextPick::Thread(id) | ContextPick::Pr(id)) = pick;
        let (store, config, project) = (
            self.backend.store.clone(),
            self.config.clone(),
            self.project_id,
        );
        let Some(account) = self.composer_account().map(|a| a.id) else {
            return;
        };
        let read = cx.background_spawn(async move {
            let s = super::lock(&store)?;
            let (Some(t), Some(p)) = (s.listed_thread(id)?, s.project(project)?) else {
                return anyhow::Ok(None);
            };
            if t.project_id != project || !may_reference(&t, &account, &config, &p.path) {
                return Ok(None);
            }
            let bound = Some((account, bound_folder(&t, &p.path).to_path_buf()));
            Ok(Some(match pick {
                ContextPick::Thread(_) => {
                    let reply = s.last_reply(t.id, terminal_context::MAX_REPLY_CHARS + 1)?;
                    let text = terminal_context::thread_summary(&t.title, reply.as_deref());
                    (ChipKind::Thread, t.title, text, bound)
                }
                ContextPick::Pr(_) => {
                    let Some(url) = t.pr_url else {
                        return Ok(None);
                    };
                    let text = terminal_context::pr_summary(&url, &t.title);
                    (ChipKind::PullRequest, pr_label(&url), text, bound)
                }
            }))
        });
        cx.spawn(async move |this, cx| {
            if let Ok(Some((kind, label, text, bound))) = read.await {
                // `push_context` checks `bound` again: a rebind may have landed during the read.
                let _ = this.update(cx, |this, cx| {
                    if this.push_context(kind, SharedString::from(label), text, bound, cx)
                        && let Some(chip) = this.composer.contexts.last_mut()
                    {
                        chip.thread = Some(id);
                    }
                });
            }
        })
        .detach();
    }
}
