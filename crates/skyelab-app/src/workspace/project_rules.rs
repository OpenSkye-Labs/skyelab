//! "Allow for this project" (`skyelab_core::project_rules`): the dialog that shows the rule before it is saved, saving
//! it under the thread's project and the account its agent runs under, and answering later asks that match one.
//! SkyeLab answers them itself (allow once, by the rule), so the agent's own rule grammar never sees them.

use super::{ThreadView, lock};
use crate::ui::{self, Button, SegmentedControl, TextField};
use anyhow::Context as _;
use gpui::{
    AppContext as _, Context, Entity, IntoElement, Render, SharedString, WeakEntity, Window, div,
    prelude::*, px,
};
use gpui_component::{ActiveTheme as _, WindowExt as _, input::InputState, v_flex};
use serde_json::Value;
use skyelab_agent::Mode;
use skyelab_core::project_rules::{self as rules, Kind, Rule};

impl ThreadView {
    /// Whether this thread may save or use project rules: a local thread's own agent, not a side chat, never one fed
    /// GitHub text (a rule saved there would answer an injected ask).
    pub(in crate::workspace) fn takes_project_rules(&self) -> bool {
        self.thread.is_some() && !self.is_side_chat() && !self.untrusted
    }

    /// An ask just arrived: when one of the project's rules for this account covers it, allow it once by that rule.
    /// Never in Plan mode (it plans, it doesn't run) or a side chat.
    pub(in crate::workspace) fn auto_allow(
        &mut self,
        id: &str,
        tool: &str,
        input: &Value,
        cx: &mut Context<Self>,
    ) {
        if !self.takes_project_rules() || self.mode == Mode::Plan || !rules::offers(tool) {
            return;
        }
        let Ok(account) = self.send_account() else {
            return; // rebound or unbound: the ask stays for the user, and answering it ends the agent
        };
        let (Some(t), Some(session)) = (&self.thread, self.session.clone()) else {
            return;
        };
        let (project, store, root) = (t.project_id, self.backend.store.clone(), self.cwd.clone());
        let (id, tool, input) = (id.to_owned(), tool.to_owned(), input.clone());
        self.in_background(cx, move || {
            let saved = lock(&store)?.project_rules(project, &account.id)?;
            let saved: Vec<Rule> = saved.into_iter().map(|r| r.rule).collect();
            if let Some(rule) = rules::first_match(&saved, &tool, &input, &root) {
                // Fails only when the user (or the agent) settled the ask first; then there is nothing to say.
                let _ = lock(&session)?.answer_by_rule(&id, rules::label(rule));
            }
            Ok(())
        });
    }

    /// The pending ask `id`'s tool and input.
    fn ask(&self, id: &str) -> Option<(String, Value)> {
        self.items.iter().find_map(|i| match i {
            super::thread_model::Item::Approval(a) if a.id == id && a.decision.is_none() => {
                Some((a.tool.clone(), a.raw.clone()))
            }
            _ => None,
        })
    }

