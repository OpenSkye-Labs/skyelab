//! Settings → Chat (what new threads start with) and Diffs & pull requests. A thread's own pick still wins.

use super::Settings;
use super::widgets::{section, setting};
use crate::workspace::thread_extras::prefs;
use gpui::{AnyElement, Context, SharedString, div, prelude::*};
use gpui_component::{
    Sizable as _,
    button::Button,
    h_flex,
    input::Input,
    menu::{DropdownMenu as _, PopupMenuItem},
    switch::Switch,
    v_flex,
};
use skyelab_agent::{CLAUDE_EFFORTS, CLAUDE_MODELS, Mode};
use skyelab_core::{DiffSettings, ThreadSettings};

/// The permission modes a new thread may start in. Full access is confirmed per thread, never a default.
const MODES: [(Mode, &str); 3] = [
    (Mode::Default, "Ask"),
    (Mode::Plan, "Plan"),
    (Mode::AcceptEdits, "Accept edits"),
];
/// Codex's reasoning efforts (its models list theirs; these are the common ones).
const CODEX_EFFORTS: [&str; 4] = ["low", "medium", "high", "xhigh"];

/// Settings → Threads' fetch intervals, in minutes.
const FETCH_CHOICES: [(u32, &str); 5] = [
    (0, "Off"),
    (1, "Every minute"),
    (5, "Every 5 minutes"),
    (15, "Every 15 minutes"),
    (30, "Every 30 minutes"),
];

/// A model or effort name safe on a command line, as the agent layer checks it at launch.
fn valid_name(v: &str) -> bool {
    !v.starts_with('-')
        && v.len() <= 128
        && v.chars()
            .all(|c| c.is_ascii_alphanumeric() || "-_.:[]/".contains(c))
}

fn title_case(s: &str) -> String {
    let mut c = s.chars();
    c.next()
        .map(|f| f.to_uppercase().chain(c).collect())
        .unwrap_or_default()
}

