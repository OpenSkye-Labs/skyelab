use crate::text::connection_text;

#[path = "ask_rows.rs"]
pub mod ask_rows;
#[path = "thread_screen.rs"]
mod thread_screen;
use crate::{
    env_store,
    source::{Connection, ThreadSource},
    thread_rows::{Usage, row_detail, sections},
};
use gpui::{
    AnyElement, App, Context, Entity, FontWeight, ListAlignment, ListState, SharedString,
    Subscription, Task, Window, WindowOptions, div, prelude::*, px,
};
use gpui_component::{
    ActiveTheme as _, Root, Theme,
    button::{Button, ButtonVariants as _},
    h_flex,
    input::{Input, InputEvent, InputState},
    v_flex,
};
use skyelab_agent::timeline::Item;
use skyelab_protocol::{PermissionAnswer, Project, Thread, ThreadEvent};
use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};

/// The smallest thing a thumb taps, in points (Apple's and Material's 44/48).
pub const TAP: f32 = 44.;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    Picker,
    /// Every project's threads, the desktop's flat list.
    Threads,
    Thread {
        thread_id: i64,
    },
}

/// Opens a source for a validated MagicDNS name. The app uses [`source_for`]; tests pass their own.
pub type Connect = Box<dyn Fn(&str) -> std::io::Result<Box<dyn ThreadSource>>>;

/// The real client for every name, pinning nodes in `data_dir`; with the `demo` feature, canned data instead.
pub fn source_for(data_dir: &Path, name: &str) -> std::io::Result<Box<dyn ThreadSource>> {
    #[cfg(feature = "demo")]
    {
        let _ = (data_dir, name);
        Ok(Box::new(crate::source::FakeSource::new()))
    }
    #[cfg(not(feature = "demo"))]
    {
        let mut options = skyelab_client::Options::default();
        options.device_key = Some(skyelab_client::load_or_create_device_key(
            &data_dir.join(DEVICE_KEY_FILE),
        )?);
        Ok(Box::new(crate::remote::RemoteSource::connect(
            name, options, data_dir,
        )?))
    }
}

/// This install's device key, in the app's data directory.
pub const DEVICE_KEY_FILE: &str = "device-key";

pub struct MobileApp {
    data_dir: PathBuf,
    connect_to: Connect,
    /// The environment being used, from Connect until Back to the picker.
    source: Option<Box<dyn ThreadSource>>,
    /// Applies everything the source receives in the background, so it never piles up.
    _pump: Option<Task<()>>,
    pub screen: Screen,
    pub env_input: Entity<InputState>,
    pub composer: Entity<InputState>,
    pub error: Option<SharedString>,
    /// The open thread's timeline rows, one per item. Only the visible ones are built each frame.
    pub events_list: ListState,
    /// The open thread's timeline: the desktop's model, built from its events.
    items: Vec<Item>,
    /// What `items` was built from: (thread, events applied, last seq applied). `None` after a source change,
    /// since thread ids are per machine.
    shown: Option<(i64, usize, Option<i64>)>,
    /// The open thread's usage limit and window readings, from its events.
    usage: Usage,
    /// Rows and turn folds the user opened (tool ids, `fold:<row>`).
    expanded: HashSet<String>,
    /// AskUserQuestion picks not sent yet: request id → per question, the option indexes.
    drafts: HashMap<String, Vec<BTreeSet<usize>>>,
    /// Event rows built so far, so tests can check a frame builds only the visible ones.
    pub rows_built: usize,
    _subscriptions: [Subscription; 3],
}

