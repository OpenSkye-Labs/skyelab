//! T3's command palette row (`CommandPaletteResults.tsx`): a 16 px muted icon, the title with some of it bold (the
//! letters a query matched, a project name), an optional muted second line, then a toggle's tick, the shortcut as plain
//! text and a chevron on a row that opens a page. The list around it draws the hover and selected background.

use super::{BODY, Kbd, META, SkyeIcon};
use gpui::{
    App, FontWeight, HighlightStyle, IntoElement, ParentElement as _, RenderOnce, SharedString,
    Styled as _, StyledText, Window, div, prelude::*, px,
};
use gpui_component::{ActiveTheme as _, Icon, Sizable as _, h_flex, v_flex};
use std::ops::Range;

#[derive(IntoElement)]
pub struct CommandRow {
    icon: SkyeIcon,
    title: SharedString,
    strong: Vec<Range<usize>>,
    subtitle: Option<SharedString>,
    checked: Option<bool>,
    shortcut: Option<Kbd>,
    submenu: bool,
}

impl CommandRow {
    pub fn new(icon: SkyeIcon, title: impl Into<SharedString>) -> Self {
        Self {
            icon,
            title: title.into(),
            strong: Vec::new(),
            subtitle: None,
            checked: None,
            shortcut: None,
            submenu: false,
        }
    }

    /// Byte ranges of the title drawn semibold: sorted, not overlapping, on char boundaries.
    pub fn strong(mut self, ranges: Vec<Range<usize>>) -> Self {
        self.strong = ranges;
        self
    }

    pub fn subtitle(mut self, subtitle: Option<SharedString>) -> Self {
        self.subtitle = subtitle;
        self
    }

    /// A toggle's state, drawn as a ticked or empty box.
    pub fn checked(mut self, checked: Option<bool>) -> Self {
        self.checked = checked;
        self
    }

    pub fn shortcut(mut self, shortcut: Option<Kbd>) -> Self {
        self.shortcut = shortcut;
        self
    }

    /// Ends the row with a chevron: choosing it opens a page.
    pub fn submenu(mut self, submenu: bool) -> Self {
        self.submenu = submenu;
        self
    }
}

impl RenderOnce for CommandRow {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let muted = theme.muted_foreground;
        let bold = HighlightStyle {
            font_weight: Some(FontWeight::SEMIBOLD),
            ..Default::default()
        };
        let title =
            StyledText::new(self.title).with_highlights(self.strong.into_iter().map(|r| (r, bold)));
        let tick = self.checked.map(|on| {
            // Drawn, not a control: the row is what toggles (T3's box is pointer-events-none too).
            div()
                .size_3p5()
                .flex()
                .flex_none()
                .items_center()
                .justify_center()
                .rounded(px(3.))
                .border_1()
                .border_color(if on { theme.primary } else { muted })
                .when(on, |b| {
                    b.bg(theme.primary).child(
                        Icon::new(SkyeIcon::Check)
                            .xsmall()
                            .text_color(theme.primary_foreground),
                    )
                })
        });
        h_flex()
            .w_full()
            .min_w_0()
            .gap_2()
            .items_start()
            .child(
                // As tall as the title's line, so a two-line row keeps its icon on the first.
                div()
                    .flex_none()
                    .h(px(20.))
                    .flex()
                    .items_center()
                    .child(Icon::new(self.icon).size_4().text_color(muted)),
            )
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .child(
                        div()
                            .truncate()
                            .text_size(BODY)
                            .line_height(px(20.))
                            .child(title),
                    )
                    .children(
                        self.subtitle
                            .map(|s| div().truncate().text_size(META).text_color(muted).child(s)),
                    ),
            )
            .child(
                h_flex()
                    .flex_none()
                    .h(px(20.))
                    // Clear of the list's overlay scrollbar, which otherwise covers the keys once it shows.
                    .pr_3()
                    .gap_2()
                    .children(tick)
                    .children(self.shortcut.map(Kbd::bare))
                    .when(self.submenu, |r| {
                        r.child(
                            Icon::new(SkyeIcon::ChevronRight)
                                .size_4()
                                .text_color(muted.opacity(0.7)),
                        )
                    }),
            )
    }
}
