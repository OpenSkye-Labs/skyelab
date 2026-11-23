//! The context-window meter (T3's thread details "Context" row): how full the thread's context is after the newest
//! model call, in the composer footer and the thread details card. Past 80% it warns and hints `/compact`.

use super::ThreadView;
use crate::ui;
use crate::workspace::thread_model::Record;
use gpui::{AnyElement, App, FontWeight, prelude::*, px};
use gpui_component::{ActiveTheme as _, Theme, h_flex, v_flex};
use skyelab_agent::Event;
use skyelab_core::context;

/// The newest reading the agent sent: each half holds until a newer one replaces it.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(in crate::workspace) struct Reading {
    pub tokens: Option<u64>,
    pub window: Option<u64>,
}

/// What the meter shows: the tokens in context, the window when known and how full that is.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::workspace) struct Meter {
    pub tokens: u64,
    pub window: Option<u64>,
    pub percent: Option<u32>,
}

impl Meter {
    /// "42% context", or "84k context" when the window is unknown.
    pub fn text(&self) -> String {
        match self.percent {
            Some(p) => format!("{p}% context"),
            None => format!("{} context", context::tokens_text(self.tokens)),
        }
    }

    pub fn near_full(&self) -> bool {
        self.percent.is_some_and(context::near_full)
    }

    /// "84k / 200k", or the tokens alone.
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
        if let Record::Agent(Event::Context { tokens, window }) = record {
            let r = &mut self.context;
            r.tokens = tokens.or(r.tokens);
            r.window = window.or(r.window);
        }
    }

    /// The meter, once the agent reported tokens. The window is the agent's own, else the one SkyeLab knows for the
    /// model the thread runs.
    pub(in crate::workspace) fn context_meter(&self) -> Option<Meter> {
        let tokens = self.context.tokens?;
        let c = &self.composer;
        let model = c.reported_model.as_deref().or(c.pick.model.as_deref());
        let window = self
            .context
            .window
            .or_else(|| model.and_then(context::window));
        Some(Meter {
            tokens,
            window,
            percent: window.and_then(|w| context::percent(tokens, w)),
        })
    }

    /// The details card's "Context" row: a bar filled to the use, the percentage and "84k / 200k", and the `/compact`
    /// hint past 80%.
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

/// Plain until 80%, then the warning ink, and the danger ink from 90% (T3's tones).
pub(in crate::workspace) fn meter_color(meter: &Meter, theme: &Theme) -> gpui::Hsla {
    match meter.percent {
        Some(p) if p >= 90 => ui::ink(theme, theme.danger),
        Some(p) if context::near_full(p) => ui::ink(theme, theme.warning),
        _ => theme.foreground.opacity(0.8),
    }
}
