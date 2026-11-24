//! Settings → Chat (what new threads start with, app-wide or for one project) and Diffs & pull requests. A thread's
//! own pick still wins.

use super::Settings;
use super::widgets::{section, setting};
use crate::workspace::thread_extras::prefs;
use gpui::{AnyElement, Context, div, prelude::*};
use gpui_component::{Sizable as _, h_flex, input::Input, switch::Switch, v_flex};
use skyelab_agent::{CLAUDE_EFFORTS, CLAUDE_MODELS, Mode};
use skyelab_core::thread_defaults::ThreadField;
use skyelab_core::{DiffSettings, ThreadSettings};

/// The permission modes a new thread may start in. Full access is confirmed per thread, never a default.
const MODES: [(Mode, &str); 3] = [
    (Mode::Default, "Ask"),
    (Mode::Plan, "Plan"),
    (Mode::AcceptEdits, "Accept edits"),
];
/// Codex's reasoning efforts (its models list theirs; these are the common ones).
const CODEX_EFFORTS: [&str; 4] = ["low", "medium", "high", "xhigh"];

/// Settings → Diffs & pull requests' fetch intervals, in minutes.
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
    /// Applies `change` to the new-thread settings (Chat, Diffs, Worktrees) and saves it; a failed save shows on the page.
    pub(super) fn set_threads(
        &mut self,
        change: impl Fn(&mut ThreadSettings) + Send + 'static,
        cx: &mut Context<Self>,
    ) {
        let save = prefs::edit(change, cx);
        cx.spawn(async move |this, cx| {
            let failed = save
                .await
                .err()
                .map(|e| format!("Saving the thread settings failed: {e:#}"));
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
        if self.shown_field(ThreadField::CodexModel, cx) != model {
            self.set_shown_field(ThreadField::CodexModel, model, cx);
        }
    }

    pub(super) fn render_chat(&self, cx: &mut Context<Self>) -> AnyElement {
        let t = prefs::get(cx);
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
                self.field_choice("threads-mode", ThreadField::PermissionMode, modes, cx),
                cx,
            ),
            setting(
                false,
                "Claude model",
                if self.scope.is_some() {
                    "For this project's new Claude threads, over the last model picked in it."
                } else {
                    "For new Claude threads in a project that hasn't picked one yet."
                },
                h_flex()
                    .gap_2()
                    .child(self.field_choice(
                        "threads-claude-model",
                        ThreadField::ClaudeModel,
                        claude_models,
                        cx,
                    ))
                    .child(self.field_choice(
                        "threads-claude-effort",
                        ThreadField::ClaudeEffort,
                        efforts(CLAUDE_EFFORTS),
                        cx,
                    )),
                cx,
            ),
            setting(
                false,
                "Codex model",
                if self.scope.is_some() {
                    "A Codex model id (empty: inherit All projects'), and its reasoning effort."
                } else {
                    "A Codex model id (empty: the account's default), and its reasoning effort."
                },
                h_flex()
                    .gap_2()
                    .child(
                        div()
                            .w(gpui::px(160.))
                            .child(Input::new(&self.codex_model_input).small()),
                    )
                    .child(self.field_choice(
                        "threads-codex-effort",
                        ThreadField::CodexEffort,
                        efforts(&CODEX_EFFORTS),
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
                    this.set_threads(move |t| t.generate_titles = on, cx)
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
            // App-wide only: a project picked shows just what it can override.
            .when(self.scope.is_none(), |d| {
                d.child(section(
                    "Titles",
                    "How threads get their names.",
                    titles,
                    cx,
                ))
            })
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
                    .on_click(cx.listener(move |this, on: &bool, _, cx| {
                        let mut d = prefs::get_diff(cx);
                        set(&mut d, *on);
                        let save = prefs::save_diff(d, cx);
                        this.report(save, "Diffs", cx);
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
                        this.set_threads(move |t| t.generate_git_text = on, cx)
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
            .child(section(
                "Commits & pull requests",
                "Keeping branches current, and help writing commits.",
                git,
                cx,
            ))
            .into_any_element()
    }

    /// A dropdown of `options` (value, label) setting one app-wide field of the new-thread settings.
    fn choice(
        &self,
        id: &'static str,
        current: &Option<String>,
        options: Vec<(Option<String>, String)>,
        set: fn(&mut ThreadSettings, Option<String>),
        cx: &Context<Self>,
    ) -> AnyElement {
        let label = options
            .iter()
            .find(|(v, _)| v == current)
            .map_or_else(|| current.clone().unwrap_or_default(), |(_, l)| l.clone());
        let pick: super::project_scope::Pick =
            std::rc::Rc::new(move |this, v, cx| this.set_threads(move |t| set(t, v.clone()), cx));
        self.dropdown(id, label, current.clone(), options, pick, cx)
    }
}
