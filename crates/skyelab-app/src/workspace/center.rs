//! The center shows one page at a time, like T3's routes: Welcome, a thread (or a new-thread draft), Settings, or a
//! remote thread. Opening a thread switches the page; it never adds a tab. Thread views stay alive while another page
//! shows, so a running agent keeps running.

use super::remote_thread::RemoteThreadView;
use super::thread::ThreadView;
use super::{GoBack, GoForward, OpenSettingsSection, OpenThread, Workspace};
use crate::settings::Section;
use gpui::{
    AnyView, App, Context, Entity, FocusHandle, Focusable, IntoElement, MouseButton,
    NavigationDirection, WeakEntity, Window, div, prelude::*,
};

/// What the center shows. Threads are weak: closing one (archive, delete) drops it even if history names it.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Page {
    Welcome,
    Settings(Section),
    Thread(WeakEntity<ThreadView>),
    RemoteThread(WeakEntity<RemoteThreadView>),
}

impl Page {
    /// How settings.toml remembers the page: `thread:<id>`, `settings:<section>` or `welcome`. A draft (no row yet)
    /// and a remote thread (another machine's id) come back as Welcome.
    pub(crate) fn key(&self, cx: &App) -> String {
        match self {
            Page::Settings(section) => format!("settings:{}", section.key()),
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

/// The dock's only center panel: it hosts the current page and draws no tab bar.
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
    /// The thread the center shows, if it shows one.
    pub(crate) fn current_thread(&self) -> Option<Entity<ThreadView>> {
        match &self.page {
            Page::Thread(view) => view.upgrade(),
            _ => None,
        }
    }

    /// Shows `page`, remembering the current one for Back.
    pub(super) fn show(&mut self, page: Page, window: &mut Window, cx: &mut Context<Self>) {
        if page != self.page {
            self.back.push(std::mem::replace(&mut self.page, page));
            self.forward.clear();
        }
        self.apply_page(window, cx);
    }

    /// A page that can still be shown: its view wasn't closed.
    fn alive(&self, page: &Page) -> bool {
        match page {
            // Settings is made on demand, so it can always show.
            Page::Welcome | Page::Settings(_) => true,
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

    /// Puts the current page in the center and focuses it.
    pub(super) fn apply_page(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.alive(&self.page) {
            self.page = Page::Welcome;
        }
        let view: AnyView = match self.page.clone() {
            Page::Welcome => {
                self.welcome.read(cx).focus_handle(cx).focus(window, cx);
                self.welcome.clone().into()
            }
            Page::Settings(section) => {
                let settings = self.ensure_settings(window, cx);
                settings.update(cx, |s, cx| {
                    s.section = section;
                    cx.notify();
                });
                settings.read(cx).focus_handle(cx).focus(window, cx);
                settings.into()
            }
            Page::Thread(view) => {
                let view = view.upgrade().expect("alive() checked it");
                self.unseen.remove(&view.entity_id());
                view.update(cx, |v, cx| v.focus_prompt(window, cx));
                view.into()
            }
            Page::RemoteThread(view) => {
                let view = view.upgrade().expect("alive() checked it");
                view.update(cx, |v, cx| v.focus_prompt(window, cx));
                view.into()
            }
        };
        self.center.update(cx, |c, cx| {
            c.content = view;
            cx.notify();
        });
        self.refresh_sidebar(cx);
        self.schedule_layout_save(window, cx);
        cx.notify();
    }

    /// Drops a thread's view: its agent stops and its row ends `stopped`, as closing its tab used to. The page falls
    /// back to Welcome if it showed the thread.
    // shortcut: nothing in the app closes a thread yet; the sidebar's archive/delete (package D) will call this, then
    // drop the cfg. Its tests run the fake agent, a shell script, so they are unix-only.
    #[cfg(all(test, unix))]
    pub(crate) fn close_thread(
        &mut self,
        view: &Entity<ThreadView>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_threads.retain(|v| v != view);
        self.unseen.remove(&view.entity_id());
        view.update(cx, |v, _| v.close());
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
        self.show(Page::Settings(a.section), window, cx);
    }

    /// Restores the page settings.toml remembered (see [`Page::key`]).
    pub(super) fn restore_page(&mut self, key: &str, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(id) = key.strip_prefix("thread:").and_then(|id| id.parse().ok()) {
            self.open_thread(&OpenThread { id }, window, cx);
        } else if let Some(section) = key.strip_prefix("settings:").and_then(Section::parse) {
            self.show(Page::Settings(section), window, cx);
        }
    }
}
