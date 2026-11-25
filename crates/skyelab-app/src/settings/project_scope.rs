//! "Applying settings for <project ▾>" (T3's settings scope sentence) on Settings → Chat and Worktrees: with a
//! project picked, the same rows edit that project's own defaults for new threads, and each can inherit the app's.
//! The project's account is not among them: that stays the folder's binding (Accounts).

use super::Settings;
use super::widgets::DESC;
use crate::ui::{Button, Switch};
use crate::workspace::thread_extras::prefs;
use gpui::{AnyElement, Context, SharedString, Window, prelude::*};
use gpui_component::{
    ActiveTheme as _, h_flex,
    menu::{DropdownMenu as _, PopupMenuItem},
};
use skyelab_core::thread_defaults::ThreadField;
use std::path::PathBuf;
use std::rc::Rc;

/// What picking a dropdown value does.
pub(super) type Pick = Rc<dyn Fn(&mut Settings, Option<String>, &mut Context<Settings>)>;

impl Settings {
    /// Edits `dir`'s own defaults from now on (`None`: the app-wide ones), and shows its Codex model in the field.
    pub fn set_scope(&mut self, dir: Option<PathBuf>, window: &mut Window, cx: &mut Context<Self>) {
        self.scope = dir;
        self.refresh_rules(cx);
        let codex = self.shown_field(ThreadField::CodexModel, cx);
        self.codex_model_input.update(cx, |i, cx| {
            i.set_value(codex.unwrap_or_default(), window, cx)
        });
        let cursor = self.shown_field(ThreadField::CursorModel, cx);
        self.cursor_model_input.update(cx, |i, cx| {
            i.set_value(cursor.unwrap_or_default(), window, cx)
        });
        let antigravity = self.shown_field(ThreadField::AntigravityModel, cx);
        self.antigravity_model_input.update(cx, |i, cx| {
            i.set_value(antigravity.unwrap_or_default(), window, cx)
        });
        self.threads_error = None;
        cx.notify();
    }

    /// Shows the picked project's rules under the account its folder is bound to now (also after a rebind).
    pub(super) fn refresh_rules(&mut self, cx: &mut Context<Self>) {
        let Some(rules) = self.rules.clone() else {
            return;
        };
        let dir = self.scope.clone();
        let account = dir
            .as_deref()
            .and_then(|d| self.config.resolve(d))
            .map(|a| a.id.clone());
        rules.update(cx, |r, cx| r.show(dir.as_deref(), account, cx));
    }

    /// The value the scoped layer holds (a project's `None` inherits).
    pub(super) fn shown_field(&self, f: ThreadField, cx: &gpui::App) -> Option<String> {
        let t = prefs::get(cx);
        match &self.scope {
            None => t.field(f).clone(),
            Some(dir) => t.projects.get(dir).and_then(|p| p.field(f).clone()),
        }
    }

    /// Sets `f` in the scoped layer and saves.
    pub(super) fn set_shown_field(
        &mut self,
        f: ThreadField,
        value: Option<String>,
        cx: &mut Context<Self>,
    ) {
        match self.scope.clone() {
            None => self.set_threads(move |t| t.set_field(f, value.clone()), cx),
            Some(dir) => self.set_threads(move |t| t.set_project_field(&dir, f, value.clone()), cx),
        }
    }

    /// A dropdown for one shared field. `options` are the app-wide choices; a project gets "Inherit: <app's>" first
    /// in place of the app's own unset entry.
    pub(super) fn field_choice(
        &self,
        id: &'static str,
        f: ThreadField,
        options: Vec<(Option<String>, String)>,
        cx: &Context<Self>,
    ) -> AnyElement {
        let app = prefs::get(cx);
        let label_of = |v: &Option<String>| {
            options
                .iter()
                .find(|(o, _)| o == v)
                .map(|(_, l)| l.clone())
                .or_else(|| v.clone())
                .unwrap_or_else(|| options.first().map(|(_, l)| l.clone()).unwrap_or_default())
        };
        let pick: Pick = Rc::new(move |this, v, cx| this.set_shown_field(f, v, cx));
        if self.scope.is_none() {
            // Unset shows (and checks) the first option, as the agent treats it: Full access for the permission mode.
            let current = Some(app.field(f).clone())
                .filter(|v| v.is_some() || options.iter().any(|(o, _)| o.is_none()))
                .unwrap_or_else(|| options.first().and_then(|(o, _)| o.clone()));
            let label = label_of(&current);
            return self.dropdown(id, label, current, options, pick, cx);
        }
        let inherit = (None, format!("Inherit: {}", label_of(app.field(f))));
        let current = self.shown_field(f, cx);
        let label = match &current {
            None => inherit.1.clone(),
            v => label_of(v),
        };
        let options = std::iter::once(inherit)
            .chain(options.into_iter().filter(|(v, _)| v.is_some()))
            .collect();
        self.dropdown(id, label, current, options, pick, cx)
    }

