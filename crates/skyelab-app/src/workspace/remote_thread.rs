//! A thread on another machine: its events arrive through the environment's client (watched, gaps filled by the
//! client), are mapped with the same `thread_model::apply` and drawn with the same item renderer as a local thread.
//! Sending and Stop go to the remote daemon; nothing here touches the local store or launches anything.

use super::thread_items::{ItemsView, render_item};
use super::thread_model::{Item, Record, apply, stamp_done};
use crate::environment::{ConnState, EnvEvent, RemoteEnv};
use gpui::{
    App, AppContext as _, Context, Entity, EventEmitter, FocusHandle, Focusable, FontWeight,
    IntoElement, ListAlignment, ListState, SharedString, Subscription, Window, actions, div, list,
    prelude::*, px,
};
use gpui_component::{
    ActiveTheme as _, Disableable as _, Sizable as _,
    button::{Button, ButtonVariants as _},
    h_flex,
    input::{InputEvent, Textarea, TextareaState},
    v_flex,
};
use skyelab_client::{Client, Pending, Thread, ThreadEvent, WatchId};
use std::collections::HashSet;
use std::rc::Rc;

actions!(skyelab, [SendRemotePrompt, RemoteStop]);

const CONTEXT: &str = "RemoteThread";

pub enum RemoteThreadEvent {
    /// A thread was started on the remote, so its project's thread list is stale.
    Started,
}

pub struct RemoteThreadView {
    focus: FocusHandle,
    env: Entity<RemoteEnv>,
    client: Rc<Client>,
    host: SharedString,
    project_id: i64,
    thread_id: Option<i64>,
    /// This view's own watch, so closing it never ends another view's watch on the same thread.
    watch: Option<WatchId>,
    title: SharedString,
    account: Option<String>,
    pub(super) items: Vec<Item>,
    pub(super) list: ListState,
    expanded: HashSet<String>,
    /// The last seq shown. The client delivers each event once, but watching again replays from the start.
    last_seq: i64,
    skipped: usize,
    pub(super) prompt: Entity<TextareaState>,
    error: Option<SharedString>,
    starting: bool,
    _subs: Vec<Subscription>,
}

impl EventEmitter<RemoteThreadEvent> for RemoteThreadView {}

impl RemoteThreadView {
    /// A view of `thread` (id, title), or a new thread in `project_id` when `None`.
    pub fn new(
        env: Entity<RemoteEnv>,
        project_id: i64,
        thread: Option<(i64, String)>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let prompt = cx.new(|cx| {
            TextareaState::new(window, cx)
                .auto_grow(2, 10)
                .placeholder("Ask the agent on the other machine… (⌘↩ to send)")
        });
        let subs = vec![
            cx.subscribe_in(&prompt, window, |this, _, e: &InputEvent, window, cx| {
                if let InputEvent::PressEnter {
                    secondary: true, ..
                } = e
                {
                    this.send(&SendRemotePrompt, window, cx);
                }
            }),
            cx.subscribe(&env, |this, _, e: &EnvEvent, cx| match e {
                EnvEvent::Thread(event) => this.on_event(event, cx),
                EnvEvent::Untrusted => {}
            }),
            // The header's state and account follow the environment. The account is kept once seen, since
            // the list it comes from changes with the selected project.
            cx.observe(&env, |this, _, cx| {
                if let Some(account) = this.listed_account(cx) {
                    this.account = Some(account);
                }
                cx.notify()
            }),
        ];
        let (thread_id, title) = match thread {
            Some((id, title)) => (Some(id), title.into()),
            None => (None, "New remote thread".into()),
        };
        let account = thread_id.and_then(|id| {
            let env = env.read(cx);
            env.threads.iter().find(|t| t.id == id).map(account_text)
        });
        let client = env.read(cx).client();
        let watch = thread_id.map(|id| client.watch(id, 0));
        Self {
            focus: cx.focus_handle(),
            host: env.read(cx).host.clone().into(),
            client,
            env,
            project_id,
            thread_id,
            watch,
            title,
            account,
            items: Vec::new(),
            list: ListState::new(0, ListAlignment::Top, px(600.)),
            expanded: HashSet::new(),
            last_seq: 0,
            skipped: 0,
            prompt,
            error: None,
            starting: false,
            _subs: subs,
        }
    }

    pub fn thread_id(&self) -> Option<i64> {
        self.thread_id
    }

    pub fn env_badge(&self) -> String {
        format!("Remote · {}", self.host)
    }

    /// The account the remote reports for this thread: "Label · id", or the id when the remote sends no label.
    pub fn account(&self, cx: &App) -> Option<String> {
        self.listed_account(cx).or_else(|| self.account.clone())
    }

    /// The account in the environment's list, which only holds the selected project's threads.
    fn listed_account(&self, cx: &App) -> Option<String> {
        let id = self.thread_id?;
        let env = self.env.read(cx);
        env.threads.iter().find(|t| t.id == id).map(account_text)
    }

