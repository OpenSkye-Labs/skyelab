//! Settings, shown as a modal over the window: a searchable section nav and the chosen section.
//! Thin: every account rule lives in `skyelab_core::AccountsConfig`.

use crate::ui::Button;
use anyhow::{Context as _, Result};
use gpui::{
    Action, App, Context, Entity, EventEmitter, FocusHandle, Focusable, KeyBinding,
    KeyBindingContextPredicate, PathPromptOptions, SharedString, Subscription, Task, Window,
    actions, div, prelude::*,
};
use gpui_component::{
    ActiveTheme as _, Sizable as _,
    input::{Input, InputEvent, InputState},
};
use skyelab_core::{Account, AccountsConfig, Binding, Provider};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;

actions!(skyelab, [SaveAccount, CancelEdit, BindFolder, FocusSearch]);

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
    /// The user asked to log account `id` in to its agent (Claude or Codex), in that account's own config dir.
    AgentLogin { id: String },
    /// Open keybindings.toml in the user's editor, writing the commented template first if it doesn't exist.
    OpenKeysFile { path: PathBuf },
    /// Close the modal (the header's ×, Esc with nothing to cancel, or the focus leaving it).
    Closed,
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
    let bindings: [(&str, Box<dyn Action>); 4] = [
        ("secondary-enter", Box::new(SaveAccount)),
        ("escape", Box::new(CancelEdit)),
        ("secondary-shift-b", Box::new(BindFolder)), // secondary-b is the workspace's sidebar key
        ("/", Box::new(FocusSearch)),
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

/// An account in the binding picker: shown by its label, picked by its id.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct AccountChoice {
    pub(super) id: SharedString,
    pub(super) label: SharedString,
}

