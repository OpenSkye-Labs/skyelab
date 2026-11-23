//! What the composer's model chip names: the model that will run, never a bare "default" (T3's "Opus 5.5 High").

use super::composer_picker::title_case;
use crate::workspace::thread::ThreadView;
use gpui::{AppContext as _, Context, SharedString};
use skyelab_agent::{CLAUDE_MODELS, ModelInfo, model_label};
use skyelab_core::Provider;

impl ThreadView {
    /// The chip's label ("Opus 5.5 High") and whether it names the account's default rather than a pick. `compact`
    /// leaves the effort out.
    pub(in crate::workspace) fn chip_label(&self, compact: bool) -> (String, bool) {
        let c = &self.composer;
        let (model, default) = match c.pick.model.as_deref() {
            Some(id) => (
                pick_name(id, c.reported_model.as_deref(), self.listed()),
                false,
            ),
            None => (self.default_model_name(), true),
        };
        match &c.pick.effort {
            Some(e) if !compact => (format!("{model} {}", title_case(e)), default),
            _ => (model, default),
        }
    }

    /// The model the account runs with no pick: what this thread's last run reported, else the account's own config,
    /// else Codex's listed default, else "Claude default" / "Codex default".
    pub(in crate::workspace) fn default_model_name(&self) -> String {
        let c = &self.composer;
        let listed = self.listed();
        c.reported_model
            .as_deref()
            .or(c.account_model.as_deref())
            .map(|id| label_for(id, listed))
            .or_else(|| {
                listed?
                    .iter()
                    .find(|m| m.is_default)
                    .map(|m| m.label.clone())
            })
            .unwrap_or_else(|| match self.composer_account().map(|a| a.provider) {
                Some(Provider::Codex) => "Codex default".into(),
                _ => "Claude default".into(),
            })
    }

    /// Keeps the model an agent's init line reports, so the chip names what actually ran.
    pub(in crate::workspace) fn note_model(
        &mut self,
        record: &crate::workspace::thread_model::Record,
    ) {
        if let crate::workspace::thread_model::Record::Agent(skyelab_agent::Event::Init {
            model,
            ..
        }) = record
            && !model.is_empty()
        {
            self.composer.reported_model = Some(model.clone());
        }
    }

    fn listed(&self) -> Option<&[ModelInfo]> {
        Some(
            self.composer
                .codex_models
                .as_ref()?
                .as_ref()
                .ok()?
                .as_slice(),
        )
    }

    /// Reads the account's default model from its config dir (the model field only, off the UI thread). A Codex
    /// account that sets none lists its models, whose default entry names it.
    pub(in crate::workspace) fn load_account_model(&mut self, cx: &mut Context<Self>) {
        let Some(account) = self.composer_account() else {
            return;
        };
        let codex = account.provider == Provider::Codex;
        let task =
            cx.background_spawn(async move { skyelab_agent::account_default_model(&account) });
        cx.spawn(async move |this, cx| {
            let model = task.await;
            let _ = this.update(cx, |this, cx| {
                if model.is_none() && codex {
                    this.list_codex_models(cx);
                }
                this.composer.account_model = model;
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

/// A picked model's name. An alias pick (`opus`) names the version this thread's last run reported for that family.
fn pick_name(id: &str, reported: Option<&str>, listed: Option<&[ModelInfo]>) -> String {
    match reported {
        Some(r)
            if CLAUDE_MODELS.iter().any(|(alias, _)| *alias == id)
                && r.starts_with(&format!("claude-{id}-")) =>
        {
            model_label(r)
        }
        _ => label_for(id, listed),
    }
}
