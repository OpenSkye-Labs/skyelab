//! Stateless section builders for the UI Gallery (`gallery.rs`): buttons, badges, palette rows, and toast samples.

use super::UiGallery;
use crate::ui::ActiveTheme as _;
use crate::ui::PopupMenuItem;
use crate::ui::{
    AccountBadge, Button, CommandRow, Kbd, META, Pill, RADIUS_MD, SectionHeader, Size, SkyeIcon,
    StatusDot, Variant, account_color_in, icon_button, provider_logo, toast,
};
use crate::ui::{h_flex, v_flex};
use gpui::{App, IntoElement, ParentElement as _, SharedString, Styled as _, div, prelude::*, px};

const VARIANTS: [(Variant, &str); 6] = [
    (Variant::Primary, "Primary"),
    (Variant::Secondary, "Secondary"),
    (Variant::Ghost, "Ghost"),
    (Variant::Outline, "Outline"),
    (Variant::Danger, "Danger"),
    (Variant::Warning, "Warning"),
];

const SIZES: [(Size, &str); 3] = [(Size::Xs, "xs"), (Size::Sm, "sm"), (Size::Md, "md")];

/// Every toast kind as drawn, and a button that shows a live one (its fade in, its autohide).
pub(super) fn toasts(cx: &App) -> impl IntoElement {
    use toast::{Handler, Toast, card};
    let none: Handler = std::rc::Rc::new(|_, _| {});
    let samples = [
        Toast::new("Thread archived.").action("Undo", |_, _| {}),
        Toast::info("Imported 3 sessions."),
        Toast::success("Fix the flaky test").title("Thread completed"),
        Toast::warning("Scheduled task \"Nightly\" is paused: no account."),
        Toast::error("Couldn't start a scratch thread: no account bound."),
    ];
    v_flex()
        .gap_2()
        .items_start()
        .children(
            samples
                .iter()
                .enumerate()
                .map(|(i, t)| card(1000 + i as u64, t, none.clone(), cx)),
        )
        .child(
            Button::new("gallery-show-toast")
                .label("Show toast")
                .on_click(|_, window, cx| {
                    toast::push(
                        window,
                        Toast::success("Debug state written.").title("Saved"),
                        cx,
                    )
                }),
        )
}

/// A titled block of the page.
pub(super) fn section(title: &'static str, body: impl IntoElement) -> impl IntoElement {
    v_flex()
        .gap_2()
        .child(SectionHeader::new(title))
        .child(body)
}

fn caption(text: impl Into<SharedString>, cx: &App) -> impl IntoElement {
    div()
        .w(px(84.))
        .flex_shrink_0()
        .text_size(META)
        .text_color(cx.theme().muted_foreground)
        .child(text.into())
}

pub(super) fn buttons(loading: impl IntoElement, cx: &App) -> impl IntoElement {
    let grid = VARIANTS.iter().map(|&(v, name)| {
        h_flex()
            .gap_3()
            .child(caption(name, cx))
            .children(SIZES.iter().map(move |&(s, sn)| {
                Button::new(SharedString::from(format!("g-{name}-{sn}")))
                    .variant(v)
                    .with_size(s)
                    .label(name)
            }))
            .child(
                Button::new(SharedString::from(format!("g-{name}-icon")))
                    .variant(v)
                    .icon(SkyeIcon::Plus)
                    .label("New thread"),
            )
            .child(
                Button::new(SharedString::from(format!("g-{name}-end")))
                    .variant(v)
                    .label("Open")
                    .icon_end(SkyeIcon::ExternalLink),
            )
            .child(
                Button::new(SharedString::from(format!("g-{name}-sq")))
                    .variant(v)
                    .icon(SkyeIcon::Settings)
                    .tooltip("Icon only"),
            )
            .child(
                Button::new(SharedString::from(format!("g-{name}-dis")))
                    .variant(v)
                    .label("Disabled")
                    .disabled(true),
            )
    });
    let states = h_flex()
        .gap_3()
        .flex_wrap()
        .child(caption("States", cx))
        .child(Button::new("g-save").primary().label("Save"))
        .child(Button::new("g-later").secondary().label("Later"))
        .child(loading)
        .child(
            Button::new("g-selected")
                .ghost()
                .icon(SkyeIcon::PanelLeft)
                .label("Selected")
                .selected(true),
        )
        .child(
            Button::new("g-key")
                .outline()
                .label("Command palette")
                .key_hint(&crate::palette::OpenCommandPalette, Some("Workspace")),
        )
        .child(
            Button::new("g-caret")
                .ghost()
                .icon(SkyeIcon::Model)
                .label("Opus")
                .dropdown_caret(true)
                .dropdown_menu(|menu, _, _| {
                    menu.item(PopupMenuItem::new("Opus").checked(true))
                        .item(PopupMenuItem::new("Sonnet"))
                        .separator()
                        .item(PopupMenuItem::new("Account default"))
                }),
        )
        .child(
            icon_button("g-iconbtn-xs", SkyeIcon::Copy)
                .xsmall()
                .tooltip("Copy (xs)"),
        )
        .child(icon_button("g-iconbtn-sm", SkyeIcon::Copy).tooltip("Copy (sm)"))
        .child(
            icon_button("g-iconbtn-md", SkyeIcon::Copy)
                .medium()
                .tooltip("Copy (md)"),
        )
        .child(
            Button::new("g-tip-action")
                .ghost()
                .icon(SkyeIcon::Search)
                .tooltip_with_action(
                    "Command palette",
                    &crate::palette::OpenCommandPalette,
                    Some("Workspace"),
                ),
        )
        .child(
            Button::new("g-dialog")
                .outline()
                .label("Open dialog")
                .on_click(|_, window, cx| UiGallery::open_dialog(window, cx)),
        );
    v_flex().gap_2().children(grid).child(states)
}

