//! The screens: environment picker → projects → threads → thread (events, composer, stop).

use crate::{env_store, source::ThreadSource};
use gpui::{
    AnyElement, App, Context, Entity, FontWeight, SharedString, Subscription, Window,
    WindowOptions, div, prelude::*, px,
};
use gpui_component::{
    ActiveTheme as _, Root,
    button::{Button, ButtonVariants as _},
    h_flex,
    input::{Input, InputEvent, InputState},
    v_flex,
};
use skyelab_protocol::{Project, Thread, ThreadEvent};
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    Picker,
    Projects,
    Threads { project_id: i64 },
    Thread { project_id: i64, thread_id: i64 },
}

pub struct MobileApp {
    data_dir: PathBuf,
    source: Box<dyn ThreadSource>,
    pub screen: Screen,
    pub env_input: Entity<InputState>,
    pub composer: Entity<InputState>,
    pub error: Option<SharedString>,
    _enter: [Subscription; 2],
}

impl MobileApp {
    pub fn new(
        data_dir: PathBuf,
        source: Box<dyn ThreadSource>,
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
            source,
            screen: Screen::Picker,
            env_input,
            composer,
            error: None,
            _enter,
        }
    }

    /// Validates and remembers the environment name, then shows its projects.
    // later: dial it with skyelab-client (docs/mobile.md step 4); today the source is the fake.
    pub fn connect(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let name = self.env_input.read(cx).value();
        self.error = match env_store::validate(&name) {
            Err(why) => Some(why.into()),
            Ok(name) => match env_store::save(&self.data_dir, &name) {
                Err(e) => Some(format!("Couldn't remember the name: {e:#}").into()),
                Ok(()) => {
                    self.screen = Screen::Projects;
                    None
                }
            },
        };
        cx.notify();
    }

    pub fn open_project(&mut self, project_id: i64, cx: &mut Context<Self>) {
        self.screen = Screen::Threads { project_id };
        cx.notify();
    }

    pub fn open_thread(&mut self, thread_id: i64, cx: &mut Context<Self>) {
        if let Screen::Threads { project_id } = self.screen {
            self.screen = Screen::Thread {
                project_id,
                thread_id,
            };
            cx.notify();
        }
    }

    pub fn back(&mut self, cx: &mut Context<Self>) {
        self.screen = match self.screen {
            Screen::Picker | Screen::Projects => Screen::Picker,
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
        self.source.send_message(thread_id, &text);
        self.composer
            .update(cx, |s, cx| s.set_value("", window, cx));
        cx.notify();
    }

    pub fn stop(&mut self, cx: &mut Context<Self>) {
        if let Screen::Thread { thread_id, .. } = self.screen {
            self.source.interrupt(thread_id);
            cx.notify();
        }
    }

    pub fn projects(&self) -> Vec<Project> {
        self.source.projects()
    }

    /// Threads of the open project.
    pub fn threads(&self) -> Vec<Thread> {
        match self.screen {
            Screen::Threads { project_id } | Screen::Thread { project_id, .. } => {
                self.source.threads(project_id)
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

    pub fn events(&self) -> Vec<ThreadEvent> {
        match self.screen {
            Screen::Thread { thread_id, .. } => self.source.events(thread_id),
            _ => Vec::new(),
        }
    }
}

/// The text of an event: its payload's `text` field, else the raw payload.
fn event_text(e: &ThreadEvent) -> String {
    serde_json::from_str::<serde_json::Value>(&e.payload_json)
        .ok()
        .and_then(|v| v.get("text")?.as_str().map(str::to_owned))
        .unwrap_or_else(|| e.payload_json.clone())
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
            .on_click(cx.listener(move |this, _, _, cx| this.open_thread(id, cx)))
        });
        v_flex().children(rows).into_any_element()
    }

    fn thread_view(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        // shortcut: renders every event as a child; switch to gpui `list` + ListState once events stream live.
        let events = self.events().into_iter().map(|e| {
            v_flex()
                .py_2()
                .child(
                    div()
                        .text_xs()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(theme.muted_foreground)
                        .child(e.kind.to_uppercase()),
                )
                .child(event_text(&e))
        });
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
            .child(
                div()
                    .id("events")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .child(v_flex().children(events)),
            )
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
        // Keep clear of the status bar, notch and home indicator; 16px is the margin past them.
        let safe = window.insets().safe_area;
        let body = match self.screen {
            Screen::Picker => self.picker(cx),
            Screen::Projects => self.project_list(cx),
            Screen::Threads { .. } => self.thread_list(cx),
            Screen::Thread { .. } => self.thread_view(cx),
        };
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
            .child(v_flex().flex_1().min_h_0().child(body))
    }
}

/// The app body both platform entry points run. `data_dir` is where the environment name is remembered.
pub fn run(data_dir: PathBuf, cx: &mut App) {
    gpui_component::init(cx);
    let source = Box::new(crate::source::FakeSource::new());
    let opened = cx.open_window(WindowOptions::default(), |window, cx| {
        let view = cx.new(|cx| MobileApp::new(data_dir, source, window, cx));
        cx.new(|cx| Root::new(view, window, cx))
    });
    if let Err(e) = opened {
        // Nothing to fall back to on a phone: log it for logcat / the simulator console.
        eprintln!("skyelab-mobile: could not open its window: {e:#}");
        return;
    }
    cx.activate(true);
}
