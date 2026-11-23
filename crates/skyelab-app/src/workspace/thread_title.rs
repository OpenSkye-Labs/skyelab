//! Generated thread titles: once the first turn ends, and "Regenerate title" in the thread menu. Local threads only:
//! the call runs on this machine under the thread's own account, resolved again from its folder (never another one).

use crate::workspace::thread::{ThreadView, ThreadViewEvent};
use crate::workspace::thread_model::{Item, Record};
use crate::workspace::{SharedStore, Workspace, lock};
use anyhow::{Context as _, Result, bail};
use gpui::{App, AppContext as _, Context, Window};
use skyelab_agent::{Block, Event};
use skyelab_core::{AccountsConfig, Provider};
use std::path::PathBuf;

/// The binaries a title call may run: `claude` and `codex` (or their fakes).
#[derive(Clone)]
pub(in crate::workspace) struct Bins {
    claude: PathBuf,
    codex: PathBuf,
}

impl Bins {
    pub(in crate::workspace) fn new(claude: PathBuf, cx: &App) -> Self {
        Self {
            claude,
            codex: crate::workspace::github::codex_bin(cx),
        }
    }
}

/// A thread's first prompt and the text of its first reply (empty if that turn wrote none).
pub(in crate::workspace) fn first_exchange(
    events: &[skyelab_store::Event],
) -> Option<(String, String)> {
    let mut prompt = None;
    let mut reply = String::new();
    for e in events {
        match Record::from_row(&e.kind, &e.payload_json) {
            Ok(Record::Prompt(p)) if prompt.is_none() => prompt = Some(p),
            Ok(Record::Agent(Event::Assistant { content })) if prompt.is_some() => {
                for b in content {
                    if let Block::Text { text } = b {
                        reply.push_str(&text);
                        reply.push('\n');
                    }
                }
            }
            Ok(Record::Agent(Event::Result { .. })) if prompt.is_some() => break,
            _ => {}
        }
    }
    Some((prompt?, reply))
}

/// Names thread `id` and stores the title. Without `force` only while its title is still the prompt's first line: then
/// `Ok(None)` (and no call at all once the user renamed it). `Some(title)` once stored.
pub(in crate::workspace) fn generate(
    store: &SharedStore,
    config: &AccountsConfig,
    bins: &Bins,
    id: i64,
    force: bool,
) -> Result<Option<String>> {
    let (thread, events) = {
        let s = lock(store)?;
        if !force && s.title_source(id)?.as_deref() != Some("prompt") {
            return Ok(None);
        }
        let thread = s.thread(id)?.context("the thread is gone")?;
        // shortcut: reads the whole thread for its first exchange; read only up to the first Result if that is slow.
        (thread, s.events_since(id, 0)?)
    };
    let (prompt, reply) =
        first_exchange(&events).context("the thread has no message to name it by yet")?;
    let account = match config.resolve(&thread.cwd) {
        Some(a) if a.id == thread.account_id => a.clone(),
        _ => bail!(
            "{} isn't bound to {} any more, so its agent can't name this thread",
            thread.cwd.display(),
            thread.account_id
        ),
    };
    let bin = match account.provider {
        Provider::Claude => &bins.claude,
        Provider::Codex => &bins.codex,
    };
    let prompt = skyelab_agent::title_prompt(&prompt, &reply);
    let title = skyelab_agent::generate_title(bin, config, &account, &thread.cwd, &prompt)?;
    Ok(lock(store)?
        .set_generated_title(id, &title, force)?
        .then_some(title))
}

impl ThreadView {
    /// A turn ended: after the thread's first one, its agent names it (when Settings → Threads says so). A rename made
    /// meanwhile wins: only a title the store took is shown.
    pub(in crate::workspace) fn title_after_turn(&mut self, cx: &mut Context<Self>) {
        let first = self
            .items
            .iter()
            .filter(|i| matches!(i, Item::Done { .. }))
            .count()
            == 1;
        let Some(id) = self.thread.as_ref().map(|t| t.id) else {
            return;
        };
        if !first || !super::prefs::get(cx).generate_titles {
            return;
        }
        let (store, bins) = (
            self.backend.store.clone(),
            Bins::new(self.backend.claude_bin.clone(), cx),
        );
        let file = cx
            .try_global::<crate::workspace::AccountsFile>()
            .map(|f| f.0.clone());
        let config = self.config.clone();
        let task = cx.background_spawn(async move {
            let config = crate::workspace::accounts_now(file.as_deref(), &config);
            generate(&store, &config, &bins, id, false)
        });
        cx.spawn(async move |this, cx| {
            // A failed title call leaves the first-line title; nothing to tell the user mid-thread.
            if let Ok(Some(title)) = task.await {
                let _ = this.update(cx, |this, cx| this.retitled(id, title, cx));
            }
        })
        .detach();
    }

    pub(in crate::workspace) fn retitled(
        &mut self,
        id: i64,
        title: String,
        cx: &mut Context<Self>,
    ) {
        if let Some(t) = self.thread.as_mut().filter(|t| t.id == id) {
            t.title = title.clone();
            cx.emit(ThreadViewEvent::Retitled(title));
            cx.notify();
        }
    }
}

impl Workspace {
    /// The thread menu's "Regenerate title": names it again, replacing any title, under the thread's own account.
    pub(in crate::workspace) fn regenerate_title(
        &mut self,
        id: i64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (store, bins, config) = (
            self.backend.store.clone(),
            Bins::new(self.backend.claude_bin.clone(), cx),
            self.accounts_job(cx),
        );
        let task = cx.background_spawn(async move { generate(&store, &config(), &bins, id, true) });
        cx.spawn_in(window, async move |this, cx| {
            let done = task.await;
            let _ = this.update_in(cx, |ws, window, cx| match done {
                Ok(Some(title)) => {
                    if let Some(view) = ws.open_view(id, cx) {
                        view.update(cx, |v, cx| v.retitled(id, title.clone(), cx));
                    }
                    ws.update_row(id, |t| t.title = title, cx);
                }
                Ok(None) => {}
                Err(e) => ws.say(format!("Couldn't name the thread: {e:#}"), window, cx),
            });
        })
        .detach();
    }
}