    pub fn focus_prompt(&self, window: &mut Window, cx: &mut App) {
        self.prompt.read(cx).focus_handle(cx).focus(window, cx);
    }

    pub(super) fn on_event(&mut self, event: &ThreadEvent, cx: &mut Context<Self>) {
        if Some(event.thread_id) != self.thread_id || event.seq <= self.last_seq {
            return;
        }
        self.last_seq = event.seq;
        let Ok(record) = Record::from_row(&event.kind, &event.payload_json) else {
            // One unreadable event (a newer format) must not hide the rest.
            self.skipped += 1;
            self.error = Some(format!("{} events couldn't be read", self.skipped).into());
            return cx.notify();
        };
        let old = self.items.len();
        if let Some(first) = apply(&mut self.items, &record) {
            stamp_done(&mut self.items, event.created_at);
            self.list.splice(first..old, self.items.len() - first);
            cx.notify();
        }
    }

    #[cfg(test)]
    /// Types `text` into the composer and sends it, as the Send button does.
    pub fn submit(&mut self, text: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.prompt
            .update(cx, |p, cx| p.set_value(text, window, cx));
        self.send(&SendRemotePrompt, window, cx);
    }

    fn send(&mut self, _: &SendRemotePrompt, window: &mut Window, cx: &mut Context<Self>) {
        let text = self.prompt.read(cx).value().trim().to_string();
        if text.is_empty() || self.starting {
            return;
        }
        // The client would refuse it anyway (not connected); say so and keep what was typed.
        if self.env.read(cx).state != ConnState::Connected {
            self.error = Some(format!("Not connected to {}. Your text is kept.", self.host).into());
            return cx.notify();
        }
        self.error = None;
        self.prompt.update(cx, |p, cx| p.set_value("", window, cx));
        // The prompt shows when the remote stores it and its event comes back, so it is never shown twice.
        match self.thread_id {
            Some(id) => {
                let pending = self.client.send_message(id, text.clone());
                self.await_reply(pending, Some(text), window, cx, |_, _, _| {});
            }
            None => {
                self.starting = true;
                let pending = self.client.start_thread(self.project_id, text.clone());
                let title: String = text
                    .lines()
                    .next()
                    .unwrap_or_default()
                    .chars()
                    .take(60)
                    .collect();
                self.await_reply(pending, Some(text), window, cx, move |this, id, cx| {
                    this.thread_id = Some(id);
                    this.title = title.into();
                    this.watch = Some(this.client.watch(id, 0));
                    cx.emit(RemoteThreadEvent::Started);
                });
            }
        }
        cx.notify();
    }

    pub(super) fn stop(&mut self, _: &RemoteStop, window: &mut Window, cx: &mut Context<Self>) {
        // Like send: never to a machine that isn't connected and trusted.
        if self.env.read(cx).state != ConnState::Connected {
            return;
        }
        if let Some(id) = self.thread_id {
            let pending = self.client.interrupt(id);
            self.await_reply(pending, None, window, cx, |_, _, _| {});
        }
    }

