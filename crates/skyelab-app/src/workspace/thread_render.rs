//! How a thread page looks. State and behavior live in `thread.rs`.

use super::thread::{CONTEXT, ThreadView};
use super::thread_find::FindHost as _;
use super::{NewThread, OpenSettings, OpenThread, SelectProject};
use crate::ui;
use crate::ui::SkyeIcon;
use crate::ui::{Button, Tooltip};
use gpui::{
    AnyElement, App, Context, FontWeight, IntoElement, SharedString, Window, div, prelude::*, px,
};
use gpui_component::{
    ActiveTheme as _, Icon, h_flex,
    menu::{DropdownMenu as _, PopupMenuItem},
    v_flex,
};

#[path = "thread_mode_picker.rs"]
pub(super) mod mode_picker;
#[path = "subagent_strip.rs"]
mod subagent_strip;

/// Agent text is untrusted (a repo can steer it), so its links only open web pages, never
/// `file://` paths or app URL schemes.
pub(super) fn opens_in_browser(url: &str) -> bool {
    let lower = url.trim_start().to_ascii_lowercase();
    lower.starts_with("https://") || lower.starts_with("http://")
}

impl ThreadView {
    /// The line above the composer about the agent's process: ended with no session running. Starting is named in
    /// the working row (`startup_line`).
    pub(in crate::workspace) fn agent_note(&self, refused: bool) -> Option<&'static str> {
        if self.starting || self.thread.is_none() || self.session.is_some() || refused {
            None
        } else if self.read_only {
            Some("This thread has ended and has no agent session to continue. Start a new thread.")
        } else if self.items.is_empty() {
            None // nothing ran yet (a new side chat): there is no agent to have ended
        } else {
            Some("The agent has ended. Send a message to continue its session.")
        }
    }

    /// The account this thread runs on: its own (from its store row), or for a new thread the one its folder resolves to.
    pub(super) fn account_badge(&self) -> ui::AccountBadge {
        match &self.thread {
            Some(t) => match self.config.accounts.iter().find(|a| a.id == t.account_id) {
                Some(a) => ui::AccountBadge::of(a),
                None => ui::AccountBadge::new(t.account_id.clone(), "Unknown account"),
            },
            None => self
                .folder_account
                .as_ref()
                .map_or_else(ui::AccountBadge::unbound, ui::AccountBadge::of),
        }
    }

    fn render_header(&self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let project = self.project_name.clone();
        let path = ui::short_path(&self.cwd, dirs::home_dir().as_deref());
        // The window's top row: breadcrumb and account, then the dock toggles. The thread's folder, branch, git and
        // lineage controls are in the thread details card.
        super::layout::page_header(window, cx)
            .child(
                h_flex()
                    .id("thread-breadcrumb")
                    // Windows: the breadcrumb's empty space moves the window; its links occlude it (`thread_link`).
                    .map(ui::drag_area)
                    .flex_1()
                    // On a very narrow center the breadcrumb keeps this much, then the account chip truncates.
                    .min_w(px(72.))
                    .overflow_hidden()
                    .gap_2()
                    .text_size(ui::BODY)
                    .font_weight(FontWeight::MEDIUM)
                    .tooltip(move |window, cx| Tooltip::new(path.clone()).build(window, cx))
                    .child(
                        div()
                            .min_w_0()
                            .truncate()
                            .text_color(theme.muted_foreground)
                            .child(project),
                    )
                    .child(
                        div()
                            .flex_shrink_0()
                            .text_color(theme.muted_foreground.opacity(0.6))
                            .child("/"),
                    )
                    .children(self.parent.clone().map(|(id, title)| {
                        h_flex()
                            .flex_shrink_0()
                            .gap_2()
                            .child(thread_link("parent-link", id, title))
                            .child(
                                div()
                                    .text_color(theme.muted_foreground.opacity(0.6))
                                    .child("/"),
                            )
                    }))
                    // The title keeps some room when the center is narrow (a right panel open): a fork's source link gives way
                    // first, then the folder.
                    .child(
                        div()
                            .flex_shrink(0.1)
                            .min_w_0()
                            .truncate()
                            .child(self.title()),
                    )
                    .children(self.composer.extras.forked_from.clone().map(|(id, label)| {
                        thread_link("forked-from", id, label)
                            .flex_shrink(8.)
                            .min_w(px(72.))
                    })),
            )
            .child(
                h_flex()
                    .id("provider-badge")
                    .min_w_0()
                    .child(self.account_badge().chip()),
            )
            .child(super::layout::panel_toggles(window, cx))
            .into_any_element()
    }

    fn render_cta(&self, cx: &mut Context<Self>) -> AnyElement {
        banner(
            "This folder isn't bound to an account",
            format!(
                "SkyeLab never picks an account for you. Bind {} to one, then send again.",
                ui::short_path(&self.cwd, dirs::home_dir().as_deref())
            ),
            div().debug_selector(|| "bind-cta".into()).child(
                Button::new("bind-cta")
                    .primary()
                    .small()
                    .label("Bind this folder to an account")
                    .on_click(|_, window, cx| {
                        crate::workspace::dispatch(Box::new(OpenSettings), window, cx)
                    }),
            ),
            cx,
        )
    }
}

