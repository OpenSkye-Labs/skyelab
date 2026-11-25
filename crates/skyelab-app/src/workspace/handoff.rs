//! "Continue in Codex" / "Continue in Claude" / "Continue in Cursor": a new thread in the same project under the
//! folder's bound account of another provider, linked to its source, whose first session starts with the source's
//! portable transcript ([`skyelab_agent::handoff`]). A folder has one binding, so a handoff starts only once the
//! folder is bound to an account of that provider; until then the user is asked to bind it. Never another account.

use crate::ui::Button;
use crate::workspace::thread::ThreadView;
use crate::workspace::{OpenSettingsSection, OpenThread, Workspace, lock};
use anyhow::{Context as _, bail};
use gpui::{
    Action, AnyElement, AppContext as _, Context, InteractiveElement, IntoElement as _,
    ParentElement as _, SharedString, Window, actions, div,
};
use gpui_component::WindowExt as _;
use skyelab_agent::{handoff, timeline::Record};
use skyelab_core::Provider;
use skyelab_store::{NewThread, Thread};

actions!(
    skyelab,
    [
        ContinueInCodex,
        ContinueInClaude,
        ContinueInCursor,
        ContinueInAntigravity
    ]
);

/// Continues thread `id` in a new thread under the folder's `to` account.
#[derive(Clone, Debug, PartialEq, Action)]
#[action(namespace = skyelab, no_json)]
pub struct HandOffThread {
    pub id: i64,
    pub to: Provider,
}

/// The other providers a thread that ran on `from` can continue in.
pub(in crate::workspace) fn others(from: Provider) -> Vec<Provider> {
    match from {
        Provider::Claude => vec![Provider::Codex, Provider::Cursor, Provider::Antigravity],
        Provider::Codex => vec![Provider::Claude, Provider::Cursor, Provider::Antigravity],
        Provider::Cursor => vec![Provider::Claude, Provider::Codex, Provider::Antigravity],
        Provider::Antigravity => vec![Provider::Claude, Provider::Codex, Provider::Cursor],
    }
}

/// "Continue in Codex".
pub(in crate::workspace) fn label(to: Provider) -> SharedString {
    format!("Continue in {}", to.label()).into()
}

/// Source threads whose handoff is still being created: a second click would make a second thread.
#[derive(Default)]
struct HandingOff(std::collections::HashSet<i64>);
impl gpui::Global for HandingOff {}

enum Outcome {
    Started(Box<Thread>),
    /// The folder isn't bound to a `to` account: what it is bound to now, if anything.
    NeedsBinding {
        folder: String,
        bound: Option<String>,
    },
}

impl Workspace {
    /// Creates the continuing thread and opens it, or asks the user to bind the folder to a `to` account.
    pub(in crate::workspace) fn hand_off_thread(
        &mut self,
        a: &HandOffThread,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.is_remote() {
            return; // a remote window's threads live on another machine; their history isn't here
        }
        if !cx.default_global::<HandingOff>().0.insert(a.id) {
            return;
        }
        let (store, config, id, to, prefs) = (
            self.backend.store.clone(),
            self.accounts_job(cx),
            a.id,
            a.to,
            crate::workspace::thread_extras::prefs::get(cx),
        );
        let task = cx.background_spawn(async move {
            let config = config();
            let mut s = lock(&store)?;
            let src = s.thread(id)?.context("the thread is gone")?;
            let project = s.project(src.project_id)?.context("its project is gone")?;
            let from = config
                .accounts
                .iter()
                .find(|a| a.id == src.account_id)
                .map(|a| a.provider);
            if from == Some(to) {
                bail!("this thread already runs on {}", to.label());
            }
            let cwd = crate::workspace::thread_extras::fork::start_folder(&src, &project.path);
            let account = match config.resolve(&cwd) {
                Some(a) if a.provider == to => a.clone(),
                bound => {
                    return Ok(Outcome::NeedsBinding {
                        folder: crate::ui::short_path(&cwd, dirs::home_dir().as_deref()),
                        bound: bound.map(|a| format!("{} ({})", a.label, a.provider.label())),
                    });
                }
            };
            let mut events = s.events_since(id, 0)?;
            // A handoff copies no events, so an earlier handoff's history is its source's: walk up the links (a fork
            // already holds its source's events up to the cut; the bound only guards a broken chain).
            let mut at = src.clone();
            for _ in 0..32 {
                let Some(parent) = at.forked_from.map(|p| s.thread(p)).transpose()?.flatten()
                else {
                    break;
                };
                if parent.account_id != at.account_id {
                    let mut earlier = s.events_since(parent.id, 0)?;
                    earlier.append(&mut events);
                    events = earlier;
                }
                at = parent;
            }
            let records: Vec<Record> = events
                .iter()
                .filter_map(|e| Record::from_row(&e.kind, &e.payload_json).ok())
                .collect();
            let context = handoff::prompt(
                &src.title,
                from.map_or("another agent", Provider::label),
                to.label(),
                &handoff::items(&records),
                handoff::BUDGET,
            );
            let new = NewThread {
                project_id: project.id,
                title: &src.title,
                cwd: &cwd,
                account_id: &account.id,
                status: "stopped",
            };
            // No copied events: the other provider's rows would replay as this thread's own.
            let mut thread = s.create_fork(&new, id, &context, &[])?;
            crate::workspace::thread_extras::fork::set_new_mode(
                &s,
                &mut thread,
                &prefs,
                &project.path,
            )?;
            Ok(Outcome::Started(Box::new(thread)))
        });
        cx.spawn_in(window, async move |this, cx| {
            let done = task.await;
            let _ = this.update_in(cx, |ws, window, cx| {
                cx.default_global::<HandingOff>().0.remove(&id);
                match done {
                    Ok(Outcome::Started(thread)) => {
                        ws.thread_started(&thread, cx);
                        ws.open_thread(&OpenThread { id: thread.id }, window, cx);
                    }
                    Ok(Outcome::NeedsBinding { folder, bound }) => {
                        ask_bind(folder, bound, to, window, cx)
                    }
                    Err(e) => ws.say(format!("Couldn't continue the thread: {e:#}"), window, cx),
                }
            });
        })
        .detach();
    }
}

