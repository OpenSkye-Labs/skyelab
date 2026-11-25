//! Scratch threads (T3's "No project" threads): quick questions outside any project. The user picks the account;
//! the thread runs in that account's scratch folder, bound to it on first use, and the folder shows as the project
//! "Scratch · <account>". Never a default account: no pick, no thread.

use super::Workspace;
use super::thread::ThreadView;
use crate::ui::toast::Toast;
use gpui::{Action, AppContext as _, Context, Window, actions};

actions!(skyelab, [NewScratchThread]);

/// A scratch thread under this account (the account picker's entries).
#[derive(Clone, PartialEq, Action)]
#[action(namespace = skyelab, no_json)]
pub struct NewScratchThreadFor {
    pub account: String,
}

/// The scratch project's name for an account labeled `label`.
pub(super) fn project_name(label: &str) -> String {
    format!("Scratch · {label}")
}

impl Workspace {
    pub(super) fn scratch_actions(
        &self,
        el: gpui::Stateful<gpui::Div>,
        cx: &Context<Self>,
    ) -> gpui::Stateful<gpui::Div> {
        use gpui::InteractiveElement as _;
        el.on_action(cx.listener(Self::new_scratch_thread))
            .on_action(cx.listener(Self::new_scratch_thread_for))
    }

    /// The palette as an account picker: "New scratch thread with: <account>" for each one.
    fn new_scratch_thread(
        &mut self,
        _: &NewScratchThread,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.is_remote() {
            return; // the folder and binding would land on this machine, not the one shown
        }
        if self.config.accounts.is_empty() {
            return self.open_onboarding(&super::OpenOnboarding, window, cx);
        }
        let Some(prev) = window.focused(cx) else {
            return;
        };
        let entries = self
            .config
            .accounts
            .iter()
            .map(|a| {
                let account = a.id.clone();
                crate::palette::Entry::new(
                    format!("New scratch thread with: {}", a.label),
                    crate::palette::Kind::Project,
                    std::rc::Rc::new(move |window, cx| {
                        let account = account.clone();
                        window.dispatch_action(Box::new(NewScratchThreadFor { account }), cx)
                    }),
                )
                .title(a.label.clone())
                .icon(crate::ui::SkyeIcon::User)
            })
            .collect();
        crate::palette::open_page("New scratch thread with", entries, prev, window, cx);
    }

    /// Binds the account's scratch folder (creating it on first use), stores it as a project and opens a new thread.
    pub(super) fn new_scratch_thread_for(
        &mut self,
        a: &NewScratchThreadFor,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.is_remote() {
            return;
        }
        let Some(root) = self.scratch_root.clone() else {
            return self.scratch_failed(
                "no data folder on this OS for scratch threads",
                window,
                cx,
            );
        };
        let mut dir = None;
        let bound = self.edit_accounts(
            |c| {
                dir = Some(skyelab_core::scratch::prepare(c, &root, &a.account)?);
                Ok(())
            },
            cx,
        );
        let dir = match (bound, dir) {
            (Ok(()), Some(dir)) => dir,
            (Err(e), _) => return self.scratch_failed(&format!("{e:#}"), window, cx),
            (Ok(()), None) => return, // `prepare` ran and returned, so the folder is set
        };
        let label = self.config.resolve(&dir).map(|acct| acct.label.clone());
        let name = project_name(label.as_deref().unwrap_or(&a.account));
        let store = self.backend.store.clone();
        let stored = cx.background_spawn(async move {
            let store = super::lock(&store)?;
            match store.readd_project(&dir)? {
                Some(p) => anyhow::Ok(p),
                None => store.create_project(&name, &dir),
            }
        });
        self._add = Some(cx.spawn_in(window, async move |this, cx| {
            let stored = stored.await;
            let _ = this.update_in(cx, |this, window, cx| match stored {
                Ok(project) => {
                    this.scope.clear();
                    this.load_projects(Some(project.id), cx);
                    let (config, backend) = (this.config.clone(), this.backend.clone());
                    let view = cx.new(|cx| ThreadView::new(&project, config, backend, window, cx));
                    this.add_thread_tab(view, true, window, cx);
                }
                Err(e) => this.scratch_failed(&format!("{e:#}"), window, cx),
            });
        }));
    }

    fn scratch_failed(&mut self, why: &str, window: &mut Window, cx: &mut Context<Self>) {
        let note = Toast::error(format!("Couldn't start a scratch thread: {why}"));
        crate::ui::toast::push(window, note, cx);
        cx.notify();
    }
}