impl ThreadView {
    fn render_refused(&self, reason: SharedString, cx: &mut Context<Self>) -> AnyElement {
        let project_id = self.project_id;
        banner(
            "This thread can't continue here",
            reason,
            h_flex().gap_2().children(self.handoff_cta(cx)).child(
                div().debug_selector(|| "new-thread-cta".into()).child(
                    Button::new("new-thread-cta")
                        .primary()
                        .small()
                        .label("Start a new thread")
                        .on_click(move |_, window, cx| {
                            // The sidebar may show another project by now; start in this thread's.
                            crate::workspace::dispatch(
                                Box::new(SelectProject { id: project_id }),
                                window,
                                cx,
                            );
                            crate::workspace::dispatch(Box::new(NewThread), window, cx);
                        }),
                ),
            ),
            cx,
        )
    }
}

/// A warning banner over the composer (T3's composer banner): a tinted card with a title, why, and the next step.
fn banner(
    title: &'static str,
    detail: impl Into<SharedString>,
    action: impl IntoElement,
    cx: &App,
) -> AnyElement {
    let theme = cx.theme();
    v_flex()
        .w_full()
        .max_w(ui::CHAT_MAX_W)
        .mb_2()
        .px_4()
        .py_3()
        .gap_1()
        .rounded(ui::RADIUS_LG)
        .bg(theme.warning.opacity(0.1))
        .border_1()
        .border_color(theme.warning.opacity(0.35))
        .child(
            div()
                .text_size(ui::BODY)
                .font_weight(FontWeight::MEDIUM)
                .child(title),
        )
        .child(
            div()
                .text_size(ui::META)
                .text_color(theme.muted_foreground)
                .child(detail.into()),
        )
        .child(div().pt_1().self_start().child(action))
        .into_any_element()
}

/// A button that opens thread `id`.
fn thread_link(element_id: impl Into<gpui::ElementId>, id: i64, title: SharedString) -> Button {
    Button::new(element_id)
        .ghost()
        .xsmall()
        .label(title)
        .map(ui::no_drag)
        .on_click(move |_, window, cx| {
            crate::workspace::dispatch(Box::new(OpenThread { id }), window, cx)
        })
}

