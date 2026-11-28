//! The Changes tab's sparkle buttons: a commit message, or a PR title and body, written by the thread's own account's
//! agent (one call with no tools, see `skyelab_agent::one_shot`) from what Commit would commit, or from the branch
//! against its base. The text only fills the editable fields; nothing is committed or opened. Settings → Threads
//! turns the buttons off.

use super::changes::ChangesView;
use super::thread_extras::title::Bins;
use super::thread_model::Record;
use super::{SharedStore, lock};
use crate::ui::Button;
use crate::ui::Icon;
use crate::ui::SkyeIcon;
use anyhow::{Context as _, Result, bail};
use gpui::{AnyElement, App, Context, Window, prelude::*};
use skyelab_core::AccountsConfig;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Gen {
    Commit,
    Pr,
}

/// What a generation wrote.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Text {
    Commit(String),
    Pr(String, String),
}

/// The thread's title and its last prompt (empty if it has none yet).
fn thread_context(store: &SharedStore, id: i64) -> Result<(String, String)> {
    let s = lock(store)?;
    let title = s.thread(id)?.context("the thread is gone")?.title;
    // shortcut: reads the whole thread for its last prompt; keep a last-prompt column if that gets slow.
    let last = s
        .events_since(id, 0)?
        .iter()
        .filter_map(|e| match Record::from_row(&e.kind, &e.payload_json) {
            Ok(Record::Prompt(p)) => Some(p),
            _ => None,
        })
        .next_back()
        .unwrap_or_default();
    Ok((title, last))
}

/// Writes `what` for thread `id` in `dir` under `account_id`, which `dir` must still resolve to (else refused, never
/// another account). `paths` are the ticked files (none: everything Commit would take).
pub(super) fn generate(
    what: Gen,
    store: &SharedStore,
    config: &AccountsConfig,
    bins: &Bins,
    (id, account_id): (i64, &str),
    dir: &Path,
    paths: &[PathBuf],
) -> Result<Text> {
    let account = match config.resolve(dir) {
        Some(a) if a.id == account_id => a.clone(),
        _ => bail!(
            "{} isn't bound to {account_id} any more, so its agent can't write this",
            dir.display()
        ),
    };
    let bin = bins.of(account.provider);
    let (title, last) = thread_context(store, id)?;
    match what {
        Gen::Commit => {
            let diff = skyelab_git::commit_diff(dir, paths)?;
            if diff.trim().is_empty() {
                bail!("nothing to describe: no changes in this folder");
            }
            let prompt = skyelab_agent::commit_prompt(&title, &last, &diff);
            skyelab_agent::generate_commit_message(bin, config, &account, dir, &prompt)
                .map(Text::Commit)
        }
        Gen::Pr => {
            let pr = skyelab_git::pr_context(dir)?;
            if pr.commits.trim().is_empty() {
                bail!("this branch has no commits ahead of {}", pr.base);
            }
            let prompt = skyelab_agent::pr_prompt(
                &title,
                &last,
                &pr.base,
                &pr.commits,
                &pr.diff,
                pr.template.as_deref(),
            );
            let (t, body) = skyelab_agent::generate_pr(bin, config, &account, dir, &prompt)?;
            Ok(Text::Pr(t, body))
        }
    }
}

impl ChangesView {
    /// The sparkle inside the commit message or PR title field; `None` while Settings → Threads has it off.
    pub(super) fn generate_button(&self, what: Gen, cx: &Context<Self>) -> Option<AnyElement> {
        if !super::thread_extras::prefs::get(cx).generate_git_text {
            return None;
        }
        let (id, tip) = match what {
            Gen::Commit => ("generate-commit", "Generate a commit message"),
            Gen::Pr => ("generate-pr", "Generate the PR title and description"),
        };
        Some(
            Button::new(id)
                .ghost()
                .xsmall()
                .icon(Icon::new(SkyeIcon::Sparkles))
                .tooltip(tip)
                .loading(self.generating == Some(what))
                .disabled(self.busy || self.generating.is_some())
                .on_click(cx.listener(move |this, _, window, cx| this.generate(what, window, cx)))
                .into_any_element(),
        )
    }

    /// Asks the thread's agent for `what` and puts it in the fields, unless the user changed them meanwhile.
    pub(super) fn generate(&mut self, what: Gen, window: &mut Window, cx: &mut Context<Self>) {
        if self.generating.is_some() {
            return;
        }
        if !super::thread_extras::prefs::get(cx).generate_git_text {
            self.publish_error = Some("Generating is off in Settings → Threads.".into());
            cx.notify();
            return;
        }
        let target = match self.target(cx) {
            Ok(t) => t,
            Err(e) => {
                self.publish_error = Some(format!("{e:#}").into());
                cx.notify();
                return;
            }
        };
        let Some(claude) = self
            .thread
            .as_ref()
            .and_then(|t| t.upgrade())
            .map(|t| t.read(cx).backend.claude_bin.clone())
        else {
            return;
        };
        let bins = Bins::new(claude, cx);
        let paths = self.picked_paths();
        let accounts_file = cx.try_global::<super::AccountsFile>().map(|f| f.0.clone());
        let before = self.field_text(what, cx);
        self.generating = Some(what);
        self.publish_error = None;
        self.publish_note = None;
        cx.notify();
        let task = cx.background_spawn(async move {
            let config = super::accounts_now(accounts_file.as_deref(), &target.config);
            let who = (target.thread_id, target.account_id.as_str());
            generate(
                what,
                &target.store,
                &config,
                &bins,
                who,
                &target.dir,
                &paths,
            )
        });
        self._generate = Some(cx.spawn_in(window, async move |this, cx| {
            let result = task.await;
            let _ = this.update_in(cx, |this, window, cx| {
                this.generating = None;
                match result {
                    Ok(_) if this.field_text(what, cx) != before => {
                        this.publish_note =
                            Some("You edited the field meanwhile, so it was left as it is.".into());
                    }
                    Ok(Text::Commit(msg)) => this
                        .commit_message
                        .update(cx, |s, cx| s.set_value(msg, window, cx)),
                    Ok(Text::Pr(title, body)) => {
                        this.pr_title
                            .update(cx, |s, cx| s.set_value(title, window, cx));
                        this.pr_body
                            .update(cx, |s, cx| s.set_value(body, window, cx));
                    }
                    Err(e) => this.publish_error = Some(format!("{e:#}").into()),
                }
                cx.notify();
            });
        }));
    }

    fn field_text(&self, what: Gen, cx: &App) -> (String, String) {
        match what {
            Gen::Commit => (
                self.commit_message.read(cx).value().to_string(),
                String::new(),
            ),
            Gen::Pr => (
                self.pr_title.read(cx).value().to_string(),
                self.pr_body.read(cx).value().to_string(),
            ),
        }
    }
}