pub(super) fn badges(cx: &App) -> impl IntoElement {
    let theme = cx.theme();
    h_flex()
        .gap_3()
        .flex_wrap()
        .child(Pill::new("Neutral"))
        .child(Pill::new("Running").color(theme.info))
        .child(Pill::new("Merged").color(theme.success))
        .child(Pill::new("Limited").color(theme.warning))
        .child(Pill::new("Failed").color(theme.danger))
        .child(Pill::new("Worktree").icon(SkyeIcon::GitBranch))
        .child(
            Pill::new("PR #12")
                .icon(SkyeIcon::GitPullRequest)
                .color(theme.success),
        )
        .child(StatusDot::new(theme.info))
        .child(
            AccountBadge::new("work", "Work")
                .provider(skyelab_core::Provider::Claude)
                .chip(),
        )
        .child(AccountBadge::new("home", "Home"))
        .child(AccountBadge::unbound().chip())
        // Provider logos: the brands' own files in their own colors; the account's color is the tile's dot.
        .child(provider_logo(skyelab_core::Provider::Claude, px(16.), cx))
        .child(provider_logo(skyelab_core::Provider::Codex, px(16.), cx))
        .child(provider_logo(skyelab_core::Provider::Cursor, px(16.), cx))
        .child(provider_logo(
            skyelab_core::Provider::Antigravity,
            px(16.),
            cx,
        ))
        .child(crate::settings::provider_mark(
            skyelab_core::Provider::Claude,
            account_color_in(cx, "work"),
            cx,
        ))
        .child(crate::settings::provider_mark(
            skyelab_core::Provider::Codex,
            account_color_in(cx, "home"),
            cx,
        ))
        .child(crate::settings::provider_mark(
            skyelab_core::Provider::Cursor,
            account_color_in(cx, "cursor"),
            cx,
        ))
        .child(crate::settings::provider_mark(
            skyelab_core::Provider::Antigravity,
            account_color_in(cx, "agy"),
            cx,
        ))
        .child(Kbd::new(
            gpui::Keystroke::parse("cmd-k").expect("a plain key parses"),
        ))
        .child(Kbd::new(
            gpui::Keystroke::parse("cmd-shift-p").expect("a plain key parses"),
        ))
}

/// The palette's rows: plain, a bold part and a shortcut, a subtitle, a toggle, a submenu; the second one selected.
pub(super) fn command_rows(cx: &App) -> impl IntoElement {
    let theme = cx.theme();
    let key = |k: &'static str| Kbd::new(gpui::Keystroke::parse(k).expect("a plain key parses"));
    let rows = [
        CommandRow::new(SkyeIcon::NewThread, "New thread in skyelab")
            .strong(crate::palette::match_ranges(
                "",
                "New thread in skyelab",
                Some(14..21),
            ))
            .shortcut(Some(key("cmd-n"))),
        CommandRow::new(SkyeIcon::NewThread, "New thread in…").submenu(true),
        CommandRow::new(SkyeIcon::Copy, "Copy thread")
            .subtitle(Some("Fix the flaky login test".into())),
        CommandRow::new(SkyeIcon::Remote, "Toggle Sharing on Tailnet").checked(Some(true)),
        CommandRow::new(SkyeIcon::Settings, "Open Settings")
            .strong(crate::palette::match_ranges("set", "Open Settings", None))
            .shortcut(Some(key("cmd-,"))),
    ];
    div()
        .w(px(520.))
        .flex()
        .flex_col()
        .children(rows.into_iter().enumerate().map(|(i, row)| {
            div()
                .px_2()
                .py_1p5()
                .rounded(RADIUS_MD)
                .when(i == 1, |d| {
                    d.bg(theme.accent).text_color(theme.accent_foreground)
                })
                .child(row)
        }))
}
