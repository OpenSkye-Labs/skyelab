//! How a thread page looks. State and behavior live in `thread.rs`.

use super::thread::{CONTEXT, ThreadView};
use super::thread_items::render_item;
use super::{NewThread, OpenSettings, OpenThread, SelectProject};
use crate::ui;
use gpui::{
    AnyElement, App, Context, FontWeight, IntoElement, SharedString, Window, div, list, prelude::*,
    px,
};
use gpui_component::{
    ActiveTheme as _, Selectable as _, Sizable as _,
    button::{Button, ButtonVariants as _},
    h_flex,
    tooltip::Tooltip,
    v_flex,
};

/// Agent text is untrusted (a repo can steer it), so its links only open web pages, never
/// `file://` paths or app URL schemes.
pub(super) fn opens_in_browser(url: &str) -> bool {
    let lower = url.trim_start().to_ascii_lowercase();
    lower.starts_with("https://") || lower.starts_with("http://")
}

impl ThreadView {
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

    fn render_header(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let folder = self.cwd.file_name().map_or_else(
            || self.cwd.display().to_string(),
            |n| n.to_string_lossy().into_owned(),
        );
        let path = ui::short_path(&self.cwd, dirs::home_dir().as_deref());
        // The window's top row: breadcrumb, then the thread's actions (other features add theirs here), then the
        // dock toggles last.
        super::layout::page_header(cx)
            .child(
                h_flex()
                    .id("thread-breadcrumb")
                    .flex_1()
                    .min_w_0()
                    .gap_2()
                    .text_size(ui::BODY)
                    .font_weight(FontWeight::MEDIUM)
                    .tooltip(move |window, cx| Tooltip::new(path.clone()).build(window, cx))
                    .child(
                        div()
                            .min_w_0()
                            .truncate()
                            .text_color(theme.muted_foreground)
                            .child(folder),
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
                            .child("Delegated by")
                            .child(thread_link("parent-link", id, title))
                            .child(
                                div()
                                    .text_color(theme.muted_foreground.opacity(0.6))
                                    .child("/"),
                            )
                    }))
                    // The title keeps some room when the center is narrow (a right panel open), the folder gives way first.
                    .child(div().min_w(px(72.)).truncate().child(self.title())),
            )
            .child(
                div()
                    .id("provider-badge")
                    .child(self.account_badge().chip()),
            )
            .children(self.render_branch(cx))
            .child(self.render_run(cx))
            .children(self.render_pr_link(cx))
            .child(self.render_worktree_actions(cx))
            .child(super::layout::panel_toggles(cx))
            .into_any_element()
    }

    /// The threads this one's agent delegated, each a link.
    fn render_children(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if self.children.is_empty() {
            return None;
        }
        let theme = cx.theme();
        Some(
            h_flex()
                .flex_wrap()
                .px_4()
                .py_1()
                .gap_1()
                .border_b_1()
                .border_color(theme.border)
                .text_xs()
                .child(
                    div()
                        .text_color(theme.muted_foreground)
                        .child(format!("Delegated threads ({})", self.children.len())),
                )
                .children(self.children.iter().map(|c| {
                    thread_link(
                        SharedString::from(format!("child-{}", c.id)),
                        c.id,
                        c.title.clone(),
                    )
                }))
                .into_any_element(),
        )
    }

    fn render_pr_link(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let url = self.thread.as_ref()?.pr_url.clone()?;
        let label = url.trim_start_matches("https://").to_string();
        Some(
            Button::new("pr-link")
                .outline()
                .xsmall()
                .label(format!("PR · {label}"))
                .tooltip(url.clone())
                .on_click(cx.listener(move |_, _, _, cx| {
                    if opens_in_browser(&url) {
                        cx.open_url(&url);
                    }
                }))
                .into_any_element(),
        )
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
                    .on_click(|_, window, cx| window.dispatch_action(Box::new(OpenSettings), cx)),
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
            div().debug_selector(|| "new-thread-cta".into()).child(
                Button::new("new-thread-cta")
                    .primary()
                    .small()
                    .label("Start a new thread")
                    .on_click(move |_, window, cx| {
                        // The sidebar may show another project by now; start in this thread's.
                        window.dispatch_action(Box::new(SelectProject { id: project_id }), cx);
                        window.dispatch_action(Box::new(NewThread), cx);
                    }),
            ),
            cx,
        )
    }
}

/// What each permission mode is called in the composer, and what it allows.
pub(super) fn mode_label(mode: skyelab_agent::Mode) -> (&'static str, &'static str) {
    use skyelab_agent::Mode;
    match mode {
        Mode::Default => (
            "Ask",
            "The agent asks before commands and edits its rules don't already allow",
        ),
        Mode::Plan => ("Plan", "Read and plan only: no edits, no commands"),
        Mode::AcceptEdits => (
            "Accept edits",
            "File edits in this folder go ahead; anything else asks",
        ),
        Mode::Bypass => (
            "Full access",
            "Nothing asks: any command, any edit (Codex also leaves its sandbox)",
        ),
    }
}

