//! Acting on an issue from an Issues page row: comment on it, close it (completed or not planned, with an optional
//! comment) or reopen it, in a small dialog shaped like the PR actions one. After T3's issue panel (its comment
//! composer and its Open / Closed / Not planned states). gh runs with the account the folder is bound to now, checked
//! against the account the page group showed (`pr_actions::account_gh_dir`), never the user's default gh login.

use super::gh_pages::{GhPage, Listing};
use super::pr_actions::account_gh_dir;
use super::usage_page_render::segments;
use crate::ui;
use crate::ui::{Button, SkyeIcon};
use gpui::{
    App, AppContext as _, Context, Entity, IntoElement, Render, SharedString, Window, div,
    prelude::*, px,
};
use gpui_component::{
    ActiveTheme as _, Sizable as _, h_flex,
    input::{InputEvent, Textarea, TextareaState},
    v_flex,
};
use skyelab_core::AccountsConfig;
use skyelab_git::{CloseReason, GitHub};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;

/// The issue and who acts on it.
#[derive(Clone, PartialEq)]
pub(crate) struct Target {
    /// A canonical GitHub issue link.
    pub url: String,
    pub number: u64,
    /// Shown in the dialog's title, so it says which issue it acts on.
    pub title: String,
    pub closed: bool,
    /// The folder whose binding picks the account.
    pub dir: PathBuf,
    /// The account the page group showed: the folder must still be bound to it.
    pub account: String,
    pub config: AccountsConfig,
}

/// What runs after an action succeeds (the page reads the issue again).
type OnDone = Rc<dyn Fn(&mut Window, &mut App)>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Act {
    Comment,
    Close(CloseReason),
    Reopen,
}

pub(crate) struct IssueActions {
    pub(crate) target: Target,
    gh: PathBuf,
    accounts_file: Option<PathBuf>,
    pub(crate) reason: CloseReason,
    pub(crate) busy: bool,
    pub(crate) error: Option<SharedString>,
    pub(crate) body: Entity<TextareaState>,
    on_done: OnDone,
}

/// Opens the dialog for `target`; `on_done` runs after an action succeeds (the dialog is closed by then).
pub(crate) fn open(
    target: Target,
    on_done: impl Fn(&mut Window, &mut App) + 'static,
    window: &mut Window,
    cx: &mut App,
) -> Entity<IssueActions> {
    let gh = super::gh_bin(cx);
    let accounts_file = cx.try_global::<super::AccountsFile>().map(|f| f.0.clone());
    let entity = cx.new(|cx| {
        let body = cx.new(|cx| {
            TextareaState::new(window, cx)
                .auto_grow(3, 8)
                .placeholder("Leave a comment")
        });
        // Comment turns on, and Close names the comment, as soon as there is text.
        cx.subscribe(&body, |_, _, e: &InputEvent, cx| {
            if matches!(e, InputEvent::Change) {
                cx.notify();
            }
        })
        .detach(); // dies with the dialog
        IssueActions {
            target: target.clone(),
            gh,
            accounts_file,
            reason: CloseReason::Completed,
            busy: false,
            error: None,
            body,
            on_done: Rc::new(on_done),
        }
    });
    let title: SharedString = format!("#{} {}", target.number, target.title).into();
    let view = entity.clone();
    crate::ui::open_dialog(window, cx, move |dialog, _, _| {
        dialog
            .w(px(480.))
            .title(title.clone())
            .child(view.clone())
            .footer(
                ui::dialog_footer().child(
                    ui::dialog_cancel("issue-actions-cancel")
                        .label("Close")
                        .accessibility_label("Close issue actions"),
                ),
            )
    });
    entity
}

impl IssueActions {
    pub(crate) fn act(&mut self, act: Act, window: &mut Window, cx: &mut Context<Self>) {
        // Sent as typed (an indented code block keeps its indent); blank means no comment.
        let body = self.body.read(cx).value().to_string();
        if self.busy || (act == Act::Comment && body.trim().is_empty()) {
            return;
        }
        (self.busy, self.error) = (true, None);
        let (target, gh, file) = (
            self.target.clone(),
            self.gh.clone(),
            self.accounts_file.clone(),
        );
        let done = cx.background_spawn(async move {
            let config = super::accounts_now(file.as_deref(), &target.config);
            let gh_dir = account_gh_dir(&config, &target.dir, &target.account)?;
            let github = GitHub {
                gh: &gh,
                config_dir: &gh_dir,
            };
            match act {
                Act::Comment => skyelab_git::comment_issue(&github, &target.url, &body)?,
                Act::Close(reason) => {
                    skyelab_git::close_issue(&github, &target.url, reason, &body)?
                }
                Act::Reopen => skyelab_git::reopen_issue(&github, &target.url, &body)?,
            }
            anyhow::Ok(())
        });
        let on_done = self.on_done.clone();
        cx.spawn_in(window, async move |this, cx| {
            let done = done.await;
            let ok = done.is_ok();
            let _ = this.update_in(cx, |this, window, cx| {
                this.busy = false;
                match done {
                    Err(e) => {
                        let e = format!("{e:#}");
                        // Posted already: a retry only closes or reopens.
                        if e.starts_with(skyelab_git::COMMENT_POSTED) {
                            this.body.update(cx, |b, cx| b.set_value("", window, cx));
                        }
                        this.error = Some(e.into());
                    }
                    Ok(()) => crate::ui::close_dialog(window, cx),
                }
                cx.notify();
            });
            // Also when the dialog was cancelled while gh ran: the issue changed all the same.
            if ok {
                let _ = cx.update(|window, cx| on_done(window, cx));
            }
        })
        .detach(); // gh is killed at its timeout; `this` is weak
        cx.notify();
    }
}

