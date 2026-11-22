//! Accounts and folder-bindings settings. Thin: every rule lives in `skyelab_core::AccountsConfig`.

use anyhow::{Context as _, Result};
use gpui::{
    Action, App, Context, Entity, FocusHandle, Focusable, KeyBinding, KeyBindingContextPredicate,
    PathPromptOptions, SharedString, Subscription, Task, Window, actions, div, prelude::*,
};
use gpui_component::{
    ActiveTheme as _, IndexPath, Sizable as _,
    button::{Button, ButtonVariants as _},
    input::{Input, InputEvent, InputState},
    select::{Select, SelectState},
};
use skyelab_core::{Account, AccountsConfig, Binding};
use std::path::{Path, PathBuf};
use std::rc::Rc;

actions!(skyelab, [SaveAccount, CancelEdit, BindFolder]);

/// Fill the form with this account for editing.
#[derive(Clone, PartialEq, Action)]
#[action(namespace = skyelab, no_json)]
pub struct EditAccount {
    pub id: String,
}

#[derive(Clone, PartialEq, Action)]
#[action(namespace = skyelab, no_json)]
pub struct RemoveAccount {
    pub id: String,
}

#[derive(Clone, PartialEq, Action)]
#[action(namespace = skyelab, no_json)]
pub struct RemoveBinding {
    pub path: PathBuf,
}

const CONTEXT: &str = "Settings";

pub fn init(cx: &mut App) {
    // `KeyBinding::new` can't parse `secondary-` (it doesn't know the OS); `load` resolves it.
    let context = KeyBindingContextPredicate::parse(CONTEXT).ok().map(Rc::new);
    let bindings: [(&str, Box<dyn Action>); 3] = [
        ("secondary-enter", Box::new(SaveAccount)),
        ("escape", Box::new(CancelEdit)),
        ("secondary-b", Box::new(BindFolder)),
    ];
    let bindings: Vec<_> = bindings
        .into_iter()
        .map(|(keys, action)| {
            KeyBinding::load(keys, action, context.clone(), false, None, cx)
                .expect("static keystrokes are valid")
        })
        .collect();
    cx.bind_keys(bindings);
}

type AccountSelect = SelectState<Vec<SharedString>>;

pub struct Settings {
    path: Option<PathBuf>,
    config: AccountsConfig,
    error: Option<String>,
    /// Why saving is off. Set when the file failed to load, so an edit never overwrites it.
    locked: Option<String>,
    /// Id of the account the form is editing; `None` means the form adds a new one.
    editing: Option<String>,
    id_input: Entity<InputState>,
    label_input: Entity<InputState>,
    dir_input: Entity<InputState>,
    test_input: Entity<InputState>,
    /// Cached so render does no filesystem work (`resolve` canonicalizes).
    test_result: Option<Option<Account>>,
    account_select: Entity<AccountSelect>,
    focus: FocusHandle,
    _pick: Option<Task<()>>,
    _subs: Vec<Subscription>,
}

impl Settings {
    pub fn new(
        path: Option<PathBuf>,
        config: AccountsConfig,
        error: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let input = |placeholder: &str, window: &mut Window, cx: &mut Context<Self>| {
            let placeholder = placeholder.to_string();
            cx.new(|cx| InputState::new(window, cx).placeholder(placeholder))
        };
        let id_input = input("id, e.g. acme", window, cx);
        let label_input = input("label, e.g. Acme (Team)", window, cx);
        let dir_input = input(
            "absolute config dir, e.g. /Users/me/.claude-acme",
            window,
            cx,
        );
        let test_input = input("/path/to/a/folder", window, cx);
        let ids = account_ids(&config);
        let selected = (!ids.is_empty()).then(|| IndexPath::new(0));
        let account_select = cx.new(|cx| SelectState::new(ids, selected, window, cx));
        let subs = vec![cx.subscribe(&test_input, |this, _, e: &InputEvent, cx| {
            if matches!(e, InputEvent::Change) {
                this.refresh_test(cx);
                cx.notify();
            }
        })];
        let locked = match (&path, error) {
            (None, _) => Some("no config directory on this OS, so nothing can be saved".into()),
            (Some(p), Some(e)) => Some(format!(
                "{e}. Editing is off so {} isn't overwritten: fix the file, then restart.",
                p.display()
            )),
            (Some(_), None) => None,
        };
        Self {
            path,
            config,
            error: locked.clone(),
            locked,
            editing: None,
            id_input,
            label_input,
            dir_input,
            test_input,
            test_result: None,
            account_select,
            focus: cx.focus_handle(),
            _pick: None,
            _subs: subs,
        }
    }

