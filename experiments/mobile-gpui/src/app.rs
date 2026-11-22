//! Mobile spike: the smallest SkyeLab remote-client screen (thread list + message input) on GPUI.
//! Data is a canned `skyelab-protocol` reply, so the protocol types are proven to build for the target too.

use gpui::{App, Context, Entity, SharedString, Window, WindowOptions, div, prelude::*, px};
use gpui_component::{
    ActiveTheme as _, Root,
    button::{Button, ButtonVariants as _},
    input::{Input, InputState},
};
use skyelab_protocol::{Reply, ServerMessage, Thread};

const CANNED: &str = r#"{"type":"response","id":1,"result":{"kind":"threads","data":[
 {"id":1,"project_id":1,"title":"Fix login redirect","account_id":"work","status":"running","created_at":0,"updated_at":0},
 {"id":2,"project_id":1,"title":"Add dark mode","account_id":"work","status":"idle","created_at":0,"updated_at":0},
 {"id":3,"project_id":1,"title":"Bump tokio","account_id":"personal","status":"done","created_at":0,"updated_at":0}]}}"#;

struct MobileSpike {
    threads: Vec<Thread>,
    selected: Option<i64>,
    sent: Vec<SharedString>,
    input: Entity<InputState>,
}

impl MobileSpike {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let threads = match serde_json::from_str::<ServerMessage>(CANNED) {
            Ok(ServerMessage::Response {
                result: Reply::Threads(t),
                ..
            }) => t,
            other => panic!("canned reply must parse as threads: {other:?}"),
        };
        Self {
            selected: threads.first().map(|t| t.id),
            threads,
            sent: Vec::new(),
            input: cx.new(|cx| InputState::new(window, cx).placeholder("Message the agent…")),
        }
    }

    fn send(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let text = self.input.read(cx).value().trim().to_string();
        if text.is_empty() {
            return;
        }
        self.sent.push(text.into());
        self.input.update(cx, |s, cx| s.set_value("", window, cx));
        cx.notify();
    }
}

impl Render for MobileSpike {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let rows = self.threads.iter().map(|t| {
            let id = t.id;
            let selected = self.selected == Some(id);
            div()
                .id(("thread", id as u64))
                .p_3()
                .rounded_md()
                .when(selected, |d| d.bg(theme.accent))
                .child(div().text_lg().child(t.title.clone()))
                .child(
                    div()
                        .text_sm()
                        .text_color(theme.muted_foreground)
                        .child(format!("{} · {}", t.account_id, t.status)),
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.selected = Some(id);
                    cx.notify();
                }))
        });
        div()
            .size_full()
            .flex()
            .flex_col()
            .gap_2()
            .pt(px(64.))
            .px_4()
            .pb(px(40.))
            .bg(theme.background)
            .text_color(theme.foreground)
            .child(div().text_2xl().child("SkyeLab threads"))
            .children(rows)
            .child(div().flex_1())
            .children(self.sent.iter().map(|m| div().child(format!("you: {m}"))))
            .child(
                div()
                    .flex()
                    .gap_2()
                    .child(div().flex_1().child(Input::new(&self.input)))
                    .child(
                        Button::new("send")
                            .primary()
                            .label("Send")
                            .on_click(cx.listener(|this, _, window, cx| this.send(window, cx))),
                    ),
            )
    }
}

/// The app body both platform entry points run.
pub fn run(cx: &mut App) {
    gpui_component::init(cx);
    cx.open_window(WindowOptions::default(), |window, cx| {
        let view = cx.new(|cx| MobileSpike::new(window, cx));
        cx.new(|cx| Root::new(view, window, cx))
    })
    .expect("the spike has nothing to recover to if its only window fails to open");
    cx.activate(true);
}