impl Render for IssueActions {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let has_text = !self.body.read(cx).value().trim().is_empty();
        let busy = self.busy;
        let theme = cx.theme();
        let link = ui::mono(
            self.target
                .url
                .trim_start_matches("https://github.com/")
                .to_owned(),
            theme,
        )
        .text_size(ui::META);
        let error = self.error.clone().map(|e| {
            div()
                .id("issue-actions-error")
                .text_size(ui::META)
                .text_color(ui::ink(theme, theme.danger))
                .child(e)
        });
        let comment = Button::new("issue-comment")
            .small()
            .outline()
            .label("Comment")
            .disabled(busy || !has_text)
            .on_click(cx.listener(|this, _, window, cx| this.act(Act::Comment, window, cx)));
        let mut buttons = h_flex().gap_2().items_center();
        if self.target.closed {
            buttons = buttons.child(div().flex_1()).child(comment).child(
                Button::new("issue-reopen")
                    .small()
                    .primary()
                    .icon(SkyeIcon::CircleDot)
                    .label(if has_text {
                        "Reopen with comment"
                    } else {
                        "Reopen issue"
                    })
                    .disabled(busy)
                    .loading(busy)
                    .on_click(cx.listener(|this, _, window, cx| this.act(Act::Reopen, window, cx))),
            );
        } else {
            let reasons = [
                (CloseReason::Completed, "Completed"),
                (CloseReason::NotPlanned, "Not planned"),
            ];
            let reason = self.reason;
            buttons = buttons
                .child(segments(
                    "issue-close-reason",
                    "Close reason",
                    &reasons,
                    reason,
                    |this: &mut Self, r, cx| {
                        this.reason = r;
                        cx.notify();
                    },
                    cx,
                ))
                .child(div().flex_1())
                .child(comment)
                .child(
                    Button::new("issue-close")
                        .small()
                        .primary()
                        .icon(SkyeIcon::CircleCheck)
                        .label(if has_text {
                            "Close with comment"
                        } else {
                            "Close issue"
                        })
                        .disabled(busy)
                        .loading(busy)
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.act(Act::Close(this.reason), window, cx)
                        })),
                );
        }
        v_flex()
            .gap_3()
            .child(link)
            .child(
                Textarea::new(&self.body)
                    .small()
                    .disabled(busy)
                    .aria_label("Issue comment"),
            )
            .child(buttons)
            .children(error)
    }
}

impl GhPage {
    /// The comment, close and reopen dialog for issue `number` of `project`'s shown list, acting with the group's
    /// account; that issue's row is read again after an action.
    pub(crate) fn open_issue_actions(
        &mut self,
        project: i64,
        number: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Entity<IssueActions>> {
        let source = self.sources.iter().find(|s| s.id == project)?;
        let group = self.groups.get(&(project, self.filter))?;
        let Listing::Issues(list) = &group.listing else {
            return None;
        };
        let issue = &list.iter().find(|r| r.row.number == number)?.row;
        // gh's JSON is untrusted: only a canonical issue link is acted on.
        let url = skyelab_git::issue_link(&issue.url).ok()?.to_owned();
        let target = Target {
            url,
            number,
            title: issue.title.clone(),
            closed: issue.closed,
            dir: source.path.clone(),
            account: group.account.as_ref()?.0.clone(),
            config: self.inputs.as_ref()?.config.clone(),
        };
        let page = cx.weak_entity();
        let done = target.clone();
        Some(open(
            target,
            move |_, cx| {
                _ = page.update(cx, |p, cx| p.reread_issue(project, done.clone(), cx));
            },
            window,
            cx,
        ))
    }

    /// Reads issue `target` again with the account that acted and swaps it into every cached list of `project` that
    /// shows it; a closed issue stays listed until Refresh, so it can be reopened. A failed read lists them again.
    fn reread_issue(&mut self, project: i64, target: Target, cx: &mut Context<Self>) {
        let Some(inputs) = &self.inputs else {
            return;
        };
        let (gh, file) = (inputs.gh.clone(), inputs.accounts_file.clone());
        let account = target.account.clone();
        let read = cx.background_spawn(async move {
            let config = super::accounts_now(file.as_deref(), &target.config);
            let gh_dir = account_gh_dir(&config, &target.dir, &target.account)?;
            let github = GitHub {
                gh: &gh,
                config_dir: &gh_dir,
            };
            anyhow::Ok(skyelab_git::issue_view(&github, &target.url)?)
        });
        cx.spawn(async move |this, cx| {
            let row = read.await;
            _ = this.update(cx, |this, cx| {
                let Ok(row) = row else {
                    this.groups.retain(|key, _| key.1 == this.filter);
                    return this.refresh(cx);
                };
                let fresh = super::gh_pages::dated(vec![row], |r| &r.updated_at);
                for (key, group) in &mut this.groups {
                    // A list read by another account since (a rebind) keeps its own rows.
                    if key.0 != project || group.account.as_ref().map(|a| &a.0) != Some(&account) {
                        continue;
                    }
                    if let Listing::Issues(list) = &mut group.listing
                        && let Some(at) = list.iter().position(|r| r.row.url == fresh[0].row.url)
                    {
                        Arc::make_mut(list)[at] = fresh[0].clone();
                    }
                }
                cx.notify();
            });
        })
        .detach(); // gh is killed at its timeout; `this` is weak
    }
}

#[cfg(all(test, unix))] // the fake gh is a shell script
#[path = "issue_actions_test.rs"]
mod tests;
