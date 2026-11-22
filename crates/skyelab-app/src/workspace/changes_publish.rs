//! The Changes tab's Commit / Push / Open PR. Each runs only while the folder still resolves to the
//! thread's recorded account; Push and Open PR also need that account's GitHub login.

use super::changes::ChangesView;
use super::{SharedStore, lock};
use anyhow::{Context as _, Result, bail};
use gpui::{AnyElement, App, Context, Window, div, prelude::*};
use gpui_component::{
    ActiveTheme as _, Disableable as _, Sizable as _,
    button::{Button, ButtonVariants as _},
    h_flex,
    input::{Input, Textarea},
    v_flex,
};
use skyelab_git::GitHub;
use std::path::PathBuf;

/// What an action runs against, read from the thread at click time.
struct Target {
    dir: PathBuf,
    thread_id: i64,
    store: SharedStore,
    /// The account's `GH_CONFIG_DIR`, when the action needs GitHub.
    gh_dir: Option<PathBuf>,
}

impl ChangesView {
    fn target(&self, github: bool, cx: &App) -> Result<Target> {
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
        let dir = self.dir.display();
        match view.config.resolve(&self.dir) {
            Some(a) if a.id == row.account_id => {}
            Some(a) => bail!(
                "{dir} is now bound to '{}', not this thread's account ('{}'); start a new thread to publish as '{}'",
                a.id,
                row.account_id,
                a.id
            ),
            None => bail!(
                "{dir} isn't bound to '{}' any more (unbound, or its binding needs re-confirming in Settings)",
                row.account_id
            ),
        }
        let gh_dir = github
            .then(|| view.config.github_config_dir(&self.dir))
            .transpose()?;
        Ok(Target {
            dir: self.dir.clone(),
            thread_id: row.id,
            store: view.backend.store.clone(),
            gh_dir,
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
        let target = match self.target(github, cx) {
            Ok(t) => t,
            Err(e) => {
                self.publish_error = Some(format!("{e:#}").into());
                cx.notify();
                return;
            }
        };
        let gh_bin = super::gh_bin(cx);
        self.busy = true;
        self.publish_error = None;
        cx.notify();
        let task = cx.background_spawn(async move {
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

    pub(super) fn commit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let message = self.commit_message.read(cx).value().to_string();
        let files = self
            .changes
            .as_ref()
            .map(|c| c.files.as_slice())
            .unwrap_or_default();
        // A ticked rename takes its old path along, so the deletion lands in the same commit.
        let paths: Vec<PathBuf> = files
            .iter()
            .filter(|f| self.selected.contains(&f.path))
            .flat_map(|f| std::iter::once(f.path.clone()).chain(f.old_path.clone()))
            .collect();
        self.run(
            false,
            window,
            cx,
            move |t, _| {
                let sha = skyelab_git::commit(&t.dir, &message, &paths)?;
                Ok(format!("Committed {}", sha.get(..8).unwrap_or(&sha)))
            },
            |this, note, window, cx| {
                this.publish_note = Some(note.into());
                this.selected.clear();
                this.commit_message
                    .update(cx, |s, cx| s.set_value("", window, cx));
            },
        );
    }

    pub(super) fn push(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.run(
            true,
            window,
            cx,
            |t, gh| {
                let to = skyelab_git::push(&t.dir, &gh.context("no GitHub login")?)?;
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
                lock(&t.store)?.set_thread_pr_url(t.thread_id, Some(&url))?;
                Ok(url)
            },
            |this, url, _, cx| {
                this.publish_note = Some(format!("Opened {url}").into());
                if let Some(view) = this.thread.as_ref().and_then(|t| t.upgrade()) {
                    view.update(cx, |t, cx| t.set_pr_url(url, cx));
                }
            },
        );
    }

    pub(super) fn render_publish(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let n = self.selected.len();
        let commit_label = match n {
            0 => "Commit all".to_string(),
            1 => "Commit 1 file".to_string(),
            n => format!("Commit {n} files"),
        };
        let button = |id: &'static str, label: String| {
            Button::new(id)
                .xsmall()
                .label(label)
                .flex_shrink_0()
                .disabled(self.busy)
        };
        v_flex()
            .px_4()
            .py_2()
            .gap_2()
            .border_b_1()
            .border_color(theme.border)
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        div()
                            .flex_1()
                            .child(Input::new(&self.commit_message).small()),
                    )
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
                    .text_color(theme.danger)
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
