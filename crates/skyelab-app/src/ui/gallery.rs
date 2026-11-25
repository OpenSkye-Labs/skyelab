//! The UI Gallery: every SkyeLab component, variant and state on one page in the current theme, the place to redesign
//! from (docs/design/components.md). Debug builds, or Developer mode on: palette → "Open Ui Gallery" opens it in its
//! own window.

use super::{
    AccountBadge, Button, Card, Checkbox, CommandRow, Divider, EmptyState, Kbd, Pill, SECTION,
    SectionHeader, SegmentedControl, Size, SkyeIcon, StatusDot, Switch, Tabs, TextField, Variant,
    dialog_cancel, dialog_footer, icon_button,
};
use gpui::{
    App, AppContext as _, Context, Entity, FontWeight, IntoElement, ParentElement as _, Render,
    SharedString, Styled as _, Window, actions, div, prelude::*, px,
};
use gpui_component::{
    ActiveTheme as _, WindowExt as _, h_flex,
    input::InputState,
    menu::{DropdownMenu as _, PopupMenuItem},
    v_flex,
};

actions!(skyelab, [OpenUiGallery]);

/// The action's handler. The workspace registers it in debug builds or with Developer mode on: the palette lists an
/// action only when a handler sits on the focused element's path, so an App-level `on_action` would never show there.
pub fn on_open(_: &OpenUiGallery, _: &mut Window, cx: &mut App) {
    // Out of the workspace window's update before opening another window.
    cx.defer(|cx| {
        if let Err(e) = open(cx) {
            eprintln!("couldn't open the UI gallery: {e:#}");
        }
    });
}

/// Opens the gallery in a window of its own, with the same window options as a workspace window.
pub fn open(cx: &mut App) -> anyhow::Result<gpui::WindowHandle<gpui_component::Root>> {
    let bounds = gpui::Bounds::centered(None, gpui::size(px(1040.), px(860.)), cx);
    let options = match cx.try_global::<crate::workspace::WindowKit>() {
        Some(kit) => (kit.options)(bounds),
        None => gpui::WindowOptions {
            window_bounds: Some(gpui::WindowBounds::Windowed(bounds)),
            titlebar: Some(super::titlebar_options()),
            ..Default::default()
        },
    };
    cx.open_window(options, |window, cx| {
        let view = cx.new(|cx| UiGallery::new(window, cx));
        cx.new(|cx| gpui_component::Root::new(view, window, cx))
    })
}

pub struct UiGallery {
    switch_on: bool,
    check_on: bool,
    segment: usize,
    tab: usize,
    field: Entity<InputState>,
    search: Entity<InputState>,
    port: Entity<InputState>,
    locked: Entity<InputState>,
    /// The pointer is over the loading button: only then does its spinner turn, so an open gallery idles.
    spinning: bool,
}

impl UiGallery {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        Self {
            switch_on: true,
            check_on: false,
            segment: 0,
            tab: 0,
            field: cx.new(|cx| InputState::new(window, cx).placeholder("Branch name")),
            search: cx.new(|cx| InputState::new(window, cx).placeholder("Search")),
            port: cx.new(|cx| InputState::new(window, cx).default_value("5173")),
            locked: cx.new(|cx| InputState::new(window, cx).placeholder("Disabled")),
            spinning: false,
        }
    }

    fn open_dialog(window: &mut Window, cx: &mut App) {
        crate::ui::open_dialog(window, cx, |dialog, _, _| {
            dialog
                .w(px(420.))
                .title("Discard changes?")
                .child("The 3 changed files go back to the last commit. This can't be undone.")
                .footer(
                    dialog_footer()
                        .child(dialog_cancel("gallery-dialog-cancel"))
                        .child(
                            Button::new("gallery-dialog-ok")
                                .danger()
                                .small()
                                .label("Discard")
                                .on_click(|_, window, cx| window.close_dialog(cx)),
                        ),
                )
        });
    }
}

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
fn toasts(cx: &App) -> impl IntoElement {
    use super::toast::{Handler, Toast, card};
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
                    super::toast::push(
                        window,
                        Toast::success("Debug state written.").title("Saved"),
                        cx,
                    )
                }),
        )
}

/// A titled block of the page.
fn section(title: &'static str, body: impl IntoElement) -> impl IntoElement {
    v_flex()
        .gap_2()
        .child(SectionHeader::new(title))
        .child(body)
}

fn caption(text: impl Into<SharedString>, cx: &App) -> impl IntoElement {
    div()
        .w(px(84.))
        .flex_shrink_0()
        .text_size(super::META)
        .text_color(cx.theme().muted_foreground)
        .child(text.into())
}

