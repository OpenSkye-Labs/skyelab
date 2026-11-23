//! Terminal output added to the prompt (T3's TerminalContextInlineChip): a chip above the prompt, collapsed to the
//! terminal's name and line count, that opens to show the text; sent after the typed text as a fenced block.

use super::thread::ThreadView;
use crate::ui;
use gpui::{AnyElement, Context, SharedString, div, prelude::*, px};
use gpui_component::{
    ActiveTheme as _, Icon, Sizable as _,
    button::{Button, ButtonVariants as _},
    h_flex, v_flex,
};
use gpui_kit_assets::IconName as Lucide;
use skyelab_core::terminal_context;

/// The most excerpts one message carries; a new one past it replaces the oldest.
const MAX_CONTEXTS: usize = 8;

/// One excerpt waiting in the composer.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct TerminalContext {
    /// The terminal's tab title ("Terminal 2").
    pub label: SharedString,
    /// Already capped ([`terminal_context::clip`]).
    pub text: String,
    /// The chip shows the text.
    pub open: bool,
}

impl TerminalContext {
    fn chip_label(&self) -> String {
        match self.text.lines().count() {
            1 => format!("{} · 1 line", self.label),
            n => format!("{} · {n} lines", self.label),
        }
    }
}

impl ThreadView {
    /// Adds `text` from terminal `label` to the composer, capped. Blank text adds nothing.
    pub(crate) fn add_terminal_context(
        &mut self,
        label: SharedString,
        text: &str,
        cx: &mut Context<Self>,
    ) {
        let text = terminal_context::clip(text);
        if text.trim().is_empty() || self.read_only {
            return;
        }
        let contexts = &mut self.composer.terminal_contexts;
        if contexts.len() >= MAX_CONTEXTS {
            contexts.remove(0);
        }
        contexts.push(TerminalContext {
            label,
            text,
            open: false,
        });
        cx.notify();
    }

    /// `text` with the composer's excerpts after it, as the agent gets it; the excerpts leave the composer. An
    /// outside sender's message (`external`) never takes them.
    pub(in crate::workspace) fn take_contexts(&mut self, text: String, external: bool) -> String {
        if external || self.composer.terminal_contexts.is_empty() {
            return text;
        }
        let contexts = std::mem::take(&mut self.composer.terminal_contexts);
        terminal_context::with_contexts(
            &text,
            contexts.iter().map(|c| (c.label.as_ref(), c.text.as_str())),
        )
    }

    /// The excerpt chips above the prompt; an open one shows its text under the row.
    pub(in crate::workspace) fn render_contexts(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let contexts = &self.composer.terminal_contexts;
        if contexts.is_empty() {
            return None;
        }
        let theme = cx.theme();
        let chips = contexts.iter().enumerate().map(|(ix, c)| {
            h_flex()
                .id(("terminal-context", ix))
                .h(px(28.))
                .max_w(px(260.))
                .pl_2()
                .gap_1()
                .rounded(ui::RADIUS)
                .border_1()
                .border_color(if c.open { theme.ring } else { theme.border })
                .bg(theme.muted)
                .text_size(ui::META)
                .font_weight(gpui::FontWeight::MEDIUM)
                .cursor_pointer()
                .on_click(cx.listener(move |this, _, _, cx| {
                    if let Some(c) = this.composer.terminal_contexts.get_mut(ix) {
                        c.open = !c.open;
                    }
                    cx.notify();
                }))
                .child(
                    Icon::new(Lucide::Terminal)
                        .xsmall()
                        .text_color(ui::ink(theme, theme.success)),
                )
                .child(div().min_w_0().truncate().child(c.chip_label()))
                .child(
                    Button::new(("remove-terminal-context", ix))
                        .ghost()
                        .xsmall()
                        .icon(Icon::new(Lucide::X))
                        .tooltip("Remove")
                        .on_click(cx.listener(move |this, _, _, cx| {
                            cx.stop_propagation();
                            if ix < this.composer.terminal_contexts.len() {
                                this.composer.terminal_contexts.remove(ix);
                            }
                            cx.notify();
                        })),
                )
        });
        let open = contexts
            .iter()
            .enumerate()
            .filter(|(_, c)| c.open)
            .map(|(ix, c)| {
                div()
                    .id(("terminal-context-text", ix))
                    .max_h(px(160.))
                    .overflow_y_scroll()
                    .p_2()
                    .rounded(ui::RADIUS)
                    .border_1()
                    .border_color(theme.border)
                    .bg(theme.background)
                    .font_family(theme.mono_font_family.clone())
                    .text_xs()
                    .whitespace_normal()
                    .child(c.text.clone())
            });
        Some(
            v_flex()
                .gap_1p5()
                .px_3()
                .pt_3()
                .child(h_flex().flex_wrap().gap_1p5().children(chips))
                .children(open)
                .into_any_element(),
        )
    }
}