impl ThreadView {
    /// The thread's permission mode, T3's composer "Permissions" toggle: one small button per mode.
    pub(super) fn render_mode_picker(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        h_flex()
            .flex_shrink_0()
            .gap_0p5()
            .child(div().pr_1().child("Permissions"))
            .children(skyelab_agent::Mode::ALL.into_iter().map(|mode| {
                let (label, tip) = mode_label(mode);
                Button::new(SharedString::from(format!("mode-{}", mode.as_str())))
                    .ghost()
                    .xsmall()
                    .label(label)
                    .tooltip(tip)
                    .selected(self.mode == mode)
                    .when(
                        mode == skyelab_agent::Mode::Bypass && self.mode == mode,
                        |b| b.text_color(ui::ink(theme, theme.warning)),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| this.set_mode(mode, cx)))
            }))
            .into_any_element()
    }

    /// Full access was picked: say what it allows and ask before turning it on.
    fn render_bypass_confirm(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.confirm_bypass {
            return None;
        }
        let actions = h_flex()
            .gap_2()
            .child(
                Button::new("confirm-bypass")
                    .danger()
                    .small()
                    .label("Turn on Full access")
                    .on_click(cx.listener(|this, _, _, cx| this.confirm_mode(true, cx))),
            )
            .child(
                Button::new("cancel-bypass")
                    .ghost()
                    .small()
                    .label("Keep asking")
                    .on_click(cx.listener(|this, _, _, cx| this.confirm_mode(false, cx))),
            );
        Some(banner(
            "Give this thread's agent full access?",
            "It will run any command and change any file without asking, under this folder's account. A Codex \
             agent also leaves its sandbox: it can write outside the folder and use the network.",
            div()
                .debug_selector(|| "bypass-confirm".into())
                .child(actions),
            cx,
        ))
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
        .on_click(move |_, window, cx| window.dispatch_action(Box::new(OpenThread { id }), cx))
}

impl Render for ThreadView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.restore_draft(window, cx);
        let header = self.render_header(cx);
        let children = self.render_children(cx);
        let cta = self.needs_binding.then(|| self.render_cta(cx));
        let discard_confirm = self.render_discard_confirm(cx);
        let bypass_confirm = self.render_bypass_confirm(cx);
        let refused = self
            .resume_refused
            .clone()
            .map(|r| self.render_refused(r, cx));
        let composer = self.render_composer(cx);
        let theme = cx.theme();
        let note = if self.starting {
            Some("Starting the agent…")
        } else if self.thread.is_none() || self.session.is_some() || refused.is_some() {
            None
        } else if self.read_only {
            Some("This thread has ended and has no agent session to continue. Start a new thread.")
        } else {
            Some("The agent has ended. Send a message to continue its session.")
        };
        let body = if self.items.is_empty() && !self.read_only {
            let folder = self.cwd.file_name().map_or_else(
                || self.cwd.display().to_string(),
                |n| n.to_string_lossy().into_owned(),
            );
            div()
                .flex_1()
                .flex()
                .items_center()
                .justify_center()
                .px_6()
                .child(
                    ui::EmptyState::new(match &self.thread {
                        None => format!("New thread in {folder}"),
                        Some(_) => "Nothing here yet".to_string(),
                    })
                        .hint(
                            "Write a prompt below. The agent runs in this folder under its bound account.",
                        )
                        .page(),
                )
                .into_any_element()
        } else {
            list(self.list.clone(), cx.processor(render_item))
                .flex_1()
                .py_3()
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
            .on_action(cx.listener(Self::steer_prompt))
            .on_action(cx.listener(Self::attach_files))
            .on_action(cx.listener(Self::menu_up))
            .on_action(cx.listener(Self::menu_down))
            .on_action(cx.listener(Self::menu_accept))
            .on_action(cx.listener(Self::menu_close))
            .on_action(cx.listener(Self::interrupt))
            .on_action(cx.listener(Self::stop))
            .on_action(cx.listener(Self::toggle_worktree))
            .on_action(cx.listener(Self::open_changes))
            .on_action(cx.listener(Self::open_worktree_terminal))
            .on_action(cx.listener(Self::discard_worktree))
            .on_action(cx.listener(Self::confirm_discard))
            .on_action(cx.listener(Self::cancel_discard))
            .size_full()
            .bg(theme.background)
            .child(header)
            .children(children)
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
                    .children(self.error.clone().map(|e| line(e, ui::ink(theme, theme.danger))))
                    .children(note.map(|n| line(n.into(), theme.muted_foreground)))
                    .child(composer),
            )
    }
}
