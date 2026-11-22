//! Accounts and folder-bindings settings. Thin: every rule lives in `skyelab_core::AccountsConfig`.

use anyhow::{Context as _, Result};
use gpui::{
    App, Context, Entity, FocusHandle, Focusable, KeyBinding, PathPromptOptions, SharedString,
    Subscription, Task, Window, actions, div, prelude::*,
};
use gpui_component::{
    ActiveTheme as _, IndexPath, Sizable as _,
    button::{Button, ButtonVariants as _},
    input::{Input, InputEvent, InputState},
    select::{Select, SelectState},
};
use skyelab_core::{Account, AccountsConfig, Binding};
use std::path::{Path, PathBuf};

actions!(skyelab, [SaveAccount, CancelEdit, BindFolder]);

const CONTEXT: &str = "Settings";

pub fn init(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("secondary-enter", SaveAccount, Some(CONTEXT)),
        KeyBinding::new("escape", CancelEdit, Some(CONTEXT)),
        KeyBinding::new("secondary-b", BindFolder, Some(CONTEXT)),
    ]);
}

type AccountSelect = SelectState<Vec<SharedString>>;

pub struct Settings {
    path: Option<PathBuf>,
    config: AccountsConfig,
    error: Option<String>,
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
        let dir_input = input("config dir, e.g. ~/.claude-acme", window, cx);
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
        let error = error.or_else(|| {
            path.is_none()
                .then(|| "no config directory on this OS".into())
        });
        Self {
            path,
            config,
            error,
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

    fn edit_account(&mut self, account: &Account, window: &mut Window, cx: &mut Context<Self>) {
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
                let edit = a.clone();
                let id = a.id.clone();
                row(cx)
                    .child(
                        div()
                            .flex_1()
                            .flex()
                            .flex_col()
                            .child(format!("{}  ({})", a.label, a.id))
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(theme.muted_foreground)
                                    .child(a.config_dir.display().to_string()),
                            ),
                    )
                    .child(
                        Button::new(SharedString::from(format!("edit-{}", a.id)))
                            .label("Edit")
                            .small()
                            .ghost()
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.edit_account(&edit, window, cx)
                            })),
                    )
                    .child(
                        Button::new(SharedString::from(format!("remove-account-{}", a.id)))
                            .label("Remove")
                            .small()
                            .danger()
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.apply(|c| c.remove_account(&id), window, cx);
                            })),
                    )
            }))
            .child(div().pt_2().text_sm().child(form_title))
            .child(
                div()
                    .flex()
                    .gap_2()
                    .when(self.editing.is_none(), |d| {
                        d.child(div().w_40().child(Input::new(&self.id_input)))
                    })
                    .child(div().w_56().child(Input::new(&self.label_input)))
                    .child(div().flex_1().child(Input::new(&self.dir_input)))
                    .child(
                        Button::new("save-account")
                            .label(if self.editing.is_some() {
                                "Update"
                            } else {
                                "Add"
                            })
                            .primary()
                            .on_click(|_, window, cx| {
                                window.dispatch_action(Box::new(SaveAccount), cx)
                            }),
                    )
                    .when(self.editing.is_some(), |d| {
                        d.child(Button::new("cancel-edit").label("Cancel").ghost().on_click(
                            |_, window, cx| window.dispatch_action(Box::new(CancelEdit), cx),
                        ))
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
                let path = b.path.clone();
                row(cx)
                    .child(div().flex_1().child(b.path.display().to_string()))
                    .child(
                        div()
                            .text_color(theme.muted_foreground)
                            .child(format!("→  {}", label_of(&b.account))),
                    )
                    .child(
                        Button::new(SharedString::from(format!("unbind-{}", b.path.display())))
                            .label("Remove")
                            .small()
                            .danger()
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.apply(|c| c.remove_binding(&path), window, cx);
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
                            .on_click(|_, window, cx| {
                                window.dispatch_action(Box::new(BindFolder), cx)
                            }),
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

fn section(title: &str, cx: &App) -> gpui::Div {
    div()
        .flex()
        .flex_col()
        .gap_1()
        .p_3()
        .border_1()
        .border_color(cx.theme().border)
        .rounded_md()
        .child(div().text_lg().pb_1().child(title.to_string()))
}

fn row(cx: &App) -> gpui::Div {
    div()
        .flex()
        .items_center()
        .gap_2()
        .py_1()
        .border_b_1()
        .border_color(cx.theme().border)
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
