//! The Settings page's frame: the section nav on the left, the header, and the chosen section.

use super::*;
use gpui_component::{Icon, IconName};

/// A Settings section, picked in the page's left nav (T3's settings routes).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Section {
    #[default]
    Accounts,
    GitHub,
    Sharing,
    Threads,
    Usage,
    Appearance,
}

impl Section {
    pub const ALL: [Section; 6] = [
        Section::Accounts,
        Section::GitHub,
        Section::Sharing,
        Section::Threads,
        Section::Usage,
        Section::Appearance,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Section::Accounts => "Accounts & bindings",
            Section::GitHub => "GitHub",
            Section::Sharing => "Sharing",
            Section::Threads => "Threads",
            Section::Usage => "Usage",
            Section::Appearance => "Appearance",
        }
    }

    /// How settings.toml names it.
    pub fn key(self) -> &'static str {
        match self {
            Section::Accounts => "accounts",
            Section::GitHub => "github",
            Section::Sharing => "sharing",
            Section::Threads => "threads",
            Section::Usage => "usage",
            Section::Appearance => "appearance",
        }
    }

    pub fn parse(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|s| s.key() == key)
    }

    fn icon(self) -> IconName {
        match self {
            Section::Accounts => IconName::CircleUser,
            Section::GitHub => IconName::Github,
            Section::Sharing => IconName::Globe,
            Section::Threads => IconName::Bot,
            Section::Usage => IconName::ChartPie,
            Section::Appearance => IconName::Palette,
        }
    }
}

impl Settings {
    /// The page's left column (T3's settings nav): 240 wide on the sidebar surface, one row per section.
    fn render_nav(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let theme = cx.theme();
        let rows = Section::ALL.map(|section| {
            crate::ui::row(
                SharedString::from(format!("settings-nav-{}", section.key())),
                section == self.section,
                theme,
            )
            .child(
                Icon::new(section.icon())
                    .size_4()
                    .text_color(theme.muted_foreground),
            )
            .child(div().flex_1().min_w_0().truncate().child(section.label()))
            .on_click(move |_, window, cx| {
                window.dispatch_action(
                    Box::new(crate::workspace::OpenSettingsSection { section }),
                    cx,
                )
            })
        });
        div()
            .flex()
            .flex_col()
            .flex_shrink_0()
            .w(gpui::px(240.))
            .h_full()
            .bg(theme.sidebar)
            .border_r_1()
            .border_color(theme.border)
            .child(
                crate::workspace::layout::page_header(cx)
                    .border_b_0()
                    .text_size(crate::ui::BODY)
                    .font_weight(gpui::FontWeight::MEDIUM)
                    .child("Settings"),
            )
            .child(div().flex().flex_col().gap_0p5().px_2().children(rows))
            .into_any_element()
    }

    fn render_section(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let theme = cx.theme();
        if self.section == Section::Sharing {
            return match &self.remote {
                Some(remote) => remote.clone().into_any_element(),
                None => div()
                    .size_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        crate::ui::EmptyState::new("Sharing isn't available")
                            .hint("This window has no tailnet connection to share over."),
                    )
                    .into_any_element(),
            };
        }
        let (muted, danger, ink_danger) = (
            theme.muted_foreground,
            theme.danger,
            crate::ui::ink(theme, theme.danger),
        );
        let body = match self.section {
            Section::Accounts => div()
                .flex()
                .flex_col()
                .gap_8()
                .child(
                    div()
                        .text_size(crate::ui::BODY)
                        .text_color(muted)
                        .child("Every folder runs on the account it is bound to. SkyeLab never picks one for you."),
                )
                .children(self.error.clone().map(|e| {
                    div()
                        .id("settings-error")
                        .px_4()
                        .py_3()
                        .rounded(crate::ui::RADIUS_LG)
                        .bg(danger.opacity(0.1))
                        .border_1()
                        .border_color(danger.opacity(0.35))
                        .text_size(crate::ui::BODY)
                        .text_color(ink_danger)
                        .child(e)
                }))
                .child(self.render_accounts(cx))
                .child(self.render_bindings(cx))
                .child(self.render_test(cx)),
            Section::GitHub => div().child(self.render_github(cx)),
            Section::Threads => div().child(self.render_threads(cx)),
            Section::Usage => div().children(self.usage.clone()),
            Section::Sharing => unreachable!("returned above"),
            Section::Appearance => div().child(widgets::section(
                "Appearance",
                [widgets::row(true, cx)
                    .text_size(crate::ui::BODY)
                    .text_color(muted)
                    .child("SkyeLab follows the system's light or dark appearance. More choices come later.")
                    .into_any_element()],
                cx,
            )),
        };
        div()
            .id("settings-body")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .child(
                // T3's settings page: 896 wide, 24 padding.
                div()
                    .w_full()
                    .max_w(crate::ui::PAGE_MAX_W)
                    .mx_auto()
                    .px_6()
                    .pt_6()
                    .pb_12()
                    .child(body),
            )
            .into_any_element()
    }
}

impl Render for Settings {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let header = crate::ui::header(theme)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .gap_2()
                    .text_size(crate::ui::BODY)
                    .font_weight(gpui::FontWeight::MEDIUM)
                    .child(div().text_color(theme.muted_foreground).child("Settings"))
                    .child(
                        div()
                            .text_color(theme.muted_foreground.opacity(0.6))
                            .child("/"),
                    )
                    .child(div().truncate().child(self.section.label())),
            )
            .child(crate::workspace::layout::panel_toggles(cx));
        let nav = self.render_nav(cx);
        let section = self.render_section(cx);
        let theme = cx.theme();
        div()
            .id("settings")
            .key_context(CONTEXT)
            .track_focus(&self.focus)
            // The form's keys act only where the form shows.
            .when(self.section == Section::Accounts, |d| {
                d.on_action(cx.listener(Self::save_account))
                    .on_action(cx.listener(Self::cancel_edit))
                    .on_action(cx.listener(Self::bind_folder))
            })
            .on_action(cx.listener(Self::edit_account))
            .on_action(cx.listener(Self::remove_account))
            .on_action(cx.listener(Self::gh_log_in))
            .on_action(cx.listener(Self::remove_binding))
            .on_action(cx.listener(Self::reconfirm_binding))
            .size_full()
            .flex()
            .bg(theme.background)
            .text_color(theme.foreground)
            .child(nav)
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .child(header)
                    .child(section),
            )
    }
}
