//! "Quote" (T3's `AssistantSelectionToolbar`): text selected in an agent reply offers a small floating button; it adds
//! the selection to the composer as a Quote chip, sent as a markdown quote (`composer_context.rs`).
//!
//! The selection is the window's (`gpui_base::TextSelection`), which tells no one when a pointer selection changes: it
//! is read once the mouse comes up in the timeline (docs/gpui/elements.md).

use super::composer_context::ChipKind;
use super::thread::ThreadView;
use crate::ui;
use crate::ui::SkyeIcon;
use gpui::{
    AnyElement, Context, InteractiveElement as _, MouseButton, Pixels, Point, Window, anchored,
    deferred, div, point, prelude::*, px,
};
use gpui_base::TextSelection;
use gpui_component::{ActiveTheme as _, ElementExt as _, Icon, Sizable as _, h_flex};

/// The chip's label: the quote's first words.
fn label(text: &str) -> String {
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    match flat.char_indices().nth(48) {
        Some((at, _)) => format!("{}…", &flat[..at]),
        None => flat,
    }
}

impl ThreadView {
    /// The timeline's listeners: a mouse-up reads the selection it made, a scroll hides the button (it doesn't follow).
    pub(in crate::workspace) fn quote_listeners(
        &self,
        body: gpui::Div,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        body.capture_any_mouse_up(cx.listener(|_, e: &gpui::MouseUpEvent, window, cx| {
            if e.button != MouseButton::Left {
                return;
            }
            let at = e.position;
            // After this event: the text view ends its drag, and a click on the button has already taken the quote.
            cx.defer_in(window, move |this, window, cx| {
                this.selection_ended(at, window, cx)
            });
        }))
        .on_scroll(cx.listener(|this, _, _, cx| {
            if this.composer.quote.take().is_some() {
                cx.notify();
            }
        }))
    }

    fn selection_ended(&mut self, at: Point<Pixels>, window: &mut Window, cx: &mut Context<Self>) {
        let text = TextSelection::selected_text(window, cx);
        let quote = (!self.read_only && !text.trim().is_empty()).then_some((at, text));
        if self.composer.quote != quote {
            self.composer.quote = quote;
            cx.notify();
        }
    }

    /// Adds the selection as a Quote chip and clears it.
    fn quote_selection(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((_, text)) = self.composer.quote.take() else {
            return;
        };
        // Cleared since (a click outside the timeline): nothing is selected any more.
        if TextSelection::selected_text(window, cx).trim().is_empty() {
            return cx.notify();
        }
        TextSelection::clear(window, cx);
        let text = skyelab_core::terminal_context::clip(&text);
        self.push_context(ChipKind::Quote, label(&text).into(), text, None, cx);
        self.focus_prompt(window, cx);
    }

    /// The floating button, just above where the selection ended.
    pub(in crate::workspace) fn render_quote_button(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let (at, _) = self.composer.quote.as_ref()?;
        let theme = cx.theme();
        let button = h_flex()
            .id("quote-selection")
            .role(gpui::accesskit::Role::Button)
            .aria_label("Quote selection")
            .debug_selector(|| "quote-selection".into())
            .keep_text_selection("quote-selection-keep")
            .h(px(26.))
            .px_2()
            .gap_1()
            .rounded(ui::RADIUS)
            .border_1()
            .border_color(theme.border)
            .bg(theme.popover)
            .shadow_md()
            .text_size(ui::META)
            .font_weight(gpui::FontWeight::MEDIUM)
            .text_color(theme.foreground)
            .cursor_pointer()
            .hover(|this| this.bg(cx.theme().accent))
            .child(
                Icon::new(SkyeIcon::Quote)
                    .xsmall()
                    .text_color(theme.muted_foreground),
            )
            .child("Quote")
            .on_click(cx.listener(|this, _, window, cx| this.quote_selection(window, cx)));
        Some(
            deferred(
                anchored()
                    .position(*at + point(px(4.), px(-34.)))
                    .snap_to_window_with_margin(px(8.))
                    .child(div().child(button)),
            )
            .with_priority(1)
            .into_any_element(),
        )
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn label_is_the_first_words() {
        assert_eq!(super::label("  a\n b  "), "a b");
        let long = "word ".repeat(20);
        let l = super::label(&long);
        assert!(l.ends_with('…') && l.chars().count() == 49, "{l}");
    }
}