/// Asks the user to bind `folder` to a `to` account (it is bound to `bound`, or to nothing).
fn ask_bind(
    folder: String,
    bound: Option<String>,
    to: Provider,
    window: &mut Window,
    cx: &mut gpui::App,
) {
    let now = match bound {
        Some(b) => format!("{folder} is bound to {b}."),
        None => format!("{folder} isn't bound to an account."),
    };
    let to = to.label();
    let body = format!(
        "{now} A thread continues in {to} only under the {to} account this folder is bound to; SkyeLab never picks \
         one for you. Bind the folder to a {to} account in Settings → Accounts, then choose Continue in {to} again. \
         Threads of the other provider here can't send while it is bound to {to}."
    );
    crate::ui::open_dialog(window, cx, move |dialog, _, _| {
        dialog
            .w(gpui::px(440.))
            .title(format!("Bind this folder to a {to} account"))
            .child(body.clone())
            .footer(
                crate::ui::dialog_footer()
                    .child(crate::ui::dialog_cancel("handoff-cancel"))
                    .child(
                        Button::new("handoff-bind")
                            .primary()
                            .small()
                            .label("Open Settings")
                            .on_click(|_, window, cx| {
                                window.close_dialog(cx);
                                crate::workspace::dispatch(
                                    Box::new(OpenSettingsSection {
                                        section: crate::settings::Section::Accounts,
                                    }),
                                    window,
                                    cx,
                                );
                            }),
                    ),
            )
    });
}

impl ThreadView {
    /// The provider of the account this thread ran under: `None` before its first send, for a side chat, or when
    /// that account is gone from the config.
    pub(in crate::workspace) fn provider(&self) -> Option<Provider> {
        let t = self.thread.as_ref().filter(|t| t.side_of.is_none())?;
        self.config
            .accounts
            .iter()
            .find(|a| a.id == t.account_id)
            .map(|a| a.provider)
    }

    /// The providers this thread can continue in from this window: the others, unless the window shows another
    /// machine (the new thread couldn't open there).
    /// The providers this thread can continue in: the others that have at least one account here (a provider with
    /// none could only ever say to add one).
    pub(in crate::workspace) fn handoff_targets(&self) -> Vec<Provider> {
        let mut targets = self
            .provider()
            .filter(|_| !self.remote_window)
            .map(others)
            .unwrap_or_default();
        targets.retain(|to| self.config.accounts.iter().any(|a| a.provider == *to));
        targets
    }

    /// Asks the workspace to continue this thread in `to`.
    pub(in crate::workspace) fn hand_off(
        &mut self,
        to: Provider,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(t) = &self.thread {
            crate::workspace::dispatch(Box::new(HandOffThread { id: t.id, to }), window, cx);
        }
    }

    /// Registers only the other providers' "Continue in …", so the palette lists them.
    pub(in crate::workspace) fn handoff_actions<E: InteractiveElement>(
        &self,
        mut el: E,
        cx: &Context<Self>,
    ) -> E {
        for to in self.handoff_targets() {
            el = match to {
                Provider::Codex => el.on_action(cx.listener(Self::continue_in_codex)),
                Provider::Claude => el.on_action(cx.listener(Self::continue_in_claude)),
                Provider::Cursor => el.on_action(cx.listener(Self::continue_in_cursor)),
                Provider::Antigravity => el.on_action(cx.listener(Self::continue_in_antigravity)),
            };
        }
        el
    }

    /// The "can't continue here" notice's button when the folder is now bound to another provider: the thread can
    /// continue there with its history.
    pub(in crate::workspace) fn handoff_cta(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let to = self.folder_account.as_ref()?.provider;
        self.handoff_targets().contains(&to).then_some(())?;
        Some(
            div()
                .debug_selector(|| "handoff-cta".into())
                .child(
                    Button::new("handoff-cta")
                        .primary()
                        .small()
                        .label(label(to))
                        .on_click(
                            cx.listener(move |this, _, window, cx| this.hand_off(to, window, cx)),
                        ),
                )
                .into_any_element(),
        )
    }

    pub(in crate::workspace) fn continue_in_codex(
        &mut self,
        _: &ContinueInCodex,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.hand_off(Provider::Codex, window, cx);
    }

    pub(in crate::workspace) fn continue_in_claude(
        &mut self,
        _: &ContinueInClaude,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.hand_off(Provider::Claude, window, cx);
    }

    pub(in crate::workspace) fn continue_in_cursor(
        &mut self,
        _: &ContinueInCursor,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.hand_off(Provider::Cursor, window, cx);
    }

    pub(in crate::workspace) fn continue_in_antigravity(
        &mut self,
        _: &ContinueInAntigravity,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.hand_off(Provider::Antigravity, window, cx);
    }
}
