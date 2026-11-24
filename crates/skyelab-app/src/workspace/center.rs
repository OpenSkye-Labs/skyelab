//! The center shows one page at a time, like T3's routes: Welcome, a thread (or a new-thread draft), a remote
//! thread, or the Pull requests or Issues page. Settings is a modal over it (`settings_tab.rs`). Opening a thread
//! switches the page; it never adds a tab. Thread views stay alive while another page shows, so a running agent
//! keeps running.

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
    /// A multi-model send's threads side by side, by comparison group.
    Compare(i64),
    /// The Pull requests or Issues page.
    Github(super::gh_pages::Kind),
}

impl Page {
    /// How settings.toml remembers the page: `thread:<id>`, `settings:<section>`, `github:<page>` or `welcome`. A draft (no row yet)
    /// and a remote thread (another machine's id) come back as Welcome.
    pub(crate) fn key(&self, cx: &App) -> String {
        match self {
            Page::Github(kind) => format!("github:{}", kind.key()),
            Page::Thread(view) => view
                .upgrade()
                .and_then(|v| {
                    v.read(cx)
                        .thread
                        .as_ref()
                        .map(|t| format!("thread:{}", t.id))
                })
                .unwrap_or_else(|| "welcome".into()),
            Page::Welcome | Page::RemoteThread(_) | Page::Compare(_) => "welcome".into(),
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
        let _render = crate::debug_renders::count("Center");
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
            .child(page(&self.content))
    }
}

/// The page, cached (the dock draws the center uncached, `center_skin.rs`) unless it is a thread: its composer's
/// caret blink would miss the cache and rebuild every timeline row, so the thread page caches its timeline itself.
fn page(content: &AnyView) -> gpui::AnyElement {
    if content.clone().downcast::<ThreadView>().is_ok() {
        content.clone().into_any_element()
    } else {
        content
            .clone()
            .cached(gpui::StyleRefinement::default().size_full())
            .into_any_element()
    }
}

/// Each window's thread page (`None` while it shows another page), for what only the selected thread may do when
/// nothing has the focus: several shown threads (a side chat, Compare) can get a question at once.
#[derive(Default)]
pub(crate) struct ShownThread(
    pub std::collections::HashMap<gpui::WindowId, Option<gpui::EntityId>>,
);

impl gpui::Global for ShownThread {}

impl Workspace {
    pub(crate) fn current_thread(&self) -> Option<Entity<ThreadView>> {
        match &self.page {
            Page::Thread(view) => view.upgrade(),
            _ => None,
        }
    }

    pub(super) fn show(&mut self, page: Page, window: &mut Window, cx: &mut Context<Self>) {
        self.close_settings(window, cx); // going to a page (a thread link in Settings) leaves the modal
        if page != self.page {
            self.back.push(std::mem::replace(&mut self.page, page));
            self.forward.clear();
        }
        self.apply_page(window, cx);
    }

    pub(super) fn alive(&self, page: &Page, cx: &App) -> bool {
        match page {
            Page::Welcome => true,
            // Made on demand, so they can always show; but they list this machine's projects, never while another
            // machine is shown (Back, Forward, a restore).
            Page::Github(_) => !self.is_remote(),
            Page::Thread(view) => view
                .upgrade()
                .is_some_and(|v| self.open_threads.contains(&v)),
            Page::RemoteThread(view) => view
                .upgrade()
                .is_some_and(|v| self.env.tabs.iter().any(|(t, _)| *t == v)),
            Page::Compare(group) => self.group_alive(*group, cx),
        }
    }

    pub(super) fn go_back(&mut self, _: &GoBack, window: &mut Window, cx: &mut Context<Self>) {
        while let Some(page) = self.back.pop() {
            if self.alive(&page, cx) {
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
            if self.alive(&page, cx) {
                self.back.push(std::mem::replace(&mut self.page, page));
                return self.apply_page(window, cx);
            }
        }
    }

    pub(super) fn apply_page(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.alive(&self.page, cx) {
            self.page = Page::Welcome;
        }
        let shown = match &self.page {
            Page::Thread(view) => Some(view.entity_id()),
            _ => None,
        };
        cx.default_global::<ShownThread>()
            .0
            .insert(window.window_handle().window_id(), shown);
        let view: AnyView = match self.page.clone() {
            Page::Welcome => {
                self.welcome.read(cx).focus_handle(cx).focus(window, cx);
                self.welcome.clone().into()
            }
            Page::Github(kind) => self.gh_page(kind, window, cx).into(),
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
            Page::Compare(group) => self.compare_page(group, window, cx).into(),
        };
        self.sync_terminals(window, cx);
        self.refresh_details(cx);
        self.center.update(cx, |c, cx| {
            c.content = view;
            cx.notify();
        });
        self.mark_seen(window, cx);
        self.seen_linked(cx);
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
        } else if let Some(kind) = key
            .strip_prefix("github:")
            .and_then(super::gh_pages::Kind::parse)
        {
            self.show(Page::Github(kind), window, cx);
        }
        // An older file's `settings:<section>`: Settings is a modal now, so the window opens on Welcome.
    }
}