pub struct Settings {
    pub(super) path: Option<PathBuf>,
    pub(super) config: AccountsConfig,
    pub(crate) error: Option<String>,
    /// The error banner shows the full text.
    pub(super) error_open: bool,
    /// Why saving is off. Set when the file failed to load, so an edit never overwrites it.
    pub(super) locked: Option<String>,
    /// Id of the account the form is editing; `None` means the form adds a new one.
    pub(super) editing: Option<String>,
    /// The "Add account" card is open.
    pub(super) adding: bool,
    /// The provider the form saves the account with.
    pub(crate) provider: Provider,
    pub(crate) id_input: Entity<InputState>,
    pub(crate) label_input: Entity<InputState>,
    pub(crate) dir_input: Entity<InputState>,
    pub(super) gh_input: Entity<InputState>,
    pub(super) test_input: Entity<InputState>,
    /// Who is logged in to github.com in each account's GitHub config dir (`None`: nobody).
    pub(super) gh_logins: HashMap<String, Option<String>>,
    /// Why checking an account's GitHub login failed.
    pub(super) gh_errors: HashMap<String, String>,
    pub(super) _gh_check: Option<Task<()>>,
    /// Cached so render does no filesystem work (`resolve` canonicalizes).
    pub(super) test_result: Option<Option<Account>>,
    /// Stale bindings' paths and where each resolves now (`None`: can't be resolved).
    pub(super) stale: Vec<(PathBuf, Option<PathBuf>)>,
    /// Account id chosen in the "Bind a folder to" menu; defaults to the first configured account.
    pub(super) binding_account: Option<SharedString>,
    pub section: Section,
    /// Settings → Chat's Codex, Cursor and Antigravity model fields, and why the last Chat or Diffs save failed.
    pub(super) codex_model_input: Entity<InputState>,
    pub(crate) cursor_model_input: Entity<InputState>,
    pub(super) antigravity_model_input: Entity<InputState>,
    /// "Custom model id…" was picked: the Codex text field shows.
    pub(super) codex_custom: bool,
    pub(super) threads_error: Option<String>,
    /// Settings → Chat → Visual replies' new-host field.
    pub(super) page_host_input: Entity<InputState>,
    /// Chat and Worktrees edit this project folder's own defaults (`None`: All projects).
    pub scope: Option<PathBuf>,
    /// The local projects the scope menu lists (name, folder); the workspace sets them as the modal opens.
    pub projects: Vec<(SharedString, PathBuf)>,
    /// Settings → Keybindings' search, and the rows it leaves.
    pub(super) keys_search: Entity<InputState>,
    /// Where the focus was when the modal opened; it goes back there on close.
    pub return_focus: Option<FocusHandle>,
    /// The nav's search field.
    pub(crate) search_input: Entity<InputState>,
    /// Settings → General's editor command.
    pub(super) editor_input: Entity<InputState>,
    keys: Vec<keys::KeyRow>,
    /// The Sharing section's content; `None` without a tailnet hook.
    pub remote: Option<Entity<crate::remote::RemotePanel>>,
    pub machines: Option<Entity<crate::workspace::machines_page::MachinesPage>>,
    /// The Usage section's content; the workspace sets it and reads the store for it.
    pub usage: Option<Entity<crate::workspace::usage_page::UsagePage>>,
    /// The Scheduled tasks section's content, set by the workspace like `usage`.
    pub scheduled: Option<Entity<crate::workspace::scheduled::ScheduledPage>>,
    /// Settings → Chat's permission rules for the picked project, set by the workspace like `usage`.
    pub rules: Option<Entity<crate::workspace::ProjectRulesPage>>,
    pub(super) focus: FocusHandle,
    pub(crate) body_scroll: scroll::BodyScroll,
    pub(super) _pick: Option<Task<()>>,
    /// Whether each account is signed in to its agent CLI (shared with the setup page).
    pub(super) login: Entity<crate::workspace::login_status::LoginStatus>,
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
        let id_input = input("work", window, cx);
        let label_input = input("Work", window, cx);
        let dir_input = input(
            "Empty: ~/.claude-<id>, ~/.codex-<id>, ~/.cursor-<id> or ~/.antigravity-<id>",
            window,
            cx,
        );
        let gh_input = input("/Users/you/.config/gh-work", window, cx);
        let test_input = input("/path/to/a/folder", window, cx);
        let keys_search = input("Search shortcuts", window, cx);
        let search_input = input("Search", window, cx);
        let editor_input = input("Automatic", window, cx);
        editor_input.update(cx, |i, cx| i.set_value(general::editor(cx), window, cx));
        // Empty means the account's default app-wide and Inherit for a project; the row's description says which.
        let codex_model_input = input("Not set", window, cx);
        let prefs_now = crate::workspace::thread_extras::prefs::get(cx);
        codex_model_input.update(cx, |i, cx| {
            i.set_value(prefs_now.codex_model.unwrap_or_default(), window, cx)
        });
        let cursor_model_input = input("Not set", window, cx);
        cursor_model_input.update(cx, |i, cx| {
            i.set_value(prefs_now.cursor_model.unwrap_or_default(), window, cx)
        });
        let antigravity_model_input = input("Not set", window, cx);
        let antigravity_model = crate::workspace::thread_extras::prefs::get(cx).antigravity_model;
        antigravity_model_input.update(cx, |i, cx| {
            i.set_value(antigravity_model.unwrap_or_default(), window, cx)
        });
        let page_host_input = input("cdn.example.com or *.example.com", window, cx);
        let ids = account_ids(&config);
        let binding_account = ids.first().map(|c| c.id.clone());
        let focus = cx.focus_handle();
        let login = crate::workspace::login_status::LoginStatus::shared(cx);
        let subs = vec![
            // A redraw, not a notify: the workspace takes a Settings notify as "accounts saved" and re-reads them.
            cx.observe(&login, |_, _, cx| cx.refresh_windows()),
            cx.subscribe(&test_input, |this, _, e: &InputEvent, cx| {
                if matches!(e, InputEvent::Change) {
                    this.refresh_fs(cx);
                    cx.notify();
                }
            }),
            cx.subscribe(&keys_search, |this, _, e: &InputEvent, cx| {
                if matches!(e, InputEvent::Change) {
                    this.refresh_keys(cx);
                }
            }),
            cx.observe_global::<crate::keymap::Keymap>(Self::refresh_keys),
            // The modal has no focus trap of its own: anything that takes the focus away (F6, the palette, a new
            // terminal) closes it, so nothing opens hidden under it. Not the window going inactive (another app,
            // the folder picker): GPUI reports that as a focus-out too.
            cx.on_focus_out(&focus, window, |_, _, window, cx| {
                if window.is_window_active() {
                    cx.emit(SettingsEvent::Closed)
                }
            }),
            cx.subscribe(&search_input, |_, _, e: &InputEvent, cx| {
                if matches!(e, InputEvent::Change) {
                    cx.notify();
                }
            }),
            cx.subscribe(&editor_input, |this, _, e: &InputEvent, cx| {
                if matches!(e, InputEvent::Blur | InputEvent::PressEnter { .. }) {
                    this.save_editor(cx);
                }
            }),
            cx.subscribe_in(
                &page_host_input,
                window,
                |this, _, e: &InputEvent, window, cx| {
                    if matches!(e, InputEvent::PressEnter { .. }) {
                        this.add_page_host(window, cx);
                    }
                },
            ),
            cx.subscribe(&codex_model_input, |this, _, e: &InputEvent, cx| {
                if matches!(e, InputEvent::Blur | InputEvent::PressEnter { .. }) {
                    this.save_codex_model(cx);
                }
            }),
            cx.subscribe(&cursor_model_input, |this, _, e: &InputEvent, cx| {
                if matches!(e, InputEvent::Blur | InputEvent::PressEnter { .. }) {
                    this.save_cursor_model(cx);
                }
            }),
            cx.subscribe(&antigravity_model_input, |this, _, e: &InputEvent, cx| {
                if matches!(e, InputEvent::Blur | InputEvent::PressEnter { .. }) {
                    this.save_antigravity_model(cx);
                }
            }),
        ];
        let locked = match (&path, error) {
            (None, _) => Some("no config directory on this OS, so nothing can be saved".into()),
            (Some(p), Some(e)) => Some(format!(
                "accounts.toml couldn't be read. Editing is off until you fix it and restart: {e} ({})",
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
            adding: false,
            error_open: false,
            search_input,
            editor_input,
            return_focus: None,
            provider: Provider::Claude,
            id_input,
            label_input,
            dir_input,
            gh_input,
            test_input,
            codex_model_input,
            cursor_model_input,
            antigravity_model_input,
            codex_custom: false,
            threads_error: None,
            page_host_input,
            scope: None,
            projects: Vec::new(),
            keys_search,
            keys: Vec::new(),
            gh_logins: HashMap::new(),
            gh_errors: HashMap::new(),
            _gh_check: None,
            test_result: None,
            binding_account,
            section: Section::default(),
            remote: None,
            machines: None,
            usage: None,
            scheduled: None,
            rules: None,
            focus,
            body_scroll: scroll::BodyScroll::new(),
            _pick: None,
            login,
            _subs: subs,
        };
        this.check_gh_logins(cx);
        this.refresh_keys(cx);
        this
    }

    /// Shows `config` (just saved here, or re-read from the file when the page is shown again).
    pub fn set_config(
        &mut self,
        config: AccountsConfig,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.config = config;
        self.refresh_rules(cx);
        let ids = account_ids(&self.config);
        let known = |id: &SharedString| ids.iter().any(|c| &c.id == id);
        self.binding_account = self
            .binding_account
            .take()
            .filter(known)
            .or_else(|| ids.first().map(|c| c.id.clone()));
        self.refresh_fs(cx);
        self.check_gh_logins(cx);
        cx.notify();
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
        // From the file as it is now, not this page's copy: the page lives all session, and the file may have been
        // edited elsewhere since it loaded.
        let mut next = AccountsConfig::default();
        let result = self
            .path
            .as_deref()
            .context("no config directory on this OS")
            .and_then(|path| {
                next = AccountsConfig::load(path)?;
                edit(&mut next)?;
                next.save(path)
            });
        let ok = match result {
            Ok(()) => {
                self.error = None;
                self.set_config(next, window, cx);
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

    fn bind_folder(&mut self, _: &BindFolder, window: &mut Window, cx: &mut Context<Self>) {
        let Some(account) = self.binding_account.clone() else {
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

fn account_ids(config: &AccountsConfig) -> Vec<AccountChoice> {
    let name = |a: &skyelab_core::Account| {
        if a.label.is_empty() {
            a.id.clone()
        } else {
            a.label.clone()
        }
    };
    config
        .accounts
        .iter()
        .map(|a| {
            // Two accounts with one label would read the same: the id tells them apart.
            let shared = config
                .accounts
                .iter()
                .filter(|b| name(b) == name(a))
                .count()
                > 1;
            AccountChoice {
                id: a.id.clone().into(),
                label: if shared {
                    format!("{} ({})", name(a), a.id)
                } else {
                    name(a)
                }
                .into(),
            }
        })
        .collect()
}

impl EventEmitter<SettingsEvent> for Settings {}

impl Focusable for Settings {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

mod about;
mod accounts;
mod accounts_card;
pub(crate) use accounts::provider_mark;
mod appearance;
mod bindings;
mod codex_models;
pub(crate) use codex_models::KnownCodexModels;
mod general;
mod keys;
mod nav;
mod page;
mod project_scope;
pub use page::Section;
pub(crate) mod scroll;
mod source_control;
mod threads;
mod visual_replies;
pub(crate) mod widgets;

#[cfg(test)]
mod outside_edit_test;
#[cfg(test)]
mod project_scope_test;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod visual_replies_test;

#[cfg(all(test, unix))] // the fake gh is a shell script
mod github_test;

#[cfg(test)]
mod antigravity_test;
#[cfg(test)]
#[path = "claude_login_test.rs"]
mod claude_login_test;
#[cfg(test)]
mod codex_test;
#[cfg(test)]
mod keys_test;
#[cfg(test)]
mod restore_error_test;
#[cfg(test)]
mod search_test;