impl Settings {
    /// Applies `change` to Settings → Threads and saves it; a failed save shows on the page.
    pub(super) fn set_threads(
        &mut self,
        change: impl FnOnce(&mut ThreadSettings),
        cx: &mut Context<Self>,
    ) {
        let mut next = prefs::get(cx);
        change(&mut next);
        let save = prefs::save(next, cx);
        cx.spawn(async move |this, cx| {
            let failed = save
                .await
                .err()
                .map(|e| format!("Saving Settings → Threads failed: {e:#}"));
            let _ = this.update(cx, |this, cx| {
                this.threads_error = failed;
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    /// The Codex model field was left or confirmed: save it (empty is the account's default).
    pub(super) fn save_codex_model(&mut self, cx: &mut Context<Self>) {
        let typed = self.codex_model_input.read(cx).value().trim().to_string();
        if !valid_name(&typed) {
            self.threads_error = Some(format!("\"{typed}\" isn't a model name."));
            return cx.notify();
        }
        let model = (!typed.is_empty()).then_some(typed);
        if prefs::get(cx).codex_model != model {
            self.set_threads(|t| t.codex_model = model, cx);
        }
    }

    pub(super) fn render_chat(&self, cx: &mut Context<Self>) -> AnyElement {
        let t = prefs::get(cx);
        let mode = Mode::parse(t.permission_mode.as_deref().unwrap_or_default());
        let modes = MODES
            .iter()
            .map(|(m, l)| (Some(m.as_str().to_owned()), (*l).to_owned()))
            .collect();
        let claude_models: Vec<(Option<String>, String)> =
            std::iter::once((None, "Default".to_owned()))
                .chain(
                    CLAUDE_MODELS
                        .iter()
                        .map(|(id, l)| (Some((*id).to_owned()), (*l).to_owned())),
                )
                .collect();
        let efforts = |list: &[&str]| -> Vec<(Option<String>, String)> {
            std::iter::once((None, "Default".to_owned()))
                .chain(list.iter().map(|e| (Some((*e).to_owned()), title_case(e))))
                .collect()
        };
        let defaults = [
            setting(
                true,
                "Default permissions",
                "What a new thread's agent may do without asking. Full access is picked per thread, never here.",
                self.choice(
                    "threads-mode",
                    &Some(mode.as_str().to_owned()),
                    modes,
                    |t, v| t.permission_mode = v,
                    cx,
                ),
                cx,
            ),
            setting(
                false,
                "Claude model",
                "For new Claude threads in a project that hasn't picked one yet.",
                h_flex()
                    .gap_2()
                    .child(self.choice(
                        "threads-claude-model",
                        &t.claude_model,
                        claude_models,
                        |t, v| t.claude_model = v,
                        cx,
                    ))
                    .child(self.choice(
                        "threads-claude-effort",
                        &t.claude_effort,
                        efforts(CLAUDE_EFFORTS),
                        |t, v| t.claude_effort = v,
                        cx,
                    )),
                cx,
            ),
            setting(
                false,
                "Codex model",
                "A Codex model id (empty: the account's default), and its reasoning effort.",
                h_flex()
                    .gap_2()
                    .child(
                        div()
                            .w(gpui::px(160.))
                            .child(Input::new(&self.codex_model_input).small()),
                    )
                    .child(self.choice(
                        "threads-codex-effort",
                        &t.codex_effort,
                        efforts(&CODEX_EFFORTS),
                        |t, v| t.codex_effort = v,
                        cx,
                    )),
                cx,
            ),
        ];
        let titles = [setting(
            true,
            "Generate thread titles",
            "After a thread's first reply, its own account's agent names it in a few words. Rename or \
             regenerate from the thread menu any time.",
            Switch::new("threads-titles")
                .checked(t.generate_titles)
                .accessibility_label("Generate thread titles")
                .on_click(cx.listener(|this, on: &bool, _, cx| {
                    let on = *on;
                    this.set_threads(|t| t.generate_titles = on, cx)
                })),
            cx,
        )];
        v_flex()
            .gap_8()
            .child(section(
                "New threads",
                "What a new thread starts with. A thread's own pick always wins.",
                defaults,
                cx,
            ))
            .child(section("Titles", "", titles, cx))
            .into_any_element()
    }

    pub(super) fn render_diffs(&self, cx: &mut Context<Self>) -> AnyElement {
        let t = prefs::get(cx);
        let d = prefs::get_diff(cx);
        let toggle = |id: &'static str,
                      title: &'static str,
                      about: &'static str,
                      on: bool,
                      set: fn(&mut DiffSettings, bool),
                      first: bool,
                      cx: &mut Context<Self>| {
            setting(
                first,
                title,
                about,
                Switch::new(id)
                    .checked(on)
                    .accessibility_label(title)
                    .on_click(cx.listener(move |_, on: &bool, _, cx| {
                        let mut d = prefs::get_diff(cx);
                        set(&mut d, *on);
                        prefs::save_diff(d, cx).detach_and_log_err(cx);
                        cx.notify();
                    })),
                cx,
            )
        };
        let view = [
            toggle(
                "diff-split",
                "Side by side",
                "Old and new next to each other; off shows one column.",
                d.split,
                |d, v| d.split = v,
                true,
                cx,
            ),
            toggle(
                "diff-wrap",
                "Wrap long lines",
                "Long lines wrap instead of running off the edge.",
                d.wrap,
                |d, v| d.wrap = v,
                false,
                cx,
            ),
            toggle(
                "diff-ws",
                "Ignore whitespace",
                "Hide changes that only touch spaces and tabs.",
                d.ignore_whitespace,
                |d, v| d.ignore_whitespace = v,
                false,
                cx,
            ),
        ];
        let git = [
            setting(
                true,
                "Fetch remote",
                "While a thread is shown, its folder fetches its upstream in the background, as its own account, so \
                 Details can show how far behind it is.",
                self.choice(
                    "threads-fetch",
                    &Some(t.fetch_minutes.to_string()),
                    FETCH_CHOICES
                        .iter()
                        .map(|(m, l)| (Some(m.to_string()), (*l).to_owned()))
                        .collect(),
                    |t, v| t.fetch_minutes = v.and_then(|v| v.parse().ok()).unwrap_or(5),
                    cx,
                ),
                cx,
            ),
            setting(
                false,
                "Generate commit messages and PRs",
                "A sparkle button in the Changes tab asks the thread's own account's agent for a commit message or a \
                 pull request title and description. It only fills the field.",
                Switch::new("threads-git-text")
                    .checked(t.generate_git_text)
                    .accessibility_label("Generate commit messages and PRs")
                    .on_click(cx.listener(|this, on: &bool, _, cx| {
                        let on = *on;
                        this.set_threads(|t| t.generate_git_text = on, cx)
                    })),
                cx,
            ),
        ];
        v_flex()
            .gap_8()
            .child(section(
                "Diff view",
                "How a new Changes tab draws a diff. Its header toggles change the same setting.",
                view,
                cx,
            ))
            .child(section("Commits & pull requests", "", git, cx))
            .into_any_element()
    }

    /// A dropdown of `options` (value, label) setting one field of Settings → Threads.
    fn choice(
        &self,
        id: &'static str,
        current: &Option<String>,
        options: Vec<(Option<String>, String)>,
        set: fn(&mut ThreadSettings, Option<String>),
        cx: &Context<Self>,
    ) -> AnyElement {
        let label: SharedString = options
            .iter()
            .find(|(v, _)| v == current)
            .map_or_else(|| current.clone().unwrap_or_default(), |(_, l)| l.clone())
            .into();
        let current = current.clone();
        let view = cx.entity().downgrade();
        Button::new(id)
            .outline()
            .small()
            .label(label)
            .dropdown_caret(true)
            .dropdown_menu(move |mut menu, _, _| {
                for (value, label) in options.clone() {
                    let view = view.clone();
                    let on = value == current;
                    menu = menu.item(PopupMenuItem::new(label).checked(on).on_click(
                        move |_, _, cx| {
                            let value = value.clone();
                            let _ =
                                view.update(cx, |this, cx| this.set_threads(|t| set(t, value), cx));
                        },
                    ));
                }
                menu
            })
            .into_any_element()
    }
}