fn buttons(loading: impl IntoElement, cx: &App) -> impl IntoElement {
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

fn badges(cx: &App) -> impl IntoElement {
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
        .child(super::provider_logo(
            skyelab_core::Provider::Claude,
            px(16.),
            cx,
        ))
        .child(super::provider_logo(
            skyelab_core::Provider::Codex,
            px(16.),
            cx,
        ))
        .child(crate::settings::provider_mark(
            skyelab_core::Provider::Claude,
            super::account_color_in(cx, "work"),
            cx,
        ))
        .child(crate::settings::provider_mark(
            skyelab_core::Provider::Codex,
            super::account_color_in(cx, "home"),
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
fn command_rows(cx: &App) -> impl IntoElement {
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
                .rounded(super::RADIUS_MD)
                .when(i == 1, |d| {
                    d.bg(theme.accent).text_color(theme.accent_foreground)
                })
                .child(row)
        }))
}

impl Render for UiGallery {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let _render = crate::debug_renders::count("UiGallery");
        let loading = div()
            .id("g-loading-hover")
            .debug_selector(|| "g-loading".into())
            .on_hover(cx.listener(|this, over: &bool, _, cx| {
                this.spinning = *over;
                cx.notify();
            }))
            .child(
                Button::new("g-loading")
                    .outline()
                    .label("Pushing")
                    .loading(true)
                    .spin(self.spinning),
            );
        let theme = cx.theme();
        let toggles = h_flex()
            .gap_6()
            .child(
                Switch::new("g-switch")
                    .label("Share on tailnet")
                    .checked(self.switch_on)
                    .on_click(cx.listener(|this, on: &bool, _, cx| {
                        this.switch_on = *on;
                        cx.notify();
                    })),
            )
            .child(
                Switch::new("g-switch-sm")
                    .xsmall()
                    .accessibility_label("Compact switch")
                    .checked(!self.switch_on),
            )
            .child(
                Switch::new("g-switch-dis")
                    .label("Disabled")
                    .checked(true)
                    .disabled(true),
            )
            .child(
                Checkbox::new("g-check")
                    .label("Delete branch after merge")
                    .checked(self.check_on)
                    .on_click(cx.listener(|this, on: &bool, _, cx| {
                        this.check_on = *on;
                        cx.notify();
                    })),
            )
            .child(
                Checkbox::new("g-check-sm")
                    .xsmall()
                    .label("Small")
                    .checked(true),
            )
            .child(
                Checkbox::new("g-check-bare")
                    .accessibility_label("Unlabeled checkbox")
                    .checked(true),
            )
            .child(
                Checkbox::new("g-check-dis")
                    .label("Disabled")
                    .disabled(true),
            );
        let choices = v_flex()
            .gap_3()
            .child(
                h_flex().child(
                    SegmentedControl::new("g-seg")
                        .items(["Squash", "Merge", "Rebase"])
                        .selected(Some(self.segment))
                        .on_click(cx.listener(|this, ix: &usize, _, cx| {
                            this.segment = *ix;
                            cx.notify();
                        })),
                ),
            )
            .child(
                h_flex().child(
                    SegmentedControl::new("g-seg-icons")
                        .with_size(Size::Sm)
                        .icon_item(SkyeIcon::Sun, "Light")
                        .icon_item(SkyeIcon::Moon, "Dark")
                        .icon_item(SkyeIcon::Monitor, "System")
                        .selected(Some(2)),
                ),
            )
            .child(
                Tabs::new("g-tabs")
                    .items(["Changes", "Files", "Checks"])
                    .with_size(Size::Sm)
                    .selected(Some(self.tab))
                    .on_click(cx.listener(|this, ix: &usize, _, cx| {
                        this.tab = *ix;
                        cx.notify();
                    })),
            )
            .child(
                Tabs::new("g-tabs-icons")
                    .icon_item(SkyeIcon::GitBranch, "Branches")
                    .icon_item(SkyeIcon::GitPullRequest, "Pull requests")
                    .selected(Some(1)),
            );
        let fields = v_flex()
            .gap_3()
            .w(px(420.))
            .child(TextField::new(&self.field))
            .child(
                TextField::new(&self.search).small().cleanable(true).prefix(
                    gpui_component::Icon::new(SkyeIcon::Search)
                        .size_3p5()
                        .text_color(theme.muted_foreground),
                ),
            )
            .child(
                TextField::new(&self.port)
                    .xsmall()
                    .suffix(div().text_color(theme.muted_foreground).child("port")),
            )
            .child(TextField::new(&self.locked).small().disabled(true));
        let surfaces = h_flex()
            .gap_4()
            .items_start()
            .child(
                Card::new()
                    .w(px(300.))
                    .child(div().font_weight(FontWeight::MEDIUM).child("Card"))
                    .child(Divider::horizontal())
                    .child(EmptyState::new("No threads yet").hint("Start one with ⌘N")),
            )
            .child(
                Card::new()
                    .flush()
                    .w(px(300.))
                    .child(super::group_row(true, theme).child("Flush card, first row"))
                    .child(super::group_row(false, theme).child("Second row"))
                    .child(
                        super::group_row(false, theme).child(
                            div()
                                .id("g-tip")
                                .text_color(theme.muted_foreground)
                                .child("Hover for a tooltip")
                                .tooltip(super::Tooltip::text("A plain tooltip")),
                        ),
                    ),
            )
            .child(
                Card::new()
                    .raised()
                    .w(px(300.))
                    .child(div().font_weight(FontWeight::MEDIUM).child("Raised card"))
                    .child(
                        h_flex()
                            .h(px(24.))
                            .gap_2()
                            .child("Left")
                            .child(Divider::vertical())
                            .child("Right"),
                    ),
            );
        let title = div()
            .text_size(SECTION)
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(theme.foreground)
            .child("UI Gallery");
        div()
            .size_full()
            .bg(theme.background)
            .child(
                v_flex()
                    .id("ui-gallery")
                    .size_full()
                    .overflow_y_scroll()
                    .pt(super::HEADER_H)
                    .px_6()
                    .pb_10()
                    .gap_6()
                    .text_color(theme.foreground)
                    .child(title)
                    .child(section("Buttons", buttons(loading, cx)))
                    .child(section("Badges, chips, keys", badges(cx)))
                    .child(section("Command palette rows", command_rows(cx)))
                    .child(section("Switch and checkbox", toggles))
                    .child(section("Segmented control and tabs", choices))
                    .child(section("Text field", fields))
                    .child(section("Card, divider, empty state", surfaces))
                    .child(section("Toasts", toasts(cx))),
            )
            .children(super::toast::layer(window, cx))
    }
}
