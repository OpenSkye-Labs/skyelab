//! The center shows one page at a time, like T3's routes: Welcome, a thread (or a new-thread draft), or a remote
//! thread. Settings is a modal over it (`settings_tab.rs`). Opening a thread switches the page; it never adds a tab. Thread views stay alive while another page
//! shows, so a running agent keeps running.

use super::remote_thread::RemoteThreadView;
use super::thread::ThreadView;
use super::{GoBack, GoForward, OpenSettingsSection, OpenThread, Workspace};
use gpui::{
    AnyView, App, Context, Entity, FocusHandle, Focusable, IntoElement, MouseButton,
    NavigationDirection, WeakEntity, Window, div, prelude::*,
};

/// What the center shows. Threads are weak, so history never keeps a closed view alive.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Page {
    Welcome,
    Thread(WeakEntity<ThreadView>),
    RemoteThread(WeakEntity<RemoteThreadView>),
}

impl Page {
    /// How settings.toml remembers the page: `thread:<id>`, `settings:<section>` or `welcome`. A draft (no row yet)
    /// and a remote thread (another machine's id) come back as Welcome.
    pub(crate) fn key(&self, cx: &App) -> String {
        match self {
            Page::Thread(view) => view
                .upgrade()
                .and_then(|v| {
                    v.read(cx)
                        .thread
                        .as_ref()
                        .map(|t| format!("thread:{}", t.id))
                })
                .unwrap_or_else(|| "welcome".into()),
            Page::Welcome | Page::RemoteThread(_) => "welcome".into(),
        }
    }
}

/// The dock's only center panel; it hosts the current page.
pub struct Center {
    pub(super) content: AnyView,
    focus: FocusHandle,
}

impl Center {
    pub(super) fn new(content: AnyView, cx: &mut Context<Self>) -> Self {
        Self {
            content,
            focus: cx.focus_handle(),
        }
    }
}

impl Focusable for Center {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for Center {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("center")
            .size_full()
            // Mouse back/forward buttons, and the swipes macOS reports as those buttons.
            .on_mouse_down(
                MouseButton::Navigate(NavigationDirection::Back),
                |_, window, cx| window.dispatch_action(Box::new(GoBack), cx),
            )
            .on_mouse_down(
                MouseButton::Navigate(NavigationDirection::Forward),
                |_, window, cx| window.dispatch_action(Box::new(GoForward), cx),
            )
            .child(self.content.clone())
    }
}

impl Workspace {
    pub(crate) fn current_thread(&self) -> Option<Entity<ThreadView>> {
        match &self.page {
            Page::Thread(view) => view.upgrade(),
            _ => None,
        }
    }

    pub(super) fn show(&mut self, page: Page, window: &mut Window, cx: &mut Context<Self>) {
        if page != self.page {
            self.back.push(std::mem::replace(&mut self.page, page));
            self.forward.clear();
        }
        self.apply_page(window, cx);
    }

    fn alive(&self, page: &Page) -> bool {
        match page {
            Page::Welcome => true,
            Page::Thread(view) => view
                .upgrade()
                .is_some_and(|v| self.open_threads.contains(&v)),
            Page::RemoteThread(view) => view
                .upgrade()
                .is_some_and(|v| self.env.tabs.iter().any(|(t, _)| *t == v)),
        }
    }

    pub(super) fn go_back(&mut self, _: &GoBack, window: &mut Window, cx: &mut Context<Self>) {
        while let Some(page) = self.back.pop() {
            if self.alive(&page) {
                self.forward.push(std::mem::replace(&mut self.page, page));
                return self.apply_page(window, cx);
            }
        }
    }

    pub(super) fn go_forward(
        &mut self,
        _: &GoForward,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        while let Some(page) = self.forward.pop() {
            if self.alive(&page) {
                self.back.push(std::mem::replace(&mut self.page, page));
                return self.apply_page(window, cx);
            }
        }
    }

    pub(super) fn apply_page(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.alive(&self.page) {
            self.page = Page::Welcome;
        }
        let view: AnyView = match self.page.clone() {
            Page::Welcome => {
                self.welcome.read(cx).focus_handle(cx).focus(window, cx);
                self.welcome.clone().into()
            }
            Page::Thread(view) => {
                let view = view.upgrade().expect("alive() checked it");
                view.update(cx, |v, cx| {
                    v.attach(window, cx);
                    v.focus_prompt(window, cx)
                });
                view.into()
            }
            Page::RemoteThread(view) => {
                let view = view.upgrade().expect("alive() checked it");
                view.update(cx, |v, cx| v.focus_prompt(window, cx));
                view.into()
            }
        };
        self.sync_terminals(window, cx);
        self.refresh_details(cx);
        self.center.update(cx, |c, cx| {
            c.content = view;
            cx.notify();
        });
        self.mark_seen(window, cx);
        self.refresh_sidebar(cx);
        self.schedule_layout_save(window, cx);
        cx.notify();
    }

    /// Drops a thread's view: its agent stops at once (the last drawn frames may hold the view a moment longer) and
    /// its row ends `stopped`. The page falls back to Welcome if it showed the thread.
    pub(crate) fn close_thread(
        &mut self,
        view: &Entity<ThreadView>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        view.update(cx, |v, _| v.close());
        self.close_side_chats(view, window, cx);
        self.unshare_thread(view, cx);
        self.forget_thread(view, window, cx);
    }

    /// Stops following a closed thread's view in this window.
    pub(super) fn forget_thread(
        &mut self,
        view: &Entity<ThreadView>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_threads.retain(|v| v != view);
        view.update(cx, |v, _| v.lists.remove(window.window_handle()));
        self.thread_subs.remove(&view.entity_id());
        self.waiters.retain(|_, (id, _)| *id != view.entity_id());
        self.orphan_terminals(view.entity_id(), window, cx);
        if self.page == Page::Thread(view.downgrade()) {
            self.page = Page::Welcome;
            self.apply_page(window, cx);
        }
        self.refresh_sidebar(cx);
    }

    pub(super) fn open_settings_section(
        &mut self,
        a: &OpenSettingsSection,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_settings_at(a.section, window, cx);
    }

    pub(super) fn restore_page(&mut self, key: &str, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(id) = key.strip_prefix("thread:").and_then(|id| id.parse().ok()) {
            self.open_thread(&OpenThread { id }, window, cx);
        }
        // An older file's `settings:<section>`: Settings is a modal now, so the window opens on Welcome.
    }
}