    /// Saves `rule` for this project under the agent's account, then allows ask `id` by it. Refused, saving nothing,
    /// when the rule isn't valid or doesn't cover that ask. `done` gets the outcome off the UI thread.
    pub(in crate::workspace) fn allow_by_new_rule(
        &mut self,
        id: &str,
        rule: Rule,
        done: impl FnOnce(Result<(), String>) + Send + 'static,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        if !self.takes_project_rules() {
            return Err(
                "Project rules are saved from the thread's own window, not a side chat.".into(),
            );
        }
        let (tool, input) = self.ask(id).ok_or("The agent no longer waits for this.")?;
        rules::validate(&rule)?;
        if rule.tool != tool || !rules::matches(&rule, &tool, &input, &self.cwd) {
            return Err("This rule doesn't cover the call waiting now. Change it so it does, or allow once.".into());
        }
        let account = self.send_account().map_err(|e| e.to_string())?;
        let session = self.answer_session(cx)?;
        let project = self
            .thread
            .as_ref()
            .map(|t| t.project_id)
            .ok_or("no thread")?;
        let (store, id) = (self.backend.store.clone(), id.to_owned());
        self.in_background(cx, move || {
            // Allowed first: an ask that is gone by now saves no rule the user was told failed.
            let r = lock(&session)
                .and_then(|mut s| s.answer_by_rule(&id, rules::label(&rule)))
                .and_then(|()| {
                    lock(&store)?
                        .add_project_rule(project, &account.id, &rule)
                        .context("Allowed once, but the rule couldn't be saved")
                });
            done(r.as_ref().map(|_| ()).map_err(|e| format!("{e:#}")));
            r
        });
        Ok(())
    }

    /// Opens the dialog that shows the rule "Allow for this project" would save for ask `id`.
    pub(in crate::workspace) fn open_rule_dialog(
        &mut self,
        id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some((tool, input)) = self.ask(id) else {
            return;
        };
        let Some(proposed) = rules::propose(&tool, &input, &self.cwd) else {
            self.show_error(
                anyhow::anyhow!(
                    "No project rule can cover this call: its file is outside the project or in .git or agent \
                     settings, or the command asks to leave the sandbox. Allow it once instead."
                ),
                cx,
            );
            return;
        };
        let account = self.send_account().map(|a| a.id).unwrap_or_default();
        let place = format!("{} under {account}", self.project_name);
        let thread = cx.entity().downgrade();
        let dialog =
            cx.new(|cx| RuleDialog::new(thread, id.into(), input, proposed, place, window, cx));
        open(dialog, window, cx);
    }
}

/// The rule being made: the agent's call, the rule shown, and the field that edits its pattern.
pub(in crate::workspace) struct RuleDialog {
    thread: WeakEntity<ThreadView>,
    ask: String,
    input: Value,
    proposed: Rule,
    /// Exact or Prefix for a command; the proposed kind otherwise.
    pub(in crate::workspace) kind: Kind,
    /// The prefix (a command) or the glob (a file) the user edits.
    pub(in crate::workspace) field: Entity<InputState>,
    /// The project and account it is saved under, in words.
    place: String,
    error: Option<SharedString>,
    busy: bool,
}

impl RuleDialog {
    fn new(
        thread: WeakEntity<ThreadView>,
        ask: String,
        input: Value,
        proposed: Rule,
        place: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let start = match proposed.kind {
            Kind::Exact => input["command"]
                .as_str()
                .and_then(rules::suggested_prefix)
                .unwrap_or_default(),
            _ => proposed.pattern.clone(),
        };
        let field = cx.new(|cx| InputState::new(window, cx));
        field.update(cx, |f, cx| f.set_value(start, window, cx));
        Self {
            thread,
            ask,
            input,
            kind: proposed.kind,
            proposed,
            field,
            place,
            error: None,
            busy: false,
        }
    }

    /// What Save would store now.
    pub(in crate::workspace) fn rule(&self, cx: &gpui::App) -> Rule {
        let pattern = match self.kind {
            Kind::Exact | Kind::Tool => self.proposed.pattern.clone(),
            Kind::Prefix | Kind::Path => self.field.read(cx).value().trim().to_owned(),
        };
        Rule {
            tool: self.proposed.tool.clone(),
            kind: self.kind,
            pattern,
        }
    }

    /// Whether the command may take a prefix rule at all (one plain command).
    fn prefix_ok(&self) -> bool {
        self.input["command"]
            .as_str()
            .and_then(rules::suggested_prefix)
            .is_some()
    }

    pub(in crate::workspace) fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        let rule = self.rule(cx);
        let (tx, rx) = flume::bounded(1);
        let sent = self.thread.update(cx, |t, cx| {
            t.allow_by_new_rule(&self.ask, rule, move |r| drop(tx.send(r)), cx)
        });
        match sent {
            Ok(Ok(())) => self.busy = true,
            Ok(Err(why)) => self.error = Some(why.into()),
            Err(_) => self.error = Some("The thread is closed.".into()),
        }
        cx.notify();
        if !self.busy {
            return;
        }
        cx.spawn_in(window, async move |this, cx| {
            let r = rx
                .recv_async()
                .await
                .unwrap_or_else(|_| Err("cancelled".into()));
            let _ = this.update_in(cx, |this, window, cx| {
                this.busy = false;
                match r {
                    Ok(()) => window.close_dialog(cx),
                    Err(e) => this.error = Some(e.into()),
                }
                cx.notify();
            });
        })
        .detach();
    }
}

