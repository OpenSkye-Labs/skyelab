//! What the composer's model chip names: the model that will run, never a bare "default" (T3's "Opus 5.5 High").

use super::composer_menu::MenuKind;
use super::composer_picker::effort_label;
use super::lock;
use crate::workspace::thread::ThreadView;
use gpui::{AppContext as _, Context, SharedString};
use skyelab_agent::{CLAUDE_MODELS, ModelInfo, model_label};
use skyelab_core::Provider;
use skyelab_store::ModelChoice;

impl ThreadView {
    /// Picks a model and effort: kept on the thread, remembered as the project's default for new threads, and
    /// handed to a running agent.
    pub(in crate::workspace) fn pick_model(&mut self, pick: ModelChoice, cx: &mut Context<Self>) {
        self.composer.pick = pick.clone();
        self.composer.picked = true;
        self.close_picker(cx);
        let (store, thread, project) = (
            self.backend.store.clone(),
            self.thread.as_ref().map(|t| t.id),
            self.project_id,
        );
        let session = self.session.clone();
        // Two quick picks save in two tasks that may run out of order: only the newest writes.
        let latest = self.composer.pick_gen.clone();
        let this_pick = latest.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
        self.in_background(cx, move || {
            {
                let s = lock(&store)?;
                if latest.load(std::sync::atomic::Ordering::SeqCst) != this_pick {
                    return Ok(());
                }
                if let Some(id) = thread {
                    s.set_thread_model(id, &pick)?;
                }
                s.set_project_model(project, &pick)?;
            }
            if let Some(session) = session {
                let mut session = lock(&session)?;
                // Checked again under this lock: a newer pick may have reached the agent since the store write.
                if latest.load(std::sync::atomic::Ordering::SeqCst) == this_pick {
                    session.set_model(pick.model.as_deref(), pick.effort.as_deref())?;
                }
            }
            Ok(())
        });
        cx.notify();
    }

    /// The chip's label ("Opus 5.5 High") and whether it names the account's default rather than a pick. `compact`
    /// leaves the effort out.
    pub(in crate::workspace) fn chip_label(&self, compact: bool) -> (String, bool) {
        let c = &self.composer;
        if let Some(set) = c.compare.as_ref().filter(|s| s.models.len() > 1) {
            return (format!("{} models", set.models.len()), false);
        }
        let (model, default) = match c.pick.model.as_deref() {
            Some(id) => (self.pick_name(id), false),
            None => (self.default_model_name(), true),
        };
        match &c.pick.effort {
            Some(e) if !compact => (format!("{model} {}", effort_label(e)), default),
            _ => (model, default),
        }
    }

    /// The model the account runs with no pick: what this thread's last run on the default reported, else the account's own config,
    /// else Codex's listed default, else "Claude default" / "Codex default".
    pub(in crate::workspace) fn default_model_name(&self) -> String {
        let c = &self.composer;
        let listed = self.listed();
        let account = c.account_model.as_deref();
        // A run's model names the default only if that run had no pick. A replayed one (pick unknown) must at least
        // match the account's own default.
        // shortcut: a replayed run under a since-cleared pick still names it when the account sets no model; store
        // each run's pick with its init line if that matters.
        let reported = c
            .reported_model
            .as_deref()
            .filter(|r| match &c.reported_pick {
                Some(pick) => pick.is_none(),
                None => account.is_none_or(|a| *r == a || r.starts_with(&format!("claude-{a}-"))),
            });
        reported
            .or(account)
            .map(|id| label_for(id, listed))
            .or_else(|| {
                listed?
                    .iter()
                    .find(|m| m.is_default)
                    .map(|m| m.label.clone())
            })
            .unwrap_or_else(|| match self.composer_account().map(|a| a.provider) {
                Some(Provider::Codex) => "Codex default".into(),
                Some(Provider::Cursor) => "Cursor default".into(),
                _ => "Claude default".into(),
            })
    }

    /// Keeps the model an agent's init line reports, so the chip names what actually ran. `live`: it arrived now, so
    /// the pick it ran under is the current one; a replayed one's pick is unknown.
    pub(in crate::workspace) fn note_model(
        &mut self,
        record: &crate::workspace::thread_model::Record,
        live: bool,
    ) {
        if let crate::workspace::thread_model::Record::Agent(skyelab_agent::Event::Init {
            model,
            ..
        }) = record
            && !model.is_empty()
        {
            self.composer.reported_model = Some(model.clone());
            self.composer.reported_pick = live.then(|| self.composer.pick.model.clone());
        }
    }

    /// A picked model's name. An alias pick (`opus`) names the version a run on that same pick reported.
    fn pick_name(&self, id: &str) -> String {
        let c = &self.composer;
        let ran_on_it = c.reported_pick.as_ref().and_then(Option::as_deref) == Some(id);
        match c.reported_model.as_deref() {
            Some(r)
                if ran_on_it
                    && CLAUDE_MODELS.iter().any(|(alias, _)| *alias == id)
                    && r.starts_with(&format!("claude-{id}-")) =>
            {
                model_label(r)
            }
            _ => label_for(id, self.listed()),
        }
    }