impl MobileApp {
    pub fn new(
        data_dir: PathBuf,
        connect_to: Connect,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let remembered = env_store::load(&data_dir).unwrap_or_default();
        let env_input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("laptop.tail1234.ts.net")
                .default_value(remembered)
        });
        let composer = cx.new(|cx| InputState::new(window, cx).placeholder("Message the agent…"));
        let enter = |cx: &mut Context<Self>,
                     window: &mut Window,
                     input: &Entity<InputState>,
                     on_enter: fn(&mut Self, &mut Window, &mut Context<Self>)| {
            cx.subscribe_in(input, window, move |this, _, e: &InputEvent, window, cx| {
                if matches!(e, InputEvent::PressEnter { .. }) {
                    on_enter(this, window, cx);
                }
            })
        };
        // Dark or light, as the phone is set.
        let appearance =
            cx.observe_window_appearance(window, |_, window, cx| follow_appearance(window, cx));
        let _subscriptions = [
            enter(cx, window, &env_input, Self::connect),
            enter(cx, window, &composer, Self::send),
            appearance,
        ];
        Self {
            data_dir,
            connect_to,
            source: None,
            _pump: None,
            screen: Screen::Picker,
            env_input,
            composer,
            error: None,
            events_list: ListState::new(0, ListAlignment::Top, px(600.)),
            items: Vec::new(),
            shown: None,
            usage: Usage::default(),
            expanded: HashSet::new(),
            drafts: HashMap::new(),
            rows_built: 0,
            _subscriptions,
        }
    }

    pub fn connect(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let name = self.env_input.read(cx).value();
        self.error = match env_store::validate(&name) {
            Err(why) => Some(why.into()),
            Ok(name) => match env_store::save(&self.data_dir, &name) {
                Err(e) => Some(format!("Couldn't remember the name: {e:#}").into()),
                Ok(()) => match (self.connect_to)(&name) {
                    Err(e) => Some(format!("Couldn't start connecting: {e}").into()),
                    Ok(source) => {
                        self.use_source(source, cx);
                        self.screen = Screen::Threads;
                        None
                    }
                },
            },
        };
        cx.notify();
    }

    fn use_source(&mut self, source: Box<dyn ThreadSource>, cx: &mut Context<Self>) {
        self._pump = source.inbox().map(|inbox| {
            cx.spawn(async move |this, cx| {
                while let Ok(msg) = inbox.recv_async().await {
                    let applied = this.update(cx, |app, cx| {
                        if let Some(source) = app.source.as_mut() {
                            source.apply(msg);
                            // Nothing of a refused machine stays open: back to its (now empty) list.
                            if source.connection().is_untrusted()
                                && matches!(app.screen, Screen::Thread { .. })
                            {
                                app.screen = Screen::Threads;
                            }
                        }
                        cx.notify();
                    });
                    if applied.is_err() {
                        break; // the view is gone
                    }
                }
            })
        });
        self.source = Some(source);
        self.shown = None;
    }

    pub fn open_thread(&mut self, thread_id: i64, window: &mut Window, cx: &mut Context<Self>) {
        if self.screen == Screen::Threads {
            // A draft belongs to the thread it was typed in.
            self.composer
                .update(cx, |s, cx| s.set_value("", window, cx));
            self.error = None;
            if let Some(source) = self.source.as_mut() {
                source.watch(thread_id);
            }
            self.screen = Screen::Thread { thread_id };
            cx.notify();
        }
    }

    pub fn back(&mut self, cx: &mut Context<Self>) {
        self.screen = match self.screen {
            Screen::Picker | Screen::Threads => {
                // Leaving the environment closes its connection.
                self._pump = None;
                self.source = None;
                Screen::Picker
            }
            Screen::Thread { .. } => {
                if let Some(source) = self.source.as_mut() {
                    source.load_threads();
                }
                Screen::Threads
            }
        };
        cx.notify();
    }

    pub fn send(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Screen::Thread { thread_id } = self.screen else {
            return;
        };
        let text = self.composer.read(cx).value().trim().to_string();
        if text.is_empty() {
            return;
        }
        if let Some(source) = self.source.as_mut() {
            source.send_message(thread_id, &text);
        }
        self.composer
            .update(cx, |s, cx| s.set_value("", window, cx));
        cx.notify();
    }

    /// Trusts the node the "isn't the machine you connected to before" warning shows.
    pub fn trust(&mut self, node_id: &str, cx: &mut Context<Self>) {
        if let Some(source) = self.source.as_mut() {
            source.trust(node_id);
            cx.notify();
        }
    }

    /// Answers the open thread's permission request `request_id`. On an ExitPlanMode request, AllowOnce approves
    /// the plan and Deny keeps planning, as on the desktop.
    pub fn answer(&mut self, request_id: &str, decision: PermissionAnswer, cx: &mut Context<Self>) {
        if let (Screen::Thread { thread_id }, Some(source)) = (self.screen, self.source.as_mut()) {
            source.answer_permission(thread_id, request_id, decision);
            cx.notify();
        }
    }

    pub fn stop(&mut self, cx: &mut Context<Self>) {
        if let (Screen::Thread { thread_id }, Some(source)) = (self.screen, self.source.as_mut()) {
            source.interrupt(thread_id);
            cx.notify();
        }
    }

    /// The environment's connection state, `None` on the picker.
    pub fn connection(&self) -> Option<Connection> {
        self.source.as_ref().map(|s| s.connection())
    }

    pub fn projects(&self) -> Vec<Project> {
        self.source
            .as_ref()
            .map(|s| s.projects())
            .unwrap_or_default()
    }

    /// Every project's threads.
    pub fn threads(&self) -> Vec<Thread> {
        self.source
            .as_ref()
            .map(|s| s.threads())
            .unwrap_or_default()
    }

    pub fn thread(&self) -> Option<Thread> {
        let Screen::Thread { thread_id } = self.screen else {
            return None;
        };
        self.threads().into_iter().find(|t| t.id == thread_id)
    }

    pub fn events(&self) -> &[ThreadEvent] {
        match (self.screen, &self.source) {
            (Screen::Thread { thread_id }, Some(source)) => source.events(thread_id),
            _ => &[],
        }
    }

    /// The open thread's timeline, as of the last [`MobileApp::sync_items`] (every frame does one).
    pub fn items(&self) -> &[Item] {
        &self.items
    }
}

