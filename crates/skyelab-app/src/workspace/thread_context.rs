//! The context-window meter (T3's thread details "Context" row): how full the thread's context is after the newest
//! model call, in the composer footer and the thread details card. From 80% it warns and hints `/compact`.

use super::ThreadView;
use crate::ui;
use crate::workspace::thread_model::Record;
use gpui::{AnyElement, App, FontWeight, prelude::*, px};
use gpui_component::{ActiveTheme as _, Theme, h_flex, v_flex};
use skyelab_agent::Event;
use skyelab_core::context;

/// The newest context readings the agent sent.
#[derive(Debug, Clone, Default, PartialEq)]
pub(in crate::workspace) struct Reading {
    pub tokens: Option<u64>,
    /// The model that made the newest call, when the agent said.
    pub model: Option<String>,
    /// The newest window reported per model (`None`: Codex's running model).
    pub windows: Vec<(Option<String>, u64)>,
}

impl Reading {
    /// The window reported for the model that made the newest call. A `[1m]` key (`claude-opus-4-7[1m]`) matches the
    /// plain id an assistant message names.
    fn reported_window(&self) -> Option<u64> {
        let model = self.model.as_deref();
        let exact = self.windows.iter().find(|(k, _)| k.as_deref() == model);
        let wide = || {
            self.windows.iter().find(|(k, _)| {
                model.is_some() && k.as_deref().and_then(|k| k.strip_suffix("[1m]")) == model
            })
        };
        exact.or_else(wide).map(|(_, w)| *w)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::workspace) struct Meter {
    pub tokens: u64,
    pub window: Option<u64>,
    pub percent: Option<u32>,
}

impl Meter {
    pub fn text(&self) -> String {
        match self.percent {
            Some(p) => format!("{p}% context"),
            None => format!("{} context", context::tokens_text(self.tokens)),
        }
    }

    pub fn near_full(&self) -> bool {
        self.percent.is_some_and(context::near_full)
    }

    pub fn amount(&self) -> String {
        let used = context::tokens_text(self.tokens);
        match self.window {
            Some(w) => format!("{used} / {}", context::tokens_text(w)),
            None => used,
        }
    }

    pub fn tooltip(&self) -> String {
        let mut text = format!("Context window: {} tokens", self.amount());
        if self.near_full() {
            text.push_str(". Send /compact to summarize the conversation and free it.");
        }
        text
    }
}

impl ThreadView {
    /// Keeps the newest context reading, live and when a stored thread replays.
    pub(in crate::workspace) fn note_context(&mut self, record: &Record) {
        let Record::Agent(Event::Context {
            tokens,
            window,
            model,
        }) = record
        else {
            return;
        };
        let r = &mut self.context;
        if let Some(tokens) = tokens {
            (r.tokens, r.model) = (Some(*tokens), model.clone());
        }
        if let Some(window) = window {
            r.windows.retain(|(k, _)| k != model);
            r.windows.push((model.clone(), *window));
        }
    }

    /// The meter, once the agent reported tokens. The window is the one reported for the model that ran; else the
    /// table's, for a picked `[1m]` model, the model that ran, or the thread's model.
    pub(in crate::workspace) fn context_meter(&self) -> Option<Meter> {
        let r = &self.context;
        let tokens = r.tokens?;
        let c = &self.composer;
        let pick = c.pick.model.as_deref();
        let window = r
            .reported_window()
            .or_else(|| {
                pick.filter(|p| p.ends_with("[1m]"))
                    .and_then(context::window)
            })
            .or_else(|| r.model.as_deref().and_then(context::window))
            .or_else(|| {
                pick.or(c.reported_model.as_deref())
                    .and_then(context::window)
            });
        Some(Meter {
            tokens,
            window,
            percent: window.and_then(|w| context::percent(tokens, w)),
        })
    }

    /// The details card's "Context" row (T3's `ThreadDetailsContext`).
    pub(in crate::workspace) fn render_context_row(&self, cx: &App) -> Option<AnyElement> {
        let meter = self.context_meter()?;
        let theme = cx.theme();
        let color = meter_color(&meter, theme);
        const TRACK: f32 = 64.;
        let filled = meter.percent.map_or(0., |p| p as f32 / 100.);
        Some(
            v_flex()
                .id("details-context")
                .debug_selector(|| "details-context".into())
                .min_h(ui::ROW_H)
                .py_1()
                .px_2p5()
                .justify_center()
                .child(
                    h_flex()
                        .gap_2p5()
                        .text_size(ui::META)
                        .font_weight(FontWeight::MEDIUM)
                        .child(
                            div_text("Context")
                                .flex_1()
                                .text_size(ui::CODE)
                                .text_color(theme.foreground.opacity(0.8)),
                        )
                        .child(
                            gpui::div()
                                .w(px(TRACK))
                                .h(px(4.))
                                .rounded_full()
                                .bg(theme.foreground.opacity(0.12))
                                .child(
                                    gpui::div()
                                        .h_full()
                                        .w(px(TRACK * filled))
                                        .rounded_full()
                                        .bg(color),
                                ),
                        )
                        .children(
                            meter
                                .percent
                                .map(|p| div_text(format!("{p}%")).text_color(color)),
                        )
                        .child(
                            div_text(meter.amount())
                                .font_weight(FontWeight::NORMAL)
                                .text_color(theme.muted_foreground),
                        ),
                )
                .when(meter.near_full(), |el| {
                    el.child(
                        div_text("Nearly full: send /compact to summarize and free it")
                            .text_size(ui::META)
                            .text_color(color),
                    )
                })
                .into_any_element(),
        )
    }
}

fn div_text(text: impl Into<gpui::SharedString>) -> gpui::Div {
    gpui::div().child(text.into())
}

/// T3's tones: plain, then warning, then danger.
pub(in crate::workspace) fn meter_color(meter: &Meter, theme: &Theme) -> gpui::Hsla {
    match meter.percent {
        Some(p) if p >= context::FULL => ui::ink(theme, theme.danger),
        Some(p) if context::near_full(p) => ui::ink(theme, theme.warning),
        _ => theme.foreground.opacity(0.8),
    }
}
