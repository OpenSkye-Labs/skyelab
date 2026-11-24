//! The card a mermaid fence draws as (T3's `MarkdownMermaidCodeBlock`): the code block's frame with "mermaid" in the
//! corner, the diagram scaled down to fit, and copy plus a Show code / Show diagram toggle. While the fence is still
//! streaming, or when the diagram can't be drawn, the card shows the source instead.

use super::mermaid::{Block, Palette, Render};
use super::tool_cards;
use crate::ui;
use crate::ui::Button;
use gpui::{AnyElement, App, ObjectFit, SharedString, Window, div, img, prelude::*, px, relative};
use gpui_component::text::MarkdownNode;
use gpui_component::{ActiveTheme as _, Icon, h_flex, v_flex};

/// The markdown block renderer for [`super::mermaid::NODE`] nodes.
pub(super) fn render(node: &MarkdownNode, window: &mut Window, cx: &mut App) -> AnyElement {
    let Some(block) = node.data::<Block>() else {
        return div().into_any_element();
    };
    let at = node.source_range().map_or(0, |r| r.start);
    let theme = cx.theme();
    let palette = Palette::new(
        // Opaque, as the card looks on the page: edge labels paint it over the lines.
        theme.background.blend(theme.muted),
        theme.background,
        theme.secondary,
        theme.foreground,
        theme.muted_foreground,
        theme.border,
    );
    // The plain code block's own background and corner (gpui-component's text view style).
    let radius = theme.semantic_tokens().radius.md;
    let (card, border, muted, mono) = (
        theme.muted,
        theme.border,
        theme.muted_foreground,
        theme.mono_font_family.clone(),
    );
    // Keyed by where the fence starts, under the text view's own id, so each block keeps its own toggle.
    let show_code = window.use_keyed_state(("mermaid-code", at), cx, |_, _| false);
    let drawn = block
        .closed
        .then(|| window.use_asset::<Render>(&(block.source.clone(), palette), cx))
        .flatten();
    let code_view = |note: Option<SharedString>| {
        v_flex()
            .gap_2()
            .children(note.map(|n| {
                div()
                    .text_size(ui::META)
                    .text_color(muted)
                    .child(SharedString::from(format!(
                        "Couldn't draw this diagram: {n}"
                    )))
            }))
            .child(
                div()
                    .font_family(mono.clone())
                    .text_size(ui::CODE)
                    .line_height(relative(1.6))
                    .child(block.source.clone()),
            )
            .into_any_element()
    };
    let showing_code = *show_code.read(cx);
    let (body, toggle) = match drawn {
        Some(Ok(d)) if !showing_code => (
            h_flex()
                .w_full()
                .justify_center()
                .child(
                    div()
                        .w(px(d.width))
                        .max_w_full()
                        .aspect_ratio(d.width / d.height)
                        .child(
                            img(d.image.clone())
                                .size_full()
                                .object_fit(ObjectFit::Contain),
                        ),
                )
                .into_any_element(),
            true,
        ),
        Some(Ok(_)) => (code_view(None), true),
        Some(Err(e)) => (code_view(Some(e)), false),
        // Still streaming, or the first frame before the render lands.
        None => (code_view(None), false),
    };
    let toggle = toggle.then(|| {
        let (icon, label) = if showing_code {
            (crate::ui::SkyeIcon::Workflow, "Show diagram")
        } else {
            (crate::ui::SkyeIcon::Code, "Show code")
        };
        Button::new(("mermaid-toggle", at))
            .ghost()
            .xsmall()
            .icon(Icon::new(icon))
            .tooltip(label)
            .on_click(move |_, _, cx| {
                show_code.update(cx, |s, cx| {
                    *s = !*s;
                    cx.notify();
                })
            })
    });
    div()
        .id(("mermaid", at))
        .debug_selector(|| "mermaid-card".into())
        .relative()
        .w_full()
        .min_w_0()
        .px_4()
        .pt(px(32.))
        .pb(px(14.))
        .bg(card)
        .border_1()
        .border_color(border)
        .rounded(radius)
        .child(body)
        .child(
            div()
                .absolute()
                .top(px(9.))
                .left_4()
                .text_size(ui::META)
                .text_color(muted)
                .child("mermaid"),
        )
        .child(
            h_flex()
                .absolute()
                .top_1()
                .right_1()
                .gap_0p5()
                .children(toggle)
                .child(tool_cards::copy(("copy-mermaid", at), block.source.clone())),
        )
        .into_any_element()
}