/// A section title in the thread list.
fn section_title(title: &'static str, cx: &App) -> impl IntoElement {
    div()
        .pt_3()
        .pb_1()
        .text_xs()
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(cx.theme().muted_foreground)
        .child(title.to_uppercase())
}

impl MobileApp {
    fn picker(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        v_flex()
            .gap_3()
            .child(
                div()
                    .text_color(theme.muted_foreground)
                    .child("The MagicDNS name of the machine running SkyeLab, on your tailnet."),
            )
            .child(Input::new(&self.env_input).h(px(TAP)))
            .when(cfg!(feature = "demo"), |v| {
                v.child(
                    div()
                        .text_sm()
                        .text_color(theme.warning)
                        .child("Demo build: every name opens canned data."),
                )
            })
            .children(
                self.error
                    .clone()
                    .map(|e| div().text_color(theme.danger).child(e)),
            )
            .child(
                Button::new("connect")
                    .primary()
                    .h(px(TAP))
                    .label("Connect")
                    .on_click(cx.listener(|this, _, window, cx| this.connect(window, cx))),
            )
            .into_any_element()
    }

    /// The flat list: section titles, then one tappable row per thread (title and status, then project, account
    /// and preview).
    fn thread_list(&self, cx: &mut Context<Self>) -> AnyElement {
        let projects = self.projects();
        let starts = skyelab_agent::thread_list::day_starts();
        let mut list = v_flex().id("threads").size_full().overflow_y_scroll();
        for (title, threads) in sections(self.threads(), starts) {
            list = list.child(section_title(title, cx));
            for t in threads {
                let id = t.id;
                let project = projects.iter().find(|p| p.id == t.project_id);
                let detail = row_detail(&t, project);
                let theme = cx.theme();
                let dot = match t.status.as_str() {
                    "starting" | "running" => theme.info,
                    "idle" | "done" => theme.success,
                    "failed" => theme.danger,
                    "interrupted" => theme.warning,
                    _ => theme.muted_foreground,
                };
                list =
                    list.child(
                        div()
                            .id(("thread", id as u64))
                            .debug_selector(|| "thread-row".into())
                            .min_h(px(TAP))
                            .py_2()
                            .border_b_1()
                            .border_color(theme.border)
                            .child(
                                h_flex()
                                    .gap_2()
                                    .child(div().size_2().rounded_full().bg(dot))
                                    .child(div().flex_1().text_base().line_clamp(1).child(t.title)),
                            )
                            .child(
                                div()
                                    .pl_4()
                                    .text_sm()
                                    .line_clamp(3)
                                    .text_color(theme.muted_foreground)
                                    .child(detail),
                            )
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.open_thread(id, window, cx)
                            })),
                    );
            }
        }
        list.into_any_element()
    }

    fn title(&self) -> SharedString {
        match self.screen {
            Screen::Picker => "Connect".into(),
            Screen::Threads => "Threads".into(),
            Screen::Thread { .. } => self.thread().map_or("Thread".into(), |t| t.title.into()),
        }
    }
}