    /// The `@` menu's model rows: the picker's models for the thread's provider (Codex's once listed), none when
    /// the composer has no account.
    pub(in crate::workspace) fn mention_models(&self) -> Vec<super::composer_menu::MenuItem> {
        let Some(provider) = self.composer_account().map(|a| a.provider) else {
            return Vec::new();
        };
        let models: Vec<(String, String)> = match provider {
            Provider::Codex | Provider::Cursor => self
                .listed()
                .unwrap_or_default()
                .iter()
                .map(|m| (m.id.clone(), m.label.clone()))
                .collect(),
            Provider::Claude => CLAUDE_MODELS
                .iter()
                .map(|(id, label)| ((*id).to_owned(), (*label).to_owned()))
                .collect(),
        };
        super::composer_menu::model_items(&models, provider)
    }

    /// A `model:<provider>/<id>` mention's display name, as the `@` menu showed it.
    pub(in crate::workspace) fn mention_model_name(&self, provider: &str, id: &str) -> String {
        match provider {
            "claude" => CLAUDE_MODELS
                .iter()
                .find(|(alias, _)| *alias == id)
                .map_or_else(|| model_label(id), |(_, label)| (*label).to_owned()),
            _ => label_for(id, self.listed()),
        }
    }

    fn listed(&self) -> Option<&[ModelInfo]> {
        let list = match self.composer_account().map(|a| a.provider) {
            Some(Provider::Cursor) => self.composer.cursor_models.as_ref(),
            _ => self.composer.codex_models.as_ref(),
        };
        Some(list?.as_ref().ok()?.as_slice())
    }

    /// Reads the account's default model from its config dir (the model field only, off the UI thread). A Codex or
    /// Cursor account that sets none lists its models, whose default entry names it.
    pub(in crate::workspace) fn load_account_model(&mut self, cx: &mut Context<Self>) {
        let Some(account) = self.composer_account() else {
            return;
        };
        let provider = account.provider;
        let task =
            cx.background_spawn(async move { skyelab_agent::account_default_model(&account) });
        cx.spawn(async move |this, cx| {
            let model = task.await;
            let _ = this.update(cx, |this, cx| {
                if model.is_none() {
                    match provider {
                        Provider::Codex => this.list_codex_models(cx),
                        Provider::Cursor => this.list_cursor_models(cx),
                        Provider::Claude => {}
                    }
                }
                this.composer.account_model = model;
                cx.notify();
            });
        })
        .detach();
    }

    /// Lists a Cursor account's models once per tab (the picker and the chip both read them).
    pub(in crate::workspace) fn list_cursor_models(&mut self, cx: &mut Context<Self>) {
        let cursor = self
            .composer_account()
            .filter(|a| a.provider == Provider::Cursor);
        let (None, Some(account)) = (&self.composer.cursor_models, cursor) else {
            return;
        };
        self.composer.cursor_models = Some(Ok(Vec::new())); // loading
        let (bin, config, cwd) = (
            crate::workspace::github::cursor_bin(cx),
            self.config.clone(),
            self.cwd.clone(),
        );
        let task = cx.background_spawn(async move {
            skyelab_agent::list_cursor_models(&bin, &config, &account, &cwd)
                .map_err(|e| SharedString::from(format!("{e:#}")))
        });
        cx.spawn(async move |this, cx| {
            let listed = task.await;
            let _ = this.update(cx, |this, cx| {
                this.composer.cursor_models = Some(listed);
                this.highlight_current(cx);
                if this
                    .composer
                    .menu
                    .as_ref()
                    .is_some_and(|m| m.kind == MenuKind::Files)
                {
                    this.load_menu_items(MenuKind::Files, cx);
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Lists a Codex account's models once per tab (the picker and the chip both read them).
    pub(in crate::workspace) fn list_codex_models(&mut self, cx: &mut Context<Self>) {
        let codex = self
            .composer_account()
            .filter(|a| a.provider == Provider::Codex);
        let (None, Some(account)) = (&self.composer.codex_models, codex) else {
            return;
        };
        self.composer.codex_models = Some(Ok(Vec::new())); // loading
        let (bin, config, cwd) = (
            crate::workspace::github::codex_bin(cx),
            self.config.clone(),
            self.cwd.clone(),
        );
        let task = cx.background_spawn(async move {
            skyelab_agent::list_codex_models(&bin, &config, &account, &cwd)
                .map_err(|e| SharedString::from(format!("{e:#}")))
        });
        cx.spawn(async move |this, cx| {
            let listed = task.await;
            let _ = this.update(cx, |this, cx| {
                this.composer.codex_models = Some(listed);
                this.highlight_current(cx); // an open picker's rows just arrived
                if this
                    .composer
                    .menu
                    .as_ref()
                    .is_some_and(|m| m.kind == MenuKind::Files)
                {
                    this.load_menu_items(MenuKind::Files, cx); // the `@` menu's model rows
                }
                cx.notify();
            });
        })
        .detach();
    }
}

/// A listed model's own label, else its id as people say it.
fn label_for(id: &str, listed: Option<&[ModelInfo]>) -> String {
    listed
        .and_then(|l| l.iter().find(|m| m.id == id))
        .map_or_else(|| model_label(id), |m| m.label.clone())
}
