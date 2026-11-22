//! Accounts and folder-bindings settings. Thin: every rule lives in `skyelab_core::AccountsConfig`.

use anyhow::{Context as _, Result};
use gpui::{
    Action, App, Context, Entity, EventEmitter, FocusHandle, Focusable, KeyBinding,
    KeyBindingContextPredicate, PathPromptOptions, SharedString, Subscription, Task, Window,
    actions, div, prelude::*,
};
use gpui_component::{
    ActiveTheme as _, IndexPath, Sizable as _,
    button::{Button, ButtonVariants as _},
    input::{Input, InputEvent, InputState},
    select::{Select, SelectState},
};
use skyelab_core::{Account, AccountsConfig, Binding, Provider};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;

actions!(skyelab, [SaveAccount, CancelEdit, BindFolder]);

/// Fill the form with this account for editing.
#[derive(Clone, PartialEq, Action)]
#[action(namespace = skyelab, no_json)]
pub struct EditAccount {
    pub id: String,
}

/// Open a terminal running `gh auth login` in this account's GitHub config dir.
#[derive(Clone, PartialEq, Action)]
#[action(namespace = skyelab, no_json)]
pub struct GhLogIn {
    pub id: String,
}

pub enum SettingsEvent {
    /// The user asked to log in to GitHub under `dir` (an account's `GH_CONFIG_DIR`).
    GhLogin { dir: PathBuf },
    /// The user asked to log in to Codex under `dir` (a Codex account's `CODEX_HOME`).
    CodexLogin { dir: PathBuf },
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

#[derive(Clone, PartialEq, Action)]
#[action(namespace = skyelab, no_json)]
pub struct ReconfirmBinding {
    pub path: PathBuf,
    /// The folder the row showed; core refuses if the path resolves elsewhere by the click.
    pub now: PathBuf,
}

const CONTEXT: &str = "Settings";

pub fn init(cx: &mut App) {
    // `KeyBinding::new` can't parse `secondary-` (it doesn't know the OS); `load` resolves it.
    let context = KeyBindingContextPredicate::parse(CONTEXT).ok().map(Rc::new);
    let bindings: [(&str, Box<dyn Action>); 3] = [
        ("secondary-enter", Box::new(SaveAccount)),
        ("escape", Box::new(CancelEdit)),
        ("secondary-shift-b", Box::new(BindFolder)), // secondary-b is the workspace's sidebar key
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
    pub(crate) error: Option<String>,
    /// Why saving is off. Set when the file failed to load, so an edit never overwrites it.
    locked: Option<String>,
    /// Id of the account the form is editing; `None` means the form adds a new one.
    editing: Option<String>,
    /// The provider the form saves the account with.
    pub(crate) provider: Provider,
    id_input: Entity<InputState>,
    label_input: Entity<InputState>,
    dir_input: Entity<InputState>,
    gh_input: Entity<InputState>,
    test_input: Entity<InputState>,
    /// Who is logged in to github.com in each account's GitHub config dir (`None`: nobody).
    gh_logins: HashMap<String, Option<String>>,
    /// Why checking an account's GitHub login failed.
    gh_errors: HashMap<String, String>,
    _gh_check: Option<Task<()>>,
    /// Cached so render does no filesystem work (`resolve` canonicalizes).
    test_result: Option<Option<Account>>,
    /// Stale bindings' paths and where each resolves now (`None`: can't be resolved).
    stale: Vec<(PathBuf, Option<PathBuf>)>,
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
            "absolute config dir, e.g. /Users/me/.claude-acme or /Users/me/.codex-acme",
            window,
            cx,
        );
        let gh_input = input(
            "GitHub config dir (optional), e.g. /Users/me/.config/gh-acme",
            window,
            cx,
        );
        let test_input = input("/path/to/a/folder", window, cx);
        let ids = account_ids(&config);
        let selected = (!ids.is_empty()).then(|| IndexPath::new(0));
        let account_select = cx.new(|cx| SelectState::new(ids, selected, window, cx));
        let subs = vec![cx.subscribe(&test_input, |this, _, e: &InputEvent, cx| {
            if matches!(e, InputEvent::Change) {
                this.refresh_fs(cx);
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
        let stale = stale_bindings(&config);
        let mut this = Self {
            path,
            config,
            stale,
            error: locked.clone(),
            locked,
            editing: None,
            provider: Provider::Claude,
            id_input,
            label_input,
            dir_input,
            gh_input,
            test_input,
            gh_logins: HashMap::new(),
            gh_errors: HashMap::new(),
            _gh_check: None,
            test_result: None,
            account_select,
            focus: cx.focus_handle(),
            _pick: None,
            _subs: subs,
        };
        this.check_gh_logins(cx);
        this
    }

    /// Asks `gh` off the UI thread who is logged in to each account's GitHub config dir.
    pub fn check_gh_logins(&mut self, cx: &mut Context<Self>) {
        let dirs: Vec<(String, PathBuf)> = self
            .config
            .accounts
            .iter()
            .filter_map(|a| Some((a.id.clone(), a.gh_config_dir.clone()?)))
            .collect();
        let gh = crate::workspace::gh_bin(cx);
        let task = cx.background_spawn(async move {
            dirs.into_iter()
                .map(|(id, dir)| {
                    let login = skyelab_git::gh_login(&skyelab_git::GitHub {
                        gh: &gh,
                        config_dir: &dir,
                    });
                    (id, login.map_err(|e| e.to_string()))
                })
                .collect::<Vec<_>>()
        });
        self._gh_check = Some(cx.spawn(async move |this, cx| {
            let checked = task.await;
            let _ = this.update(cx, |this, cx| {
                this.gh_logins.clear();
                this.gh_errors.clear();
                for (id, login) in checked {
                    match login {
                        Ok(login) => {
                            this.gh_logins.insert(id, login);
                        }
                        Err(e) => {
                            this.gh_errors.insert(id, e);
                        }
                    }
                }
                cx.notify();
            });
        }));
    }

    fn gh_log_in(&mut self, a: &GhLogIn, _: &mut Window, cx: &mut Context<Self>) {
        let dir = self
            .config
            .accounts
            .iter()
            .find(|acct| acct.id == a.id)
            .and_then(|acct| acct.gh_config_dir.clone());
        match dir {
            Some(dir) => cx.emit(SettingsEvent::GhLogin { dir }),
            None => {
                self.error = Some(format!("set a GitHub config dir for '{}' first", a.id));
                cx.notify();
            }
        }
    }

    /// Asks for a terminal running `codex login` in a Codex account's `CODEX_HOME`.
    pub(crate) fn codex_log_in(&mut self, id: &str, cx: &mut Context<Self>) {
        let dir = self
            .config
            .accounts
            .iter()
            .find(|a| a.id == id && a.provider == Provider::Codex)
            .map(|a| a.config_dir.clone());
        match dir {
            Some(dir) => cx.emit(SettingsEvent::CodexLogin { dir }),
            None => {
                self.error = Some(format!("'{id}' is not a Codex account"));
                cx.notify();
            }
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
                self.refresh_fs(cx);
                self.check_gh_logins(cx);
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

    /// Re-reads everything that depends on the filesystem, so a folder swapped while this
    /// screen is open shows up on the next edit or keystroke in the test field.
    fn refresh_fs(&mut self, cx: &App) {
        self.stale = stale_bindings(&self.config);
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
            provider: self.provider,
            gh_config_dir: Some(value(&self.gh_input))
                .filter(|v| !v.is_empty())
                .map(PathBuf::from),
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
        self.provider = Provider::Claude;
        for input in [
            &self.id_input,
            &self.label_input,
            &self.dir_input,
            &self.gh_input,
        ] {
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
        self.provider = account.provider;
        let dir = account.config_dir.display().to_string();
        let gh = account
            .gh_config_dir
            .as_ref()
            .map(|d| d.display().to_string())
            .unwrap_or_default();
        for (input, text) in [
            (&self.id_input, account.id.clone()),
            (&self.label_input, account.label.clone()),
            (&self.dir_input, dir),
            (&self.gh_input, gh),
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

    fn reconfirm_binding(
        &mut self,
        a: &ReconfirmBinding,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.apply(|c| c.reconfirm_binding(&a.path, &a.now), window, cx);
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
                        |c| c.add_binding(Binding::new(path, account.to_string())),
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
}

fn stale_bindings(config: &AccountsConfig) -> Vec<(PathBuf, Option<PathBuf>)> {
    config
        .bindings
        .iter()
        .filter(|b| b.is_stale())
        .map(|b| (b.path.clone(), b.current_root()))
        .collect()
}

fn account_ids(config: &AccountsConfig) -> Vec<SharedString> {
    config
        .accounts
        .iter()
        .map(|a| a.id.clone().into())
        .collect()
}

impl EventEmitter<SettingsEvent> for Settings {}

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
            .on_action(cx.listener(Self::gh_log_in))
            .on_action(cx.listener(Self::remove_binding))
            .on_action(cx.listener(Self::reconfirm_binding))
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

mod render;
mod widgets;

#[cfg(test)]
mod tests;

#[cfg(all(test, unix))] // the fake gh is a shell script
mod github_test;

#[cfg(test)]
mod codex_test;
