//! Settings → Chat's Codex model: a menu of the models Codex lists (the composer's `model/list`, or Settings' own once
//! per session through the first folder bound to a Codex account), the saved id when it isn't one of them, and
//! "Custom model id…", which shows a text field for any other id.

use super::Settings;
use super::project_scope::Pick;
use crate::ui::TextField;
use crate::ui::h_flex;
use gpui::{AnyElement, App, Context, Global, div, prelude::*};
use skyelab_agent::ModelInfo;
use skyelab_core::Provider;
use skyelab_core::thread_defaults::ThreadField;
use std::rc::Rc;

#[cfg(all(test, unix))] // the fake codex is a shell script
#[path = "codex_models_test.rs"]
mod tests;

/// The Codex models listed this session. Empty: asked, nothing listed (yet).
pub(crate) struct KnownCodexModels(pub Vec<ModelInfo>);
impl Global for KnownCodexModels {}

impl KnownCodexModels {
    /// A composer (or Settings) listed `models`: Settings offers them from now on.
    pub(crate) fn remember(models: &[ModelInfo], cx: &mut App) {
        if !models.is_empty() {
            cx.set_global(Self(models.to_vec()));
        }
    }
}

/// The menu's "Custom model id…" entry: never a model name (`valid_name` refuses it), so never saved.
const CUSTOM: &str = "\u{1}custom";

impl Settings {
    /// Lists Codex's models once per session when none are known, as the first Codex-bound folder's account.
    pub(super) fn discover_codex_models(&mut self, cx: &mut Context<Self>) {
        if cx.has_global::<KnownCodexModels>() {
            return;
        }
        let found = self.config.bindings.iter().find_map(|b| {
            let account = self.config.resolve(&b.path)?;
            (account.provider == Provider::Codex && b.path.is_dir())
                .then(|| (account.clone(), b.path.clone()))
        });
        let Some((account, dir)) = found else {
            return;
        };
        cx.set_global(KnownCodexModels(Vec::new()));
        let (bin, config) = (crate::workspace::github::codex_bin(cx), self.config.clone());
        let task = cx.background_spawn(async move {
            skyelab_agent::list_codex_models(&bin, &config, &account, &dir)
        });
        cx.spawn(async move |this, cx| {
            if let Ok(models) = task.await {
                cx.update(|cx| KnownCodexModels::remember(&models, cx));
                let _ = this.update(cx, |_, cx| cx.notify());
            }
        })
        .detach();
    }

    /// The Codex model menu's entries: Default, the listed models, the saved id when not listed, and Custom.
    pub(super) fn codex_options(&self, cx: &App) -> Vec<(Option<String>, String)> {
        let known = cx
            .try_global::<KnownCodexModels>()
            .map(|k| k.0.clone())
            .unwrap_or_default();
        let mut options: Vec<(Option<String>, String)> =
            std::iter::once((None, "Default model".to_owned()))
                .chain(known.into_iter().map(|m| (Some(m.id), m.label)))
                .collect();
        if let Some(saved) = self.shown_field(ThreadField::CodexModel, cx)
            && !options.iter().any(|(v, _)| v.as_ref() == Some(&saved))
        {
            options.push((Some(saved.clone()), format!("{saved} (custom)")));
        }
        options.push((Some(CUSTOM.to_owned()), "Custom model id…".to_owned()));
        options
    }

    /// A Codex menu entry was picked: Custom shows the text field and saves nothing; the rest save.
    pub(super) fn pick_codex_model(&mut self, v: Option<String>, cx: &mut Context<Self>) {
        self.codex_custom = v.as_deref() == Some(CUSTOM);
        if !self.codex_custom {
            self.set_shown_field(ThreadField::CodexModel, v, cx);
        }
        cx.notify();
    }

    /// The Codex model menu, and the text field after "Custom model id…".
    pub(super) fn codex_model_control(&self, cx: &Context<Self>) -> AnyElement {
        let options = self.codex_options(cx);
        let pick: Pick = Rc::new(|this, v, cx| this.pick_codex_model(v, cx));
        h_flex()
            .gap_2()
            .child(self.field_choice_with(
                "threads-codex-model",
                ThreadField::CodexModel,
                options,
                pick,
                cx,
            ))
            .when(self.codex_custom, |d| {
                d.child(
                    div().w(gpui::px(160.)).child(
                        TextField::new(&self.codex_model_input)
                            .small()
                            .accessibility_label("Custom Codex model id"),
                    ),
                )
            })
            .into_any_element()
    }
}