fn open(dialog: Entity<RuleDialog>, window: &mut Window, cx: &mut gpui::App) {
    crate::ui::open_dialog(window, cx, move |d, _, _| {
        let save = dialog.clone();
        d.w(px(500.))
            .title("Allow for this project")
            .child(dialog.clone())
            .footer(
                ui::dialog_footer()
                    .child(ui::dialog_cancel("project-rule-cancel"))
                    .child(
                        Button::new("project-rule-save")
                            .primary()
                            .small()
                            .label("Save rule and allow")
                            .on_click(move |_, window, cx| {
                                save.update(cx, |d, cx| d.save(window, cx))
                            }),
                    ),
            )
    });
}

impl Render for RuleDialog {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let rule = self.rule(cx);
        let theme = cx.theme();
        let mono = theme.mono_font_family.clone();
        let muted = theme.muted_foreground;
        let code = |text: String| {
            div()
                .px_2()
                .py_1()
                .rounded(ui::RADIUS)
                .bg(theme.muted)
                .font_family(mono.clone())
                .text_size(ui::META)
                .child(text)
        };
        let body = match self.proposed.kind {
            Kind::Exact => {
                let pick = SegmentedControl::new("project-rule-kind")
                    .item("This exact command")
                    .when(self.prefix_ok(), |s| s.item("Commands starting with"))
                    .selected(Some(usize::from(self.kind == Kind::Prefix)))
                    .on_click(cx.listener(|this, ix: &usize, _, cx| {
                        this.kind = if *ix == 1 { Kind::Prefix } else { Kind::Exact };
                        this.error = None;
                        cx.notify();
                    }));
                let shown = if self.kind == Kind::Prefix {
                    TextField::new(&self.field).small().into_any_element()
                } else {
                    code(self.proposed.pattern.clone()).into_any_element()
                };
                let note = if !self.prefix_ok() {
                    "This command chains, pipes, substitutes or redirects, so it can only be allowed exactly."
                } else if self.kind == Kind::Prefix {
                    "Any one plain command that starts with these words, with any arguments after them. Never one \
                     that chains (; && ||), pipes, substitutes or redirects."
                } else {
                    "Only this command, character for character."
                };
                v_flex()
                    .gap_2()
                    .child(gpui_component::h_flex().child(pick))
                    .child(shown)
                    .child(div().text_size(ui::META).text_color(muted).child(note))
            }
            Kind::Path => v_flex()
                .gap_2()
                .child(
                    div()
                        .text_size(ui::META)
                        .child(format!("{} on files matching, inside the project:", rule.tool)),
                )
                .child(TextField::new(&self.field).small())
                .child(div().text_size(ui::META).text_color(muted).child(
                    "Relative to the thread's folder. * matches within one folder, ** across folders \
                     (src/** is everything under src).",
                )),
            Kind::Prefix | Kind::Tool => v_flex()
                .child(div().text_size(ui::META).child(format!("Every call to {}.", rule.tool))),
        };
        v_flex()
            .debug_selector(|| "project-rule-dialog".into())
            .gap_3()
            .text_size(ui::BODY)
            .child(
                div().text_size(ui::META).text_color(muted).child(format!(
                    "Saved in SkyeLab for {}: later asks there that match are allowed without asking. Never \
                     written to the repo or to the account's settings. Edit or remove it in Settings → Chat.",
                    self.place
                )),
            )
            .child(body)
            .child(
                v_flex()
                    .gap_1()
                    .child(div().text_size(ui::META).text_color(muted).child("Rule"))
                    .child(code(rules::label(&rule))),
            )
            .when_some(self.error.clone(), |d, e| {
                d.child(div().text_size(ui::META).text_color(theme.danger).child(e))
            })
    }
}
