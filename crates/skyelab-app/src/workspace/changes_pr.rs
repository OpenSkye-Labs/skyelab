//! A pull request's diff in the Changes view (T3's PR Code tab): read with `gh pr diff` under the account the folder
//! is bound to, shown with the same file tree and rows as a local diff, commented line by line, and sent to GitHub as
//! one review through the PR actions dialog. The diff is other people's text: it is only parsed and drawn as text, and
//! no path in it is opened or joined to a folder.

use super::ChangesView;
use crate::workspace::Workspace;
use crate::workspace::changes_model::{self, Row, Scope};
use crate::workspace::diff_comments::DiffComments;
use crate::workspace::pr_actions::{self, Target};
use gpui::{Action, AppContext as _, Context, SharedString, Window};
use skyelab_git::GitHub;
use std::path::PathBuf;

/// Opens (or shows again) the diff tab of a pull request.
#[derive(Clone, PartialEq, Action)]
#[action(namespace = skyelab, no_json)]
pub struct OpenPrDiff {
    pub target: Target,
}

/// What a PR tab reads and the review being written on it.
pub(in crate::workspace) struct PrSource {
    /// `head` is the commit the diff shown was read at, which the review's comments are pinned to.
    pub(in crate::workspace) target: Target,
    /// Pending line comments, sent together as one review.
    pub(in crate::workspace) review: DiffComments,
    gh: PathBuf,
    accounts_file: Option<PathBuf>,
}

impl PrSource {
    pub(super) fn title(&self) -> SharedString {
        format!("PR #{} · {}", self.target.number, self.repo()).into()
    }

    /// `owner/repo` of the PR's link.
    fn repo(&self) -> String {
        let path = self.target.url.trim_start_matches("https://github.com/");
        path.split('/').take(2).collect::<Vec<_>>().join("/")
    }
}

impl ChangesView {
    /// A tab showing `target`'s diff; nothing on disk is watched or diffed.
    pub fn new_pr(target: Target, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let title = format!("#{}", target.number).into();
        let mut this = Self::blank(target.dir.clone(), None, title, None, window, cx);
        this.pr = Some(PrSource {
            target,
            review: DiffComments::default(),
            gh: crate::workspace::gh_bin(cx),
            accounts_file: cx
                .try_global::<crate::workspace::AccountsFile>()
                .map(|f| f.0.clone()),
        });
        this.refresh(cx);
        this
    }

    /// Reads the diff with the account's gh off the UI thread; the folder must still be bound to the account the
    /// caller showed. A newer read drops the older one.
    pub(super) fn load_pr(&mut self, cx: &mut Context<Self>) {
        let Some(pr) = &self.pr else {
            return;
        };
        let (target, gh, file) = (pr.target.clone(), pr.gh.clone(), pr.accounts_file.clone());
        let task = cx.background_spawn(async move {
            let config = crate::workspace::accounts_now(file.as_deref(), &target.config);
            let gh_dir = pr_actions::account_gh_dir(&config, &target.dir, &target.account)?;
            let github = GitHub {
                gh: &gh,
                config_dir: &gh_dir,
            };
            let diff = skyelab_git::pr_diff(&github, &target.url)?;
            anyhow::Ok((changes_model::from_diffs(diff.files), diff.head))
        });
        self._load = Some(cx.spawn(async move |this, cx| {
            let loaded = task.await;
            let _ = this.update(cx, |this, cx| {
                let loaded = loaded.map(|(changes, head)| {
                    if let Some(pr) = &mut this.pr
                        && pr.target.head != head
                    {
                        // The checks the caller read were another commit's: a merge from here asks once more.
                        (pr.target.head, pr.target.checks) = (head, None);
                    }
                    changes
                });
                this.loaded(loaded, false, cx)
            });
        }));
    }

    /// "Submit review…": the PR actions dialog, with the pending comments going along with the verdict picked there.
    /// They are cleared once GitHub took the review.
    pub(in crate::workspace) fn submit_review(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<gpui::Entity<pr_actions::PrActions>> {
        let pr = self.pr.as_ref()?;
        let rows = self.changes.as_ref().map_or(&[][..], |c| &c.rows[..]);
        let mut comments = Vec::new();
        let mut sent = Vec::new();
        let mut refused = None;
        for c in pr.review.items.iter().filter(|c| c.scope == Scope::Working) {
            // GitHub takes a range only inside one hunk.
            let spans = rows
                .get(c.rows.clone())
                .is_some_and(|r| r.iter().any(|r| matches!(r, Row::Hunk(_))));
            match c.review_comment().filter(|_| !spans) {
                Some(rc) => {
                    comments.push(rc);
                    sent.push(c.id);
                }
                None => {
                    refused = Some(format!(
                        "GitHub can't take the comment on {}: keep a comment's lines inside one hunk, and don't \
                         start it on an added line when it ends on a removed one.",
                        c.label()
                    ));
                    break;
                }
            }
        }
        if let Some(why) = refused {
            self.publish_note = Some(why.into());
            cx.notify();
            return None;
        }
        self.publish_note = None;
        let view = cx.weak_entity();
        let dialog = pr_actions::open(
            pr.target.clone(),
            move |reviewed, _, cx| {
                _ = view.update(cx, |v, cx| {
                    // Only what went to GitHub: comments added while gh ran stay pending.
                    if reviewed {
                        v.update_review(cx, |r| r.items.retain(|c| !sent.contains(&c.id)));
                        v.comments_changed(cx);
                    }
                })
            },
            window,
            cx,
        );
        dialog.update(cx, |d, cx| {
            (d.comments, d.from_diff) = (comments, true);
            cx.notify();
        });
        Some(dialog)
    }
}

impl Workspace {
    /// The diff tab of `a.target`'s PR in the right dock: shown and read again if one is open, else added.
    pub(in crate::workspace) fn open_pr_diff(
        &mut self,
        a: &OpenPrDiff,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.pr_diffs.retain(|c| c.upgrade().is_some());
        let open = self.pr_diffs.iter().filter_map(|c| c.upgrade()).find(|c| {
            c.read(cx)
                .pr
                .as_ref()
                .is_some_and(|p| p.target.url == a.target.url && p.target.dir == a.target.dir)
        });
        if let Some(tab) = open {
            tab.update(cx, |c, cx| {
                // The caller's account and binding are the ones checked now.
                if let Some(p) = &mut c.pr {
                    // Pending comments were written for one account: another one doesn't post them.
                    if p.target.account != a.target.account {
                        p.review = DiffComments::default();
                    }
                    // Only a diff read here moves the head (and its checks): the comments' lines are that diff's.
                    let (head, checks) = (p.target.head.take(), p.target.checks);
                    p.target = Target {
                        head,
                        checks,
                        ..a.target.clone()
                    };
                }
                c.refresh(cx)
            });
            return self.show_right_panel(tab.entity_id().into(), window, cx);
        }
        let tab = cx.new(|cx| ChangesView::new_pr(a.target.clone(), window, cx));
        self.add_right_panel(tab.clone(), window, cx);
        self.pr_diffs.push(tab.downgrade());
        cx.notify();
    }
}
