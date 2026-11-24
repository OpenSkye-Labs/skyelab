//! Settings → Chat with a project picked: that project's "Allow for this project" rules under the account its folder
//! is bound to now. Each can have its pattern edited (checked like a new rule) or be removed. Rules saved under
//! another account stay in the store but are neither listed nor used.

use super::{SharedStore, lock};
use crate::ui::{self, Button, TextField};
use gpui::{
    AppContext as _, Context, Entity, IntoElement, Render, SharedString, Window, div, prelude::*,
};
use gpui_component::{ActiveTheme as _, h_flex, input::InputState, v_flex};
use skyelab_core::project_rules::{self as rules, Kind};
use skyelab_store::project_rules::ProjectRule;
use std::path::{Path, PathBuf};

pub struct ProjectRulesPage {
    store: SharedStore,
    /// The project folder shown and the account it is bound to (`None`: unbound, so no rules apply).
    dir: Option<PathBuf>,
    pub(super) account: Option<String>,
    project: Option<i64>,
    pub(super) rules: Vec<ProjectRule>,
    /// The rule being edited and its field.
    pub(super) editing: Option<(i64, Entity<InputState>)>,
    pub(super) error: Option<SharedString>,
}

impl ProjectRulesPage {
    pub fn new(store: SharedStore, _: &mut Context<Self>) -> Self {
        Self {
            store,
            dir: None,
            account: None,
            project: None,
            rules: Vec::new(),
            editing: None,
            error: None,
        }
    }

    /// Shows folder `dir`'s rules under `account` (the folder's binding now).
    pub fn show(&mut self, dir: Option<&Path>, account: Option<String>, cx: &mut Context<Self>) {
        self.dir = dir.map(Path::to_path_buf);
        self.account = account;
        self.editing = None;
        self.error = None;
        self.reload(cx);
    }

    // shortcut: reads and writes the store on the UI thread (a few rows by key); move to background_spawn if a
    // long store job ever makes this page stall.
    fn reload(&mut self, cx: &mut Context<Self>) {
        let read = || -> anyhow::Result<(Option<i64>, Vec<ProjectRule>)> {
            let (Some(dir), Some(account)) = (&self.dir, &self.account) else {
                return Ok((None, Vec::new()));
            };
            let store = lock(&self.store)?;
            let project = store.list_projects()?.into_iter().find(|p| &p.path == dir);
            match project {
                Some(p) => Ok((Some(p.id), store.project_rules(p.id, account)?)),
                None => Ok((None, Vec::new())),
            }
        };
        match read() {
            Ok((project, list)) => (self.project, self.rules) = (project, list),
            Err(e) => self.error = Some(format!("{e:#}").into()),
        }
        cx.notify();
    }

    pub(super) fn edit(&mut self, id: i64, window: &mut Window, cx: &mut Context<Self>) {
        let Some(r) = self.rules.iter().find(|r| r.id == id) else {
            return;
        };
        let field = cx.new(|cx| InputState::new(window, cx));
        let pattern = r.rule.pattern.clone();
        field.update(cx, |f, cx| f.set_value(pattern, window, cx));
        self.editing = Some((id, field));
        self.error = None;
        cx.notify();
    }

    /// Saves the edited pattern when it makes a valid rule of the same tool and kind.
    pub(super) fn save_edit(&mut self, cx: &mut Context<Self>) {
        let Some((id, field)) = self.editing.clone() else {
            return;
        };
        let (Some(account), Some(old)) = (&self.account, self.rules.iter().find(|r| r.id == id))
        else {
            return;
        };
        let rule = rules::Rule {
            pattern: field.read(cx).value().trim().to_owned(),
            ..old.rule.clone()
        };
        let saved = rules::validate(&rule)
            .map_err(anyhow::Error::msg)
            .and_then(|()| lock(&self.store)?.set_project_rule_pattern(id, account, &rule.pattern));
        match saved {
            Ok(()) => {
                self.editing = None;
                self.error = None;
                self.reload(cx);
            }
            Err(e) => {
                self.error = Some(format!("{e:#}").into());
                cx.notify();
            }
        }
    }

    pub(super) fn remove(&mut self, id: i64, cx: &mut Context<Self>) {
        let removed = lock(&self.store).and_then(|s| s.delete_project_rule(id));
        if let Err(e) = removed {
            self.error = Some(format!("{e:#}").into());
        }
        self.editing = self.editing.take().filter(|(e, _)| *e != id);
        self.reload(cx);
    }
}

impl Render for ProjectRulesPage {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let muted = theme.muted_foreground;
        let note = |text: String| {
            ui::group_row(true, theme)
                .text_color(muted)
                .child(text)
                .into_any_element()
        };
        let mut rows: Vec<gpui::AnyElement> = Vec::new();
        match (&self.account, self.project) {
            (None, _) => rows.push(note(
                "This folder isn't bound to an account, so no rules apply.".into(),
            )),
            (Some(a), _) if self.rules.is_empty() => rows.push(note(format!(
                "No rules for {a} yet. Choose Allow for this project on an approval to add one."
            ))),
            _ => {}
        }
        for (ix, r) in self.rules.iter().enumerate() {
            let id = r.id;
            let editing = self.editing.as_ref().filter(|(e, _)| *e == id);
            let row = ui::group_row(ix == 0, theme).debug_selector(|| "project-rule-row".into());
            let row = match editing {
                Some((_, field)) => row
                    .child(div().flex_shrink_0().child(format!("{} ·", r.rule.tool)))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .child(TextField::new(field).small()),
                    )
                    .child(
                        Button::new(("rule-save", id as u64))
                            .small()
                            .primary()
                            .label("Save")
                            .on_click(cx.listener(|this, _, _, cx| this.save_edit(cx))),
                    )
                    .child(
                        Button::new(("rule-cancel", id as u64))
                            .small()
                            .ghost()
                            .label("Cancel")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.editing = None;
                                this.error = None;
                                cx.notify();
                            })),
                    ),
                None => row
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .font_family(theme.mono_font_family.clone())
                            .text_size(ui::META)
                            .child(rules::label(&r.rule)),
                    )
                    .when(r.rule.kind != Kind::Tool, |d| {
                        d.child(
                            Button::new(("rule-edit", id as u64))
                                .small()
                                .ghost()
                                .label("Edit")
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.edit(id, window, cx)
                                })),
                        )
                    })
                    .child(
                        Button::new(("rule-remove", id as u64))
                            .small()
                            .ghost()
                            .label("Remove")
                            .on_click(cx.listener(move |this, _, _, cx| this.remove(id, cx))),
                    ),
            };
            rows.push(row.into_any_element());
        }
        v_flex()
            .debug_selector(|| "project-rules".into())
            .children(rows)
            .when_some(self.error.clone(), |d, e| {
                d.child(
                    h_flex()
                        .px_4()
                        .py_2()
                        .text_size(ui::META)
                        .text_color(theme.danger)
                        .child(e),
                )
            })
    }
}