impl Render for ThreadView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let _render = crate::debug_renders::count("ThreadView");
        let header = self.render_header(window, cx);
        let cta = self.needs_binding.then(|| self.render_cta(cx));
        let discard_confirm = self.render_discard_confirm(cx);
        let bypass_confirm = self.render_bypass_confirm(cx);
        let refused = self
            .resume_refused
            .clone()
            .map(|r| self.render_refused(r, cx));
        self.sync_dock_placeholder(window, cx);
        let dock = self.render_dock(window, cx);
        let limit = self.render_limit_banner(cx);
        let composer = self.render_composer(window, cx);
        let failure = self.render_failure(cx);
        let find = self.render_find(cx);
        let strip = self.render_subagent_strip(cx);
        let quote = self.render_quote_button(cx);
        let timeline = self.quote_listeners(div(), cx);
        let theme = cx.theme();
        // T3's working row: how long the turn has run, ticking (`tick_while_working`).
        // A running hook (the account's own hooks) names itself instead, with a hint once it is slow, and a launch
        // that hasn't answered yet names its startup step.
        let (working, hook_hint) = match self.running_hook().filter(|_| self.turn_running()) {
            Some((line, hint)) => (Some(line), hint),
            None => (
                self.startup_line()
                    .filter(|_| (self.starting || self.turn_running()) && self.is_live())
                    .or_else(|| {
                        self.turn_started_ms
                            .filter(|_| self.turn_running())
                            .map(|at| format!("Working for {}", ui::elapsed(ui::now_ms() - at)))
                    }),
                None,
            ),
        };
        let note = self.agent_note(refused.is_some());
        let body = if self.items.is_empty() && !self.read_only {
            let project = &self.project_name;
            div()
                .flex_1()
                .flex()
                .items_center()
                .justify_center()
                .px_6()
                .child(
                    ui::EmptyState::new(match &self.thread {
                        None => format!("New thread in {project}"),
                        Some(_) => "Nothing here yet".to_string(),
                    })
                        .hint(
                            "Write a prompt below. The agent runs in this folder under its bound account.",
                        )
                        .page(),
                )
                .into_any_element()
        } else {
            timeline
                .flex_1()
                .min_h_0()
                .relative()
                .flex()
                .flex_col()
                .children(quote)
                .children(find)
                .child(super::timeline_view::cached(&self.timeline_view))
                .into_any_element()
        };
        let line = |text: SharedString, color: gpui::Hsla| {
            div()
                .w_full()
                .max_w(ui::CHAT_MAX_W)
                .mb_2()
                .px_1()
                .text_size(ui::META)
                .text_color(color)
                .child(text)
        };
        v_flex()
            .id("thread")
            .key_context(CONTEXT)
            .track_focus(&self.focus)
            .on_action(cx.listener(Self::send_prompt))
            .on_action(cx.listener(Self::schedule_prompt))
            .on_action(cx.listener(Self::steer_prompt))
            .on_action(cx.listener(Self::attach_files))
            .on_action(cx.listener(Self::stash_prompt))
            .on_action(cx.listener(Self::toggle_dictation))
            .on_action(cx.listener(Self::cancel_dictation))
            .on_action(cx.listener(Self::history_up))
            .on_action(cx.listener(Self::history_down))
            .on_action(cx.listener(Self::menu_up))
            .on_action(cx.listener(Self::menu_down))
            .on_action(cx.listener(Self::menu_accept))
            .on_action(cx.listener(Self::menu_close))
            .on_action(cx.listener(Self::picker_up))
            .on_action(cx.listener(Self::picker_down))
            .on_action(cx.listener(Self::picker_accept))
            .on_action(cx.listener(Self::picker_close))
            .on_action(cx.listener(Self::picker_jump))
            .on_action(cx.listener(Self::interrupt))
            .on_action(cx.listener(Self::stop))
            .on_action(cx.listener(Self::restart_session))
            .map(|el| self.handoff_actions(el, cx))
            .when(self.dock_step().is_some(), |v| {
                v.on_action(cx.listener(Self::focus_question))
            })
            .on_action(cx.listener(Self::toggle_worktree))
            .on_action(cx.listener(Self::open_changes))
            .on_action(cx.listener(Self::open_worktree_terminal))
            .on_action(cx.listener(Self::discard_worktree))
            .on_action(cx.listener(Self::confirm_discard))
            .on_action(cx.listener(Self::cancel_discard))
            .on_action(cx.listener(Self::find_in_thread))
            .when(!self.is_side_chat(), |v| {
                v.on_action(cx.listener(Self::new_side_chat))
            })
            .on_action(cx.listener(Self::find_next))
            .on_action(cx.listener(Self::find_previous))
            .on_action(cx.listener(Self::close_find))
            .size_full()
            .bg(theme.background)
            .when(!self.is_side_chat(), |v| v.child(header)) // its pane has its own
            .child(body)
            .child(
                v_flex()
                    .w_full()
                    .items_center()
                    .px_6()
                    .pb_4()
                    .children(cta)
                    .children(discard_confirm)
                    .children(bypass_confirm)
                    .children(refused)
                    .children((self.unsaved_events > 0).then(|| {
                        line(
                            format!(
                                "{} events weren't saved (the store refused them). SkyeLab keeps retrying; \
                                 closing this tab or quitting before then loses them.",
                                self.unsaved_events
                            )
                            .into(),
                            ui::ink(theme, theme.warning),
                        )
                        .debug_selector(|| "unsaved-events".into())
                    }))
                    .children(failure)
                    .children(self.error.clone().map(|e| line(e, ui::ink(theme, theme.danger))))
                    .children(note.map(|n| line(n.into(), theme.muted_foreground)))
                    .children(working.map(|w| {
                        line(w.into(), theme.muted_foreground).debug_selector(|| "working-for".into())
                    }))
                    .children(hook_hint.map(|h| {
                        line(h.into(), ui::ink(theme, theme.warning)).debug_selector(|| "slow-hook".into())
                    }))
                    .children(strip)
                    .children(limit)
                    .children(dock)
                    .child(composer),
            )
    }
}

impl ThreadView {
    /// Redraws each second while a turn runs, so "Working for …" ticks. A new turn replaces the last one's timer.
    // shortcut: redraws the whole view each second; give the line its own entity if long timelines draw slowly.
    pub(super) fn tick_while_working(&mut self, cx: &mut Context<Self>) {
        self._tick = Some(cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(std::time::Duration::from_secs(1))
                    .await;
                let working = this.update(cx, |this, cx| {
                    let working = this.starting || this.turn_running();
                    cx.notify();
                    working
                });
                if !matches!(working, Ok(true)) {
                    return;
                }
            }
        }));
    }
}