    /// Run one core edit on a copy, save it, and only then adopt it. A failed edit or save
    /// leaves memory and disk unchanged, and shows the core's error inline.
    fn apply(
        &mut self,
        edit: impl FnOnce(&mut AccountsConfig) -> Result<()>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if let Some(why) = &self.locked {
            self.error = Some(why.clone());
            cx.notify();
            return false;
        }
        let mut next = self.config.clone();
        let result = edit(&mut next).and_then(|()| {
            let path = self
                .path
                .as_deref()
                .context("no config directory on this OS")?;
            next.save(path)
        });
        let ok = match result {
            Ok(()) => {
                self.config = next;
                self.error = None;
                let ids = account_ids(&self.config);
                self.account_select.update(cx, |s, cx| {
                    let keep = s.selected_value().cloned();
                    s.set_items(ids.clone(), window, cx);
                    if let Some(id) = keep.filter(|id| ids.contains(id)).or(ids.first().cloned()) {
                        s.set_selected_value(&id, window, cx);
                    }
                });
                self.refresh_test(cx);
                true
            }
            Err(e) => {
                self.error = Some(format!("{e:#}"));
                false
            }
        };
        cx.notify();
        ok
    }

    fn refresh_test(&mut self, cx: &App) {
        let text = self.test_input.read(cx).value();
        let text = text.trim();
        self.test_result =
            (!text.is_empty()).then(|| self.config.resolve(Path::new(text)).cloned());
    }

    fn save_account(&mut self, _: &SaveAccount, window: &mut Window, cx: &mut Context<Self>) {
        let value = |input: &Entity<InputState>| input.read(cx).value().trim().to_string();
        let account = Account {
            id: self
                .editing
                .clone()
                .unwrap_or_else(|| value(&self.id_input)),
            label: value(&self.label_input),
            config_dir: PathBuf::from(value(&self.dir_input)),
        };
        let adding = self.editing.is_none();
        let saved = self.apply(
            |c| {
                if adding {
                    c.add_account(account)
                } else {
                    c.update_account(account)
                }
            },
            window,
            cx,
        );
        if saved {
            self.cancel_edit(&CancelEdit, window, cx);
        }
    }

    fn cancel_edit(&mut self, _: &CancelEdit, window: &mut Window, cx: &mut Context<Self>) {
        self.editing = None;
        for input in [&self.id_input, &self.label_input, &self.dir_input] {
            input.update(cx, |s, cx| s.set_value("", window, cx));
        }
        cx.notify();
    }

    fn edit_account(&mut self, action: &EditAccount, window: &mut Window, cx: &mut Context<Self>) {
        let Some(account) = self
            .config
            .accounts
            .iter()
            .find(|a| a.id == action.id)
            .cloned()
        else {
            return;
        };
        let account = &account;
        self.editing = Some(account.id.clone());
        let dir = account.config_dir.display().to_string();
        for (input, text) in [
            (&self.id_input, account.id.clone()),
            (&self.label_input, account.label.clone()),
            (&self.dir_input, dir),
        ] {
            input.update(cx, |s, cx| s.set_value(text, window, cx));
        }
        cx.notify();
    }

    fn remove_account(&mut self, a: &RemoveAccount, window: &mut Window, cx: &mut Context<Self>) {
        self.apply(|c| c.remove_account(&a.id), window, cx);
    }

    fn remove_binding(&mut self, a: &RemoveBinding, window: &mut Window, cx: &mut Context<Self>) {
        self.apply(|c| c.remove_binding(&a.path), window, cx);
    }