impl Render for MobileApp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Keep clear of the status bar, notch, home indicator and (on iOS) the keyboard.
        let safe = window.insets().effective();
        self.sync_items();
        let body = match self.screen {
            Screen::Picker => self.picker(cx),
            Screen::Threads => self.thread_list(cx),
            Screen::Thread { .. } => self.thread_view(cx),
        };
        let connection = self.connection();
        let status = connection.as_ref().map(connection_text);
        // Only a node the hello really named can be trusted.
        let trust = match connection {
            Some(Connection::NodeChanged { now: Some(now), .. }) => Some(now),
            _ => None,
        };
        let source_error = self.source.as_ref().and_then(|s| s.error());
        let theme = cx.theme();
        v_flex()
            .size_full()
            .bg(theme.background)
            .text_color(theme.foreground)
            .pt(safe.top + px(8.))
            .pb(safe.bottom + px(8.))
            .pl(safe.left + px(16.))
            .pr(safe.right + px(16.))
            .gap_2()
            .child(
                h_flex()
                    .gap_2()
                    .when(self.screen != Screen::Picker, |h| {
                        h.child(
                            Button::new("back")
                                .ghost()
                                .debug_selector(|| "back".into())
                                .h(px(TAP))
                                .min_w(px(TAP))
                                .label("‹ Back")
                                .on_click(cx.listener(|this, _, _, cx| this.back(cx))),
                        )
                    })
                    .child(
                        div()
                            .flex_1()
                            .text_xl()
                            .line_clamp(1)
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(self.title()),
                    ),
            )
            .children(status.map(|(text, problem)| {
                div()
                    .text_sm()
                    .text_color(if problem {
                        theme.warning
                    } else {
                        theme.muted_foreground
                    })
                    .child(text)
            }))
            .children(trust.map(|node| {
                Button::new("trust")
                    .warning()
                    .h(px(TAP))
                    .label("Trust it")
                    .on_click(cx.listener(move |this, _, _, cx| this.trust(&node, cx)))
            }))
            .children(source_error.map(|e| div().text_sm().text_color(theme.danger).child(e)))
            .child(v_flex().flex_1().min_h_0().child(body))
    }
}

/// The theme the phone's appearance asks for. Debug builds: `SKYELAB_VERIFY_THEME=dark|light` forces one, so a
/// screenshot can be taken in each without changing the device setting.
fn follow_appearance(window: &mut Window, cx: &mut App) {
    let forced = match std::env::var("SKYELAB_VERIFY_THEME").as_deref() {
        Ok("dark") if cfg!(debug_assertions) => Some(gpui_component::ThemeMode::Dark),
        Ok("light") if cfg!(debug_assertions) => Some(gpui_component::ThemeMode::Light),
        _ => None,
    };
    match forced {
        Some(mode) => Theme::change(mode, Some(window), cx),
        None => Theme::sync_system_appearance(Some(window), cx),
    }
}

/// The app body both platform entry points run. `data_dir` is where the environment name and pinned nodes live.
/// An error means the window never opened; the entry point logs it where that platform's logs go.
pub fn run(data_dir: PathBuf, cx: &mut App) -> anyhow::Result<()> {
    gpui_component::init(cx);
    cx.open_window(WindowOptions::default(), |window, cx| {
        follow_appearance(window, cx);
        let pins = data_dir.clone();
        let connect: Connect = Box::new(move |name| source_for(&pins, name));
        let view = cx.new(|cx| MobileApp::new(data_dir, connect, window, cx));
        cx.new(|cx| Root::new(view, window, cx))
    })?;
    cx.activate(true);
    Ok(())
}
