//! The small shared pieces, each after its T3 Code counterpart (docs/design/t3-reference.md §6, §7):
//! badges, pills, dots, section headers, empty states, rows, headers, key hints, settings groups.

use super::{BODY, HEADER_H, META, PAGE, RADIUS_MD, RADIUS_SM, ROW_H, SECTION};
use gpui::{
    AnyElement, App, Div, ElementId, FontWeight, Hsla, IntoElement, SharedString, Stateful, Window,
    div, prelude::*, px,
};
use gpui_component::{ActiveTheme as _, Theme, h_flex, v_flex};
use skyelab_core::Provider;

/// The account identity, SkyeLab's one addition to T3's language: a dot in the account's own color, its label, and its
/// provider. Inline (like T3's context chip) in dense rows; a tinted chip (like T3's badge) where it labels a surface.
#[derive(IntoElement)]
pub struct AccountBadge {
    /// `None`: the folder is unbound, shown in the warning color.
    account: Option<(SharedString, SharedString)>,
    provider: Option<Provider>,
    chip: bool,
}

impl AccountBadge {
    pub fn new(id: impl Into<SharedString>, label: impl Into<SharedString>) -> Self {
        Self {
            account: Some((id.into(), label.into())),
            provider: None,
            chip: false,
        }
    }

    pub fn of(account: &skyelab_core::Account) -> Self {
        Self::new(account.id.clone(), account.label.clone()).provider(account.provider)
    }

    pub fn unbound() -> Self {
        Self {
            account: None,
            provider: None,
            chip: false,
        }
    }

    pub fn provider(mut self, provider: Provider) -> Self {
        self.provider = Some(provider);
        self
    }

    /// A tinted 20 px chip instead of inline text.
    pub fn chip(mut self) -> Self {
        self.chip = true;
        self
    }
}

impl RenderOnce for AccountBadge {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let (color, label) = match &self.account {
            Some((id, label)) => (super::account_color_in(cx, id), label.clone()),
            None => (theme.warning, SharedString::from("No account")),
        };
        h_flex()
            .min_w_0()
            .gap_1p5()
            .text_size(META)
            .child(StatusDot::new(color))
            .child(
                div()
                    .min_w_0()
                    .truncate()
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(theme.foreground)
                    .child(label),
            )
            .children(self.provider.map(|p| {
                div()
                    .flex_shrink_0()
                    .text_color(theme.muted_foreground)
                    .child(p.label())
            }))
            // A chip truncates its label rather than overflow its row (the thread header on a narrow center).
            .when(self.chip, |this| {
                this.h(px(20.))
                    .px_1p5()
                    .rounded(RADIUS_SM)
                    .bg(color.opacity(0.12))
            })
    }
}

/// T3's badge: 18 px, radius 4, medium; neutral, or tinted with a status color.
#[derive(IntoElement)]
pub struct Pill {
    label: SharedString,
    color: Option<Hsla>,
    icon: Option<super::SkyeIcon>,
}

impl Pill {
    pub fn new(label: impl Into<SharedString>) -> Self {
        Self {
            label: label.into(),
            color: None,
            icon: None,
        }
    }

    pub fn color(mut self, color: Hsla) -> Self {
        self.color = Some(color);
        self
    }

    /// A 12 px icon in place of the dot (the chip form: a kind or a state with its glyph).
    pub fn icon(mut self, icon: super::SkyeIcon) -> Self {
        self.icon = Some(icon);
        self
    }
}

impl RenderOnce for Pill {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let (bg, fg) = match self.color {
            Some(c) => (c.opacity(0.14), theme.foreground),
            None => (theme.secondary, theme.muted_foreground),
        };
        h_flex()
            .flex_shrink_0()
            .gap_1()
            .h(px(18.))
            .px_1p5()
            .rounded(RADIUS_SM)
            .bg(bg)
            .text_size(META)
            .font_weight(FontWeight::MEDIUM)
            .text_color(fg)
            .map(|d| match (self.icon, self.color) {
                (Some(i), c) => d.child(
                    gpui_component::Icon::new(i)
                        .size(px(12.))
                        .text_color(c.unwrap_or(theme.muted_foreground)),
                ),
                (None, Some(c)) => d.child(StatusDot::new(c)),
                (None, None) => d,
            })
            .child(self.label)
    }
}

/// A 6 px status or identity dot.
#[derive(IntoElement)]
pub struct StatusDot(Hsla);