    fn bind_folder(&mut self, _: &BindFolder, window: &mut Window, cx: &mut Context<Self>) {
        let Some(account) = self.account_select.read(cx).selected_value().cloned() else {
            self.error = Some("add an account first, then pick it to bind a folder".into());
            cx.notify();
            return;
        };
        let picked = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Bind".into()),
        });
        self._pick = Some(cx.spawn_in(window, async move |this, cx| {
            let path = match picked.await {
                Ok(Ok(Some(mut paths))) if !paths.is_empty() => Ok(paths.swap_remove(0)),
                Ok(Ok(_)) | Err(_) => return, // cancelled, or the dialog went away
                Ok(Err(e)) => Err(e),
            };
            // The view may be gone by now; nothing to do then.
            let _ = this.update_in(cx, |this, window, cx| match path {
                Ok(path) => {
                    this.apply(
                        |c| {
                            c.add_binding(Binding {
                                path,
                                account: account.to_string(),
                            })
                        },
                        window,
                        cx,
                    );
                }
                Err(e) => {
                    this.error = Some(format!("folder picker failed: {e:#}"));
                    cx.notify();
                }
            });
        }));
    }

    fn render_accounts(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let form_title = match &self.editing {
            Some(id) => format!("Edit account '{id}'"),
            None => "Add account".into(),
        };
        section("Accounts", cx)
            .children(self.config.accounts.iter().map(|a| {
                let edit = EditAccount { id: a.id.clone() };
                let remove = RemoveAccount { id: a.id.clone() };
                row(cx)
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .child(div().truncate().child(format!("{}  ({})", a.label, a.id)))
                            .child(
                                path_text(format!("dir-{}", a.id), &a.config_dir)
                                    .text_sm()
                                    .text_color(theme.muted_foreground),
                            ),
                    )
                    .child(
                        Button::new(SharedString::from(format!("edit-{}", a.id)))
                            .label("Edit")
                            .flex_shrink_0()
                            .small()
                            .ghost()
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.edit_account(&edit, window, cx)
                            })),
                    )
                    .child(
                        Button::new(SharedString::from(format!("remove-account-{}", a.id)))
                            .label("Remove")
                            .flex_shrink_0()
                            .small()
                            .danger()
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.remove_account(&remove, window, cx)
                            })),
                    )
            }))
            .child(div().pt_2().text_sm().child(form_title))
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap_2()
                    .when(self.editing.is_none(), |d| {
                        d.child(div().w_40().child(Input::new(&self.id_input)))
                    })
                    .child(div().w_56().child(Input::new(&self.label_input)))
                    .child(div().flex_1().min_w_64().child(Input::new(&self.dir_input)))
                    .child(
                        Button::new("save-account")
                            .label(if self.editing.is_some() {
                                "Update"
                            } else {
                                "Add"
                            })
                            .primary()
                            .flex_shrink_0()
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.save_account(&SaveAccount, window, cx)
                            })),
                    )
                    .when(self.editing.is_some(), |d| {
                        d.child(
                            Button::new("cancel-edit")
                                .label("Cancel")
                                .ghost()
                                .flex_shrink_0()
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.cancel_edit(&CancelEdit, window, cx)
                                })),
                        )
                    }),
            )
    }

    fn render_bindings(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let label_of = |id: &str| {
            self.config
                .accounts
                .iter()
                .find(|a| a.id == id)
                .map_or(id.to_string(), |a| a.label.clone())
        };
        section("Folder bindings", cx)
            .when(self.config.bindings.is_empty(), |d| {
                d.child(
                    div().text_sm().text_color(theme.muted_foreground).child(
                        "No folders bound yet. Agents won't start anywhere until you bind one.",
                    ),
                )
            })
            .children(self.config.bindings.iter().map(|b| {
                let remove = RemoveBinding {
                    path: b.path.clone(),
                };
                row(cx)
                    .child(path_text(format!("path-{}", b.path.display()), &b.path).flex_1())
                    .child(
                        div()
                            .flex_shrink_0()
                            .max_w_48()
                            .truncate()
                            .text_color(theme.muted_foreground)
                            .child(format!("→  {}", label_of(&b.account))),
                    )
                    .child(
                        Button::new(SharedString::from(format!("unbind-{}", b.path.display())))
                            .label("Remove")
                            .flex_shrink_0()
                            .small()
                            .danger()
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.remove_binding(&remove, window, cx)
                            })),
                    )
            }))
            .child(
                div()
                    .pt_2()
                    .flex()
                    .gap_2()
                    .items_center()
                    .child(
                        div().w_56().child(
                            Select::new(&self.account_select)
                                .id("binding-account")
                                .placeholder("Account")
                                .accessibility_label("Account for new binding"),
                        ),
                    )
                    .child(
                        Button::new("bind-folder")
                            .label("Bind folder…")
                            .primary()
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.bind_folder(&BindFolder, window, cx)
                            })),
                    ),
            )
    }

    fn render_test(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let result = match &self.test_result {
            None => None,
            Some(Some(a)) => Some(div().text_color(theme.success).child(format!(
                "Runs as {} ({})",
                a.label,
                a.config_dir.display()
            ))),
            Some(None) => Some(
                div()
                    .text_color(theme.warning)
                    .child("Unbound, agents won't start here"),
            ),
        };
        section("Test folder", cx)
            .child(Input::new(&self.test_input))
            .children(result)
    }
}

fn account_ids(config: &AccountsConfig) -> Vec<SharedString> {
    config
        .accounts
        .iter()
        .map(|a| a.id.clone().into())
        .collect()
}

impl Focusable for Settings {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for Settings {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        div()
            .id("settings")
            .key_context(CONTEXT)
            .track_focus(&self.focus)
            .on_action(cx.listener(Self::save_account))
            .on_action(cx.listener(Self::cancel_edit))
            .on_action(cx.listener(Self::bind_folder))
            .on_action(cx.listener(Self::edit_account))
            .on_action(cx.listener(Self::remove_account))
            .on_action(cx.listener(Self::remove_binding))
            .size_full()
            .overflow_y_scroll()
            .p_4()
            .flex()
            .flex_col()
            .gap_3()
            .bg(theme.background)
            .text_color(theme.foreground)
            .child(div().text_xl().child("SkyeLab · Accounts"))
            .children(self.error.clone().map(|e| {
                div()
                    .id("settings-error")
                    .p_2()
                    .rounded_md()
                    .border_1()
                    .border_color(theme.danger)
                    .text_color(theme.danger)
                    .child(e)
            }))
            .child(self.render_accounts(cx))
            .child(self.render_bindings(cx))
            .child(self.render_test(cx))
    }
}

mod widgets;
use widgets::{path_text, row, section};

#[cfg(test)]
mod tests;
