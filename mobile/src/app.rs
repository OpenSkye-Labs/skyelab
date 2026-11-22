use crate::text::connection_text;
pub use crate::text::event_text;
use crate::{
    env_store,
    source::{Connection, ThreadSource},
};
use gpui::{
    AnyElement, App, Context, Entity, FontWeight, ListAlignment, ListState, SharedString,
    Subscription, Task, Window, WindowOptions, div, list, prelude::*, px,
};
use gpui_component::{
    ActiveTheme as _, Root,
    button::{Button, ButtonVariants as _},
    h_flex,
    input::{Input, InputEvent, InputState},
    v_flex,
};
use skyelab_protocol::{Project, Thread, ThreadEvent};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    Picker,
    Projects,
    Threads { project_id: i64 },
    Thread { project_id: i64, thread_id: i64 },
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
        // Options::default() keeps whatever transport skyelab-client defaults to (wss:// once it lands).
        Ok(Box::new(crate::remote::RemoteSource::connect(
            name,
            skyelab_client::Options::default(),
            data_dir,
        )?))
    }
}

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
    /// The open thread's rows. Only the visible ones are built each frame.
    pub events_list: ListState,
    /// What `events_list` was last synced to: (thread, event count, last seq). `None` after a source change,
    /// since thread ids are per machine.
    shown: Option<(i64, usize, Option<i64>)>,
    /// Event rows built so far, so tests can check a frame builds only the visible ones.
    pub rows_built: usize,
    _enter: [Subscription; 2],
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
        let _enter = [
            enter(cx, window, &env_input, Self::connect),
            enter(cx, window, &composer, Self::send),
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
            shown: None,
            rows_built: 0,
            _enter,
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
                        self.screen = Screen::Projects;
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
                            // Nothing of a refused machine stays open: back to its (now empty) projects.
                            if source.connection().is_untrusted()
                                && matches!(
                                    app.screen,
                                    Screen::Threads { .. } | Screen::Thread { .. }
                                )
                            {
                                app.screen = Screen::Projects;
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

    pub fn open_project(&mut self, project_id: i64, cx: &mut Context<Self>) {
        if let Some(source) = self.source.as_mut() {
            source.load_threads(project_id);
        }
        self.screen = Screen::Threads { project_id };
        cx.notify();
    }

    pub fn open_thread(&mut self, thread_id: i64, window: &mut Window, cx: &mut Context<Self>) {
        if let Screen::Threads { project_id } = self.screen {
            // A draft belongs to the thread it was typed in.
            self.composer
                .update(cx, |s, cx| s.set_value("", window, cx));
            if let Some(source) = self.source.as_mut() {
                source.watch(thread_id);
            }
            self.screen = Screen::Thread {
                project_id,
                thread_id,
            };
            cx.notify();
        }
    }

    pub fn back(&mut self, cx: &mut Context<Self>) {
        self.screen = match self.screen {
            Screen::Picker | Screen::Projects => {
                // Leaving the environment closes its connection.
                self._pump = None;
                self.source = None;
                Screen::Picker
            }
            Screen::Threads { .. } => Screen::Projects,
            Screen::Thread { project_id, .. } => Screen::Threads { project_id },
        };
        cx.notify();
    }

    pub fn send(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Screen::Thread { thread_id, .. } = self.screen else {
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

    pub fn stop(&mut self, cx: &mut Context<Self>) {
        if let (Screen::Thread { thread_id, .. }, Some(source)) =
            (self.screen, self.source.as_mut())
        {
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

    /// Threads of the open project.
    pub fn threads(&self) -> Vec<Thread> {
        match (self.screen, &self.source) {
            (Screen::Threads { project_id } | Screen::Thread { project_id, .. }, Some(source)) => {
                source.threads(project_id)
            }
            _ => Vec::new(),
        }
    }

    pub fn thread(&self) -> Option<Thread> {
        let Screen::Thread { thread_id, .. } = self.screen else {
            return None;
        };
        self.threads().into_iter().find(|t| t.id == thread_id)
    }

    pub fn events(&self) -> &[ThreadEvent] {
        match (self.screen, &self.source) {
            (Screen::Thread { thread_id, .. }, Some(source)) => source.events(thread_id),
            _ => &[],
        }
    }

    /// Brings `events_list` up to date with the open thread's events. Events only ever get inserted in seq order
    /// or cleared (a re-watch), so if the last row shown is still at its index, every new event came after it and
    /// is spliced on the end; anything else re-lays out the list.
    fn sync_events_list(&mut self) {
        let Screen::Thread { thread_id, .. } = self.screen else {
            return;
        };
        let events = self.events();
        let (n, last) = (events.len(), events.last().map(|e| e.seq));
        let shown_len = match self.shown {
            Some((shown_thread, shown_len, shown_last))
                if shown_thread == thread_id
                    && shown_len <= n
                    && (shown_len == 0 || Some(events[shown_len - 1].seq) == shown_last) =>
            {
                Some(shown_len)
            }
            _ => None,
        };
        match shown_len {
            None => self.events_list.reset(n),
            Some(len) if n > len => self.events_list.splice(len..len, n - len),
            Some(_) => {}
        }
        self.shown = Some((thread_id, n, last));
    }

    fn event_row(&mut self, ix: usize, cx: &mut Context<Self>) -> AnyElement {
        self.rows_built += 1;
        let theme = cx.theme();
        let Some(e) = self.events().get(ix) else {
            return div().into_any_element();
        };
        v_flex()
            .py_2()
            .child(
                div()
                    .text_xs()
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(theme.muted_foreground)
                    .child(e.kind.to_uppercase()),
            )
            .child(event_text(e))
            .into_any_element()
    }
}

fn row(
    id: (&'static str, i64),
    title: String,
    detail: String,
    cx: &App,
) -> gpui::Stateful<gpui::Div> {
    let theme = cx.theme();
    div()
        .id((id.0, id.1 as u64))
        .py_3()
        .border_b_1()
        .border_color(theme.border)
        .child(div().text_lg().child(title))
        .child(
            div()
                .text_sm()
                .text_color(theme.muted_foreground)
                .child(detail),
        )
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
            .child(Input::new(&self.env_input))
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
                    .label("Connect")
                    .on_click(cx.listener(|this, _, window, cx| this.connect(window, cx))),
            )
            .into_any_element()
    }

    fn project_list(&self, cx: &mut Context<Self>) -> AnyElement {
        let rows = self.projects().into_iter().map(|p| {
            let id = p.id;
            row(("project", id), p.name, p.path, cx)
                .on_click(cx.listener(move |this, _, _, cx| this.open_project(id, cx)))
        });
        v_flex().children(rows).into_any_element()
    }

    fn thread_list(&self, cx: &mut Context<Self>) -> AnyElement {
        let rows = self.threads().into_iter().map(|t| {
            let id = t.id;
            row(
                ("thread", id),
                t.title,
                format!("{} · {}", t.account_id, t.status),
                cx,
            )
            .on_click(cx.listener(move |this, _, window, cx| this.open_thread(id, window, cx)))
        });
        v_flex().children(rows).into_any_element()
    }

    fn thread_view(&self, cx: &mut Context<Self>) -> AnyElement {
        let events = list(
            self.events_list.clone(),
            cx.processor(|this, ix, _, cx| this.event_row(ix, cx)),
        )
        .flex_1()
        .min_h_0();
        let theme = cx.theme();
        v_flex()
            .flex_1()
            .min_h_0()
            .gap_2()
            .child(
                div().text_sm().text_color(theme.muted_foreground).children(
                    self.thread()
                        .map(|t| format!("{} · {}", t.account_id, t.status)),
                ),
            )
            .child(events)
            .child(
                h_flex()
                    .gap_2()
                    .child(div().flex_1().child(Input::new(&self.composer)))
                    .child(
                        Button::new("send")
                            .primary()
                            .label("Send")
                            .on_click(cx.listener(|this, _, window, cx| this.send(window, cx))),
                    )
                    .child(
                        Button::new("stop")
                            .danger()
                            .label("Stop")
                            .on_click(cx.listener(|this, _, _, cx| this.stop(cx))),
                    ),
            )
            .into_any_element()
    }

    fn title(&self) -> SharedString {
        match self.screen {
            Screen::Picker => "Connect".into(),
            Screen::Projects => "Projects".into(),
            Screen::Threads { project_id } => self
                .projects()
                .into_iter()
                .find(|p| p.id == project_id)
                .map_or("Threads".into(), |p| p.name.into()),
            Screen::Thread { .. } => self.thread().map_or("Thread".into(), |t| t.title.into()),
        }
    }
}

impl Render for MobileApp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Keep clear of the status bar, notch, home indicator and (on iOS) the keyboard.
        let safe = window.insets().effective();
        self.sync_events_list();
        let body = match self.screen {
            Screen::Picker => self.picker(cx),
            Screen::Projects => self.project_list(cx),
            Screen::Threads { .. } => self.thread_list(cx),
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
            .gap_3()
            .child(
                h_flex()
                    .gap_2()
                    .when(self.screen != Screen::Picker, |h| {
                        h.child(
                            Button::new("back")
                                .ghost()
                                .label("‹ Back")
                                .on_click(cx.listener(|this, _, _, cx| this.back(cx))),
                        )
                    })
                    .child(
                        div()
                            .text_2xl()
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
                    .label("Trust it")
                    .on_click(cx.listener(move |this, _, _, cx| this.trust(&node, cx)))
            }))
            .children(source_error.map(|e| div().text_sm().text_color(theme.danger).child(e)))
            .child(v_flex().flex_1().min_h_0().child(body))
    }
}

/// The app body both platform entry points run. `data_dir` is where the environment name and pinned nodes live.
/// An error means the window never opened; the entry point logs it where that platform's logs go.
pub fn run(data_dir: PathBuf, cx: &mut App) -> anyhow::Result<()> {
    gpui_component::init(cx);
    cx.open_window(WindowOptions::default(), |window, cx| {
        let pins = data_dir.clone();
        let connect: Connect = Box::new(move |name| source_for(&pins, name));
        let view = cx.new(|cx| MobileApp::new(data_dir, connect, window, cx));
        cx.new(|cx| Root::new(view, window, cx))
    })?;
    cx.activate(true);
    Ok(())
}