impl StatusDot {
    pub fn new(color: Hsla) -> Self {
        Self(color)
    }
}

impl RenderOnce for StatusDot {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        div().flex_shrink_0().size(px(6.)).rounded_full().bg(self.0)
    }
}

/// T3's section header: a 12 px muted caption, a hairline filling the rest of the line, an optional control at the end.
#[derive(IntoElement)]
pub struct SectionHeader {
    title: SharedString,
    trailing: Option<AnyElement>,
}

impl SectionHeader {
    pub fn new(title: impl Into<SharedString>) -> Self {
        Self {
            title: title.into(),
            trailing: None,
        }
    }

    pub fn trailing(mut self, el: impl IntoElement) -> Self {
        self.trailing = Some(el.into_any_element());
        self
    }
}

impl RenderOnce for SectionHeader {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        h_flex()
            .h(px(28.))
            .px_2()
            .gap_2()
            .child(
                div()
                    .id(SharedString::from(format!("section-header:{}", self.title)))
                    .role(gpui::accesskit::Role::Heading)
                    .aria_label(self.title.clone())
                    .flex_shrink_0()
                    .text_size(META)
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(theme.muted_foreground)
                    .child(self.title),
            )
            .child(div().flex_1().h_px().bg(theme.border.opacity(0.5)))
            .children(self.trailing)
    }
}

/// What a list or page shows with nothing in it: what's missing, and the next step in its hint. `page()` is T3's page empty
/// state (20 px title, centered); the default is the compact one for lists.
#[derive(IntoElement)]
pub struct EmptyState {
    title: SharedString,
    hint: Option<SharedString>,
    page: bool,
}

impl EmptyState {
    pub fn new(title: impl Into<SharedString>) -> Self {
        Self {
            title: title.into(),
            hint: None,
            page: false,
        }
    }

    pub fn hint(mut self, hint: impl Into<SharedString>) -> Self {
        let hint = hint.into();
        self.hint = (!hint.is_empty()).then_some(hint);
        self
    }

    pub fn page(mut self) -> Self {
        self.page = true;
        self
    }
}

impl RenderOnce for EmptyState {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let (title_size, weight, hint_size) = if self.page {
            (PAGE, FontWeight::SEMIBOLD, BODY)
        } else {
            (BODY, FontWeight::MEDIUM, META)
        };
        let a11y_label = match &self.hint {
            Some(hint) => format!("{}. {hint}", self.title),
            None => self.title.to_string(),
        };
        v_flex()
            .id(SharedString::from(format!("empty-state:{}", self.title)))
            .role(gpui::accesskit::Role::Status)
            .aria_label(a11y_label)
            // In a row it shrinks to the row, so the hint wraps instead of running past the card.
            .min_w_0()
            .when(self.page, |this| this.max_w(px(384.)).gap_1())
            .when(!self.page, |this| this.gap_0p5().px_2().py_1p5())
            .child(
                div()
                    .text_size(title_size)
                    .font_weight(weight)
                    .text_color(theme.foreground)
                    .child(self.title),
            )
            .children(self.hint.map(|h| {
                div()
                    .text_size(hint_size)
                    .text_color(theme.muted_foreground)
                    .child(h)
            }))
    }
}

/// A label with its key binding, e.g. "New terminal  ⌥⌘T".
#[derive(IntoElement)]
pub struct KeyHint {
    label: SharedString,
    action: Box<dyn gpui::Action>,
}

impl KeyHint {
    /// Shows the key that runs `action` in the workspace now (the user's keybindings.toml included).
    pub fn new(label: impl Into<SharedString>, action: Box<dyn gpui::Action>) -> Self {
        Self {
            label: label.into(),
            action,
        }
    }
}

impl RenderOnce for KeyHint {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        h_flex()
            .w_full()
            .justify_between()
            .gap_3()
            .text_size(BODY)
            .text_color(cx.theme().foreground)
            .child(self.label)
            .children(super::Kbd::for_action(
                self.action.as_ref(),
                Some("Workspace"),
                window,
            ))
    }
}

/// A project's 16 px monogram tile, T3's fallback when a project has no favicon.
#[derive(IntoElement)]
pub struct Monogram(SharedString);

impl Monogram {
    pub fn new(name: &str) -> Self {
        let letter = name
            .chars()
            .find(|c| c.is_alphanumeric())
            .map_or_else(|| "·".into(), |c| c.to_uppercase().collect::<String>());
        Self(letter.into())
    }
}