    /// Applies the reply when it comes, never waiting on the UI thread. On an error it shows why and puts `text`
    /// back in the composer, unless the user has typed something new meanwhile.
    fn await_reply<T: 'static>(
        &mut self,
        pending: Pending<T>,
        text: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
        on_ok: impl FnOnce(&mut Self, T, &mut Context<Self>) + 'static,
    ) {
        cx.spawn_in(window, async move |this, cx| {
            let reply = pending.recv_async().await;
            let _ = this.update_in(cx, |this, window, cx| {
                this.starting = false;
                match reply {
                    Ok(value) => on_ok(this, value, cx),
                    Err(e) => {
                        this.error = Some(e.to_string().into());
                        if let Some(text) = text
                            && this.prompt.read(cx).value().is_empty()
                        {
                            this.prompt
                                .update(cx, |p, cx| p.set_value(text, window, cx));
                        }
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn render_header(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let theme = cx.theme();
        let env = self.env.read(cx);
        let state = env.state.clone();
        let account = self.account(cx);
        let pill = |child: SharedString, dot| {
            h_flex()
                .gap_1p5()
                .px_2()
                .py_0p5()
                .rounded_md()
                .border_1()
                .border_color(theme.border)
                .text_xs()
                .child(div().size(px(6.)).rounded_full().bg(dot))
                .child(child)
        };
        super::layout::page_header(cx)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_sm()
                    .font_weight(FontWeight::SEMIBOLD)
                    .truncate()
                    .child(self.title.clone()),
            )
            .child(pill(self.env_badge().into(), state.color(theme)))
            .children(account.map(|a| pill(a.into(), theme.success)))
            .child(super::layout::panel_toggles(cx))
            .into_any_element()
    }
}

/// The same "Label · id" a local thread's badge shows.
fn account_text(t: &Thread) -> String {
    match &t.account_label {
        Some(label) => format!("{label} · {}", t.account_id),
        None => t.account_id.clone(),
    }
}

impl ItemsView for RemoteThreadView {
    fn items(&self) -> &[Item] {
        &self.items
    }
    fn is_expanded(&self, tool_id: &str) -> bool {
        self.expanded.contains(tool_id)
    }
    fn toggle_tool(&mut self, tool_id: &str, cx: &mut Context<Self>) {
        if !self.expanded.remove(tool_id) {
            self.expanded.insert(tool_id.to_owned());
        }
        if let Some(rows) = super::timeline_fold::rows_of(&self.items, tool_id) {
            let n = rows.len();
            self.list.splice(rows, n);
        }
        cx.notify();
    }
    fn interrupt_turn(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.stop(&RemoteStop, window, cx)
    }
    fn can_answer(&self, cx: &App) -> bool {
        self.thread_id.is_some() && self.env.read(cx).state == ConnState::Connected
    }
    fn answer(
        &mut self,
        id: &str,
        decision: skyelab_agent::Decision,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        use skyelab_client::PermissionAnswer as A;
        let (Some(thread), Some(answer)) = (
            self.thread_id,
            match decision {
                skyelab_agent::Decision::AllowOnce => Some(A::AllowOnce),
                skyelab_agent::Decision::AllowThread => Some(A::AllowThread),
                skyelab_agent::Decision::Deny => Some(A::Deny),
                skyelab_agent::Decision::Withdrawn => None,
            },
        ) else {
            return;
        };
        if !self.can_answer(cx) {
            return;
        }
        // The row updates when the agent's resolved event comes back from the other machine.
        let pending = self.client.answer_permission(thread, id, answer);
        self.await_reply(pending, None, window, cx, |_, _, _| {});
    }
}

impl Focusable for RemoteThreadView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl RemoteThreadView {
    /// The machine changed: stop following the thread, so the client stops fetching its events.
    pub(super) fn closed(&mut self, _: &mut Context<Self>) {
        if let Some(watch) = self.watch.take() {
            self.client.unwatch(watch);
        }
    }
}

impl Drop for RemoteThreadView {
    /// Backstop for a view dropped without `closed` (the window closing).
    fn drop(&mut self) {
        if let Some(watch) = self.watch.take() {
            self.client.unwatch(watch);
        }
    }
}

impl Render for RemoteThreadView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let header = self.render_header(cx);
        let theme = cx.theme();
        let connected = self.env.read(cx).state == ConnState::Connected;
        let host = self.env.read(cx).host.clone();
        let body = if self.items.is_empty() {
            div()
                .flex_1()
                .flex()
                .items_center()
                .justify_center()
                .text_sm()
                .text_color(theme.muted_foreground)
                .child(if self.thread_id.is_some() {
                    "Loading the thread from the other machine…"
                } else {
                    "Write a prompt below. It runs on the other machine, under its bound account there."
                })
                .into_any_element()
        } else {
            list(self.list.clone(), cx.processor(render_item))
                .flex_1()
                .py_2()
                .into_any_element()
        };
        v_flex()
            .id("remote-thread")
            .key_context(CONTEXT)
            .track_focus(&self.focus)
            .on_action(cx.listener(Self::send))
            .on_action(cx.listener(Self::stop))
            .size_full()
            .bg(theme.background)
            .child(header)
            .child(body)
            .children(self.error.clone().map(|e| {
                div()
                    .mx_4()
                    .mb_2()
                    .text_xs()
                    .text_color(crate::ui::ink(theme, theme.danger))
                    .child(e)
            }))
            .child(
                h_flex()
                    .px_4()
                    .pb_3()
                    .gap_2()
                    .items_end()
                    .child(div().flex_1().child(Textarea::new(&self.prompt)))
                    // Interrupt sits with the composer, as on a local thread; the header has no stop.
                    .when(self.thread_id.is_some(), |this| {
                        this.child(
                            Button::new("remote-stop")
                                .ghost()
                                .small()
                                .label("Interrupt")
                                .tooltip("Interrupt the agent on the other machine")
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.stop(&RemoteStop, window, cx)
                                })),
                        )
                    })
                    .child(
                        Button::new("remote-send")
                            .primary()
                            .small()
                            .label("Send")
                            .disabled(self.starting || !connected)
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.send(&SendRemotePrompt, window, cx)
                            })),
                    ),
            )
            // The composer footer's machine, here the one the thread runs on.
            .child(
                h_flex()
                    .id("remote-composer-footer")
                    .px_5()
                    .pb_2()
                    .gap_1p5()
                    .text_size(crate::ui::META)
                    .text_color(theme.muted_foreground)
                    .child(gpui_component::Icon::new(gpui_kit_assets::IconName::Laptop).xsmall())
                    .child(host)
                    .children(self.account.clone().map(|a| format!("· {a}"))),
            )
    }
}