    /// "New threads start in a worktree": a switch app-wide, Inherit / On / Off for a project.
    pub(super) fn worktree_control(&self, cx: &Context<Self>) -> AnyElement {
        let app = prefs::get(cx).new_worktree;
        let Some(dir) = self.scope.clone() else {
            return Switch::new("threads-worktree")
                .checked(app)
                .accessibility_label("New threads start in a worktree")
                .on_click(cx.listener(|this, on: &bool, _, cx| {
                    let on = *on;
                    this.set_threads(move |t| t.new_worktree = on, cx)
                }))
                .into_any_element();
        };
        let word = |on: bool| if on { "On" } else { "Off" };
        let own = prefs::get(cx)
            .projects
            .get(&dir)
            .and_then(|p| p.new_worktree);
        let inherit = format!("Inherit: {}", word(app));
        let label = own.map_or_else(|| inherit.clone(), |on| word(on).to_owned());
        let options = vec![
            (None, inherit),
            (Some("on".to_owned()), "On".to_owned()),
            (Some("off".to_owned()), "Off".to_owned()),
        ];
        let current = own.map(|on| if on { "on" } else { "off" }.to_owned());
        let pick: Pick = Rc::new(move |this, v, cx| {
            let on = v.map(|v| v == "on");
            let dir = dir.clone();
            this.set_threads(move |t| t.set_project_worktree(&dir, on), cx)
        });
        self.dropdown("threads-worktree", label, current, options, pick, cx)
    }

    /// A dropdown of `options` (value, label), `current` checked.
    pub(super) fn dropdown(
        &self,
        id: &'static str,
        label: String,
        current: Option<String>,
        options: Vec<(Option<String>, String)>,
        pick: Pick,
        cx: &Context<Self>,
    ) -> AnyElement {
        let view = cx.entity().downgrade();
        Button::new(id)
            .outline()
            .small()
            .label(SharedString::from(label))
            .dropdown_caret(true)
            .dropdown_menu(move |mut menu, _, _| {
                for (value, label) in options.clone() {
                    let (view, pick) = (view.clone(), pick.clone());
                    let on = value == current;
                    menu = menu.item(PopupMenuItem::new(label).checked(on).on_click(
                        move |_, _, cx| {
                            let value = value.clone();
                            let _ = view.update(cx, |this, cx| pick(this, value, cx));
                        },
                    ));
                }
                menu
            })
            .into_any_element()
    }

    /// The sentence over Chat and Worktrees: "Applying settings for [All projects ▾]". None without local projects.
    pub(super) fn render_scope(&self, cx: &Context<Self>) -> Option<AnyElement> {
        if self.projects.is_empty() {
            return None;
        }
        let name = |dir: &PathBuf| {
            self.projects
                .iter()
                .find(|(_, p)| p == dir)
                .map_or_else(|| dir.display().to_string(), |(n, _)| n.to_string())
        };
        let label = self
            .scope
            .as_ref()
            .map_or_else(|| "All projects".to_owned(), name);
        let (projects, current) = (self.projects.clone(), self.scope.clone());
        let view = cx.entity().downgrade();
        let menu = Button::new("settings-scope")
            .outline()
            .small()
            .label(SharedString::from(label))
            .dropdown_caret(true)
            .dropdown_menu(move |menu, _, _| {
                let item = |label: SharedString, dir: Option<PathBuf>| {
                    let view = view.clone();
                    PopupMenuItem::new(label).checked(dir == current).on_click(
                        move |_, window, cx| {
                            let dir = dir.clone();
                            let _ = view.update(cx, |this, cx| this.set_scope(dir, window, cx));
                        },
                    )
                };
                let mut menu = menu.item(item("All projects".into(), None)).separator();
                for (name, dir) in &projects {
                    menu = menu.item(item(name.clone(), Some(dir.clone())));
                }
                menu
            });
        let muted = cx.theme().muted_foreground;
        Some(
            gpui_component::v_flex()
                .gap_1()
                .px_1()
                .child(
                    h_flex()
                        .gap_1p5()
                        .text_size(DESC)
                        .text_color(muted)
                        .child("Applying settings for")
                        .child(menu),
                )
                .when(self.scope.is_some(), |d| {
                    d.child(gpui::div().text_size(DESC).text_color(muted).child(
                        "This project's new threads use these; Inherit follows All projects. Its \
                                 account always comes from the folder's binding in Accounts.",
                    ))
                })
                .into_any_element(),
        )
    }

    /// Restore defaults with a project picked: that project's overrides on this page go back to inheriting.
    pub(super) fn restore_project(
        &mut self,
        dir: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let section = self.section;
        self.set_threads(
            move |t| match section {
                super::Section::Chat => {
                    for f in ThreadField::ALL {
                        t.set_project_field(&dir, f, None);
                    }
                }
                super::Section::Worktrees => t.set_project_worktree(&dir, None),
                _ => {}
            },
            cx,
        );
        self.codex_model_input
            .update(cx, |i, cx| i.set_value("", window, cx));
        self.cursor_model_input
            .update(cx, |i, cx| i.set_value("", window, cx));
        self.antigravity_model_input
            .update(cx, |i, cx| i.set_value("", window, cx));
    }
}