impl RenderOnce for Monogram {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        div()
            .flex_shrink_0()
            .size(px(16.))
            .flex()
            .items_center()
            .justify_center()
            .rounded(RADIUS_SM)
            .bg(theme.muted_foreground.opacity(0.14))
            .text_color(theme.muted_foreground)
            .font_family(theme.mono_font_family.clone())
            .text_size(px(9.))
            .font_weight(FontWeight::BOLD)
            .child(self.0)
    }
}

/// T3's sidebar menu row: 32 px (28 when compact), radius 6, hover and selected fills, medium when selected, content
/// truncates.
pub fn row(id: impl Into<ElementId>, selected: bool, theme: &Theme) -> Stateful<Div> {
    row_surface(h_flex(), id, selected, theme)
        .h(if super::themes::compact() {
            px(28.)
        } else {
            ROW_H
        })
        .gap_2p5()
}

/// A sidebar row's box (hover and selected fills, radius 6) around any layout, e.g. a two-line thread row.
pub fn row_surface(
    base: Div,
    id: impl Into<ElementId>,
    selected: bool,
    theme: &Theme,
) -> Stateful<Div> {
    let hover = theme.sidebar_accent.opacity(0.7);
    super::a11y_scroll::scrolls_into_view(base.id(id))
        .role(gpui::accesskit::Role::Button)
        .aria_selected(selected)
        .w_full()
        .min_w_0()
        .px_2p5()
        .rounded(RADIUS_MD)
        .border_1()
        .border_color(gpui::transparent_black())
        .text_size(BODY)
        .cursor_pointer()
        .when(selected, |this| {
            this.bg(theme.sidebar_accent)
                .border_color(theme.sidebar_border.opacity(0.7))
                .text_color(theme.sidebar_accent_foreground)
                .font_weight(FontWeight::MEDIUM)
        })
        .when(!selected, |this| this.hover(move |s| s.bg(hover)))
}

/// A view's 52 px header: title on the left, controls on the right.
pub fn header(theme: &Theme) -> Div {
    h_flex()
        .flex_shrink_0()
        .h(HEADER_H)
        .px_5()
        .gap_3()
        .border_b_1()
        .border_color(theme.border)
}

/// A page's title (Settings, Devices, Remote): 20 px semibold.
pub fn page_title(text: impl Into<SharedString>, theme: &Theme) -> Div {
    let text: SharedString = text.into();
    div()
        .text_size(PAGE)
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(theme.foreground)
        .child(
            div()
                .id(SharedString::from(format!("heading:{text}")))
                .role(gpui::accesskit::Role::Heading)
                .aria_label(text.clone())
                .child(text),
        )
}

/// A settings section's title: 16 px semibold.
pub fn section_title(text: impl Into<SharedString>, theme: &Theme) -> Div {
    let text: SharedString = text.into();
    div()
        .px_1()
        .text_size(SECTION)
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(theme.foreground)
        .child(
            div()
                .id(SharedString::from(format!("heading:{text}")))
                .role(gpui::accesskit::Role::Heading)
                .aria_label(text.clone())
                .child(text),
        )
}

/// A group's title under a page's own heading (Machines' "Your machines"): body size, so it reads as part of the page.
pub fn subsection_title(text: impl Into<SharedString>, theme: &Theme) -> Div {
    section_title(text, theme).text_size(BODY)
}

/// T3's settings card: rounded 12, a hairline border on the muted fill, rows divided by hairlines (give each row
/// `group_row`). Backed by [`super::Card::flush`] so card chrome lives in one place.
pub fn group(_theme: &Theme) -> super::Card {
    super::Card::new().flush()
}

/// One row of a [`group`]: `px-4 py-3.5` (`py-2.5` when compact), a hairline above all but the first.
pub fn group_row(first: bool, theme: &Theme) -> Div {
    h_flex()
        .px_4()
        .map(|d| {
            if super::themes::compact() {
                d.py_2p5()
            } else {
                d.py_3p5()
            }
        })
        .gap_3()
        .text_size(BODY)
        .when(!first, |this| this.border_t_1().border_color(theme.border))
}

/// Mono text at the code size, muted.
pub fn mono(text: impl Into<SharedString>, theme: &Theme) -> Div {
    div()
        .font_family(theme.mono_font_family.clone())
        .text_size(super::CODE)
        .text_color(theme.muted_foreground)
        .child(text.into())
}
