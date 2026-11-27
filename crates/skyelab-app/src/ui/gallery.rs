//! The UI Gallery: every SkyeLab component, variant and state on one page in the current theme, the place to redesign
//! from (docs/design/components.md). Debug builds, or Developer mode on: palette → "Open UI Gallery" opens it in its
//! own window.

#[path = "gallery_sections.rs"]
mod sections;
use super::A11yScrollArea as _;
use sections::{badges, buttons, command_rows, section, toasts};

use super::{
    Button, Card, Checkbox, Divider, EmptyState, SECTION, SegmentedControl, Size, SkyeIcon, Switch,
    Tabs, TextField, dialog_cancel, dialog_footer,
};
use gpui::{
    App, AppContext as _, Context, Entity, FontWeight, IntoElement, ParentElement as _, Render,
    Styled as _, Window, actions, div, prelude::*, px,
};
use gpui_component::{ActiveTheme as _, h_flex, input::InputState, v_flex};

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
        cx.new(|cx| super::window_root(view.into(), window, cx))
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
    invalid: Entity<InputState>,
    locked: Entity<InputState>,
    /// The pointer is over the loading button: only then does its spinner turn, so an open gallery idles.
    spinning: bool,
    /// Counter bumped by "Replay row appear" so `appear_row` runs on demand (0 idle frames when untouched).
    appear_seq: u64,
    appear_at: Option<std::time::Instant>,
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
            invalid: cx.new(|cx| InputState::new(window, cx).default_value("not a host")),
            locked: cx.new(|cx| InputState::new(window, cx).placeholder("Disabled")),
            spinning: false,
            appear_seq: 0,
            appear_at: None,
        }
    }

    pub(super) fn open_dialog(window: &mut Window, cx: &mut App) {
        crate::ui::open_dialog(window, cx, |dialog, _, _| {
            dialog
                .w(px(420.))
                .title(super::dialog_title("Discard changes?"))
                .child("The 3 changed files go back to the last commit. This can't be undone.")
                .footer(
                    dialog_footer()
                        .child(dialog_cancel("gallery-dialog-cancel"))
                        .child(
                            Button::new("gallery-dialog-ok")
                                .danger()
                                .small()
                                .label("Discard")
                                .on_click(|_, window, cx| super::close_dialog(window, cx)),
                        ),
                )
        });
    }
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
                    .accessibility_id("g-switch")
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
                    .accessibility_id("g-check")
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
                        .accessibility_label("Merge strategy")
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
                        .accessibility_label("Appearance")
                        .with_size(Size::Sm)
                        .icon_item(SkyeIcon::Sun, "Light")
                        .icon_item(SkyeIcon::Moon, "Dark")
                        .icon_item(SkyeIcon::Monitor, "System")
                        .selected(Some(2)),
                ),
            )
            .child(
                Tabs::new("g-tabs")
                    .accessibility_label("View sections")
                    .accessibility_id("g-tabs")
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
                    .accessibility_label("Repository views")
                    .icon_item(SkyeIcon::GitBranch, "Branches")
                    .icon_item(SkyeIcon::GitPullRequest, "Pull requests")
                    .selected(Some(1)),
            );
        let fields = v_flex()
            .gap_3()
            .w(px(420.))
            .child(TextField::new(&self.field).accessibility_label("Sample field"))
            .child(
                TextField::new(&self.search)
                    .accessibility_label("Sample search")
                    .small()
                    .cleanable(true)
                    .prefix(
                        gpui_component::Icon::new(SkyeIcon::Search)
                            .size_3p5()
                            .text_color(theme.muted_foreground),
                    ),
            )
            .child(
                TextField::new(&self.port)
                    .accessibility_label("Sample port")
                    .xsmall()
                    .suffix(div().text_color(theme.muted_foreground).child("port")),
            )
            .child(
                TextField::new(&self.invalid)
                    .accessibility_label("Sample invalid field")
                    .small()
                    .invalid(true),
            )
            .child(
                TextField::new(&self.locked)
                    .accessibility_label("Sample disabled field")
                    .small()
                    .disabled(true),
            );
        let surfaces = h_flex()
            .gap_4()
            .items_start()
            .child(
                Card::new()
                    .w(px(300.))
                    .child(div().font_weight(FontWeight::MEDIUM).child("Card"))
                    .child(Divider::horizontal())
                    .child(
                        EmptyState::new("No threads yet")
                            .hint(format!("Start one with {}", super::keys("⌘N"))),
                    ),
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
        let sample_row = div()
            .px_3()
            .py_2()
            .rounded(super::RADIUS_MD)
            .border_1()
            .border_color(theme.border)
            .bg(theme.muted)
            .text_size(super::META)
            .child(format!(
                "Row appearance #{} (100 ms ease-out quad, 4 px relative rise)",
                self.appear_seq
            ))
            .into_any_element();
        let motion_box = v_flex()
            .gap_3()
            .items_start()
            .child(
                h_flex()
                    .gap_3()
                    .child(
                        Button::new("g-replay-row")
                            .outline()
                            .small()
                            .label("Replay row appear")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.appear_seq += 1;
                                this.appear_at = Some(std::time::Instant::now());
                                cx.notify();
                            })),
                    )
                    .child(
                        Switch::new("g-reduce-motion")
                            .label("Reduce motion")
                            .checked(cx.reduce_motion())
                            .on_click(|on: &bool, _, cx| cx.set_reduce_motion(*on)),
                    ),
            )
            .child(super::motion::appear_since(
                sample_row,
                self.appear_at,
                ("g-appear-row", self.appear_seq),
            ));
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
                    .a11y_scroll_area()
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
                    .child(section("Motion", motion_box))
                    .child(section("Toasts", toasts(cx))),
            )
            .children(super::toast::layer(window, cx))
    }
}
