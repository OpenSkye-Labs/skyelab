//! The Settings modal's frame (T3's settings modal): the nav on the left, a breadcrumb header with Restore defaults
//! and close, and the chosen section in a centered column.

use super::*;
use crate::ui::A11yScrollArea as _;
use crate::ui::SkyeIcon;
use gpui_component::{FocusTrapElement as _, Icon, h_flex, v_flex};

/// A Settings section, picked in the nav (T3's settings routes).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Section {
    General,
    Appearance,
    Chat,
    Diffs,
    Keys,
    #[default]
    Accounts,
    Scheduled,
    Usage,
    Projects,
    SourceControl,
    Worktrees,
    Machines,
    Sharing,
    About,
}

/// The nav's groups, in order: a `None` title is an ungrouped run (T3: top items, then Agents, Code, Connections,
/// then the bottom items).
pub(super) const GROUPS: [(Option<&str>, &[Section]); 5] = [
    (
        None,
        &[
            Section::General,
            Section::Appearance,
            Section::Chat,
            Section::Diffs,
            Section::Keys,
        ],
    ),
    (
        Some("Agents"),
        &[Section::Accounts, Section::Scheduled, Section::Usage],
    ),
    (
        Some("Code"),
        &[
            Section::Projects,
            Section::SourceControl,
            Section::Worktrees,
        ],
    ),
    (Some("Connections"), &[Section::Machines, Section::Sharing]),
    (None, &[Section::About]),
];

impl Section {
    #[cfg(test)]
    pub const ALL: [Section; 14] = [
        Section::General,
        Section::Appearance,
        Section::Chat,
        Section::Diffs,
        Section::Keys,
        Section::Accounts,
        Section::Scheduled,
        Section::Usage,
        Section::Projects,
        Section::SourceControl,
        Section::Worktrees,
        Section::Machines,
        Section::Sharing,
        Section::About,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Section::General => "General",
            Section::Appearance => "Appearance",
            Section::Chat => "Chat",
            Section::Diffs => "Diffs & pull requests",
            Section::Keys => "Keybindings",
            Section::Accounts => "Accounts",
            Section::Scheduled => "Scheduled tasks",
            Section::Usage => "Usage",
            Section::Projects => "Folder bindings",
            Section::SourceControl => "Source control",
            Section::Worktrees => "Worktrees",
            Section::Machines => "Machines",
            Section::Sharing => "Sharing & access",
            Section::About => "About",
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            Section::General => "general",
            Section::Appearance => "appearance",
            Section::Chat => "chat",
            Section::Diffs => "diffs",
            Section::Keys => "keys",
            Section::Accounts => "accounts",
            Section::Scheduled => "scheduled",
            Section::Usage => "usage",
            Section::Projects => "projects",
            Section::SourceControl => "source-control",
            Section::Worktrees => "worktrees",
            Section::Machines => "machines",
            Section::Sharing => "sharing",
            Section::About => "about",
        }
    }

    pub(super) fn icon(self) -> Icon {
        Icon::new(match self {
            Section::General => SkyeIcon::Settings2,
            Section::Appearance => SkyeIcon::Palette,
            Section::Chat => SkyeIcon::MessagesSquare,
            Section::Diffs => SkyeIcon::FileDiff,
            Section::Keys => SkyeIcon::Keyboard,
            Section::Accounts => SkyeIcon::CircleUser,
            Section::Scheduled => SkyeIcon::Clock,
            Section::Usage => SkyeIcon::ChartPie,
            Section::Projects => SkyeIcon::Folder,
            Section::SourceControl => SkyeIcon::GitBranch,
            Section::Worktrees => SkyeIcon::FolderGit2,
            Section::Machines => SkyeIcon::Monitor,
            Section::Sharing => SkyeIcon::Globe,
            Section::About => SkyeIcon::Info,
        })
    }

    /// Whether Restore defaults has anything to reset here.
    pub(super) fn has_defaults(self) -> bool {
        matches!(
            self,
            Section::General
                | Section::Appearance
                | Section::Chat
                | Section::Diffs
                | Section::Worktrees
        )
    }
}

impl Settings {
    /// Shows `section` (a nav click, a deep link, or the modal opening).
    pub fn show(&mut self, section: Section, _: &mut Window, cx: &mut Context<Self>) {
        if self.section != section {
            self.section = section;
            self.error = self.locked.clone();
            self.threads_error = None;
            self.error_open = false;
        }
        if section == Section::Chat {
            self.refresh_rules(cx); // a binding changed on Accounts shows here
            self.discover_codex_models(cx);
        }
        cx.notify();
    }

    fn render_header(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let theme = cx.theme();
        let muted = theme.muted_foreground;
        h_flex()
            .h(gpui::px(52.))
            .flex_shrink_0()
            .px_5()
            .gap_3()
            .border_b_1()
            .border_color(theme.border)
            .child(
                h_flex()
                    .id("settings-header-title")
                    .role(gpui::accesskit::Role::Heading)
                    .aria_label(format!("Settings / {}", self.section.label()))
                    .flex_1()
                    .min_w_0()
                    .gap_2p5()
                    .text_size(crate::ui::BODY)
                    .font_weight(gpui::FontWeight::MEDIUM)
                    .child(div().text_color(muted).child("Settings"))
                    .child(div().text_color(muted.opacity(0.6)).child("/"))
                    .child(div().truncate().child(self.section.label())),
            )
            .when(self.section.has_defaults(), |d| {
                d.child(
                    Button::new("settings-restore")
                        .ghost()
                        .small()
                        .icon(SkyeIcon::Undo2)
                        .label("Restore defaults")
                        .tooltip("Reset this section's settings")
                        .on_click(
                            cx.listener(|this, _, window, cx| this.restore_defaults(window, cx)),
                        ),
                )
            })
            .child(
                Button::new("settings-close")
                    .ghost()
                    .small()
                    .icon(SkyeIcon::Close)
                    .tooltip("Close (Esc)")
                    .on_click(cx.listener(|_, _, _, cx| cx.emit(SettingsEvent::Closed))),
            )
            .into_any_element()
    }

    fn render_section(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let body = match self.section {
            Section::Machines => {
                return match &self.machines {
                    Some(machines) => machines.clone().into_any_element(),
                    None => self.centered_empty(
                        "Machines aren't available",
                        "This window has no tailnet connection to reach other machines.",
                    ),
                };
            }
            Section::Sharing => {
                return match &self.remote {
                    Some(remote) => remote.clone().into_any_element(),
                    None => self.centered_empty(
                        "Sharing isn't available",
                        "This window has no tailnet connection to share over.",
                    ),
                };
            }
            Section::General => self.render_general(cx),
            Section::Appearance => self.render_appearance(cx),
            Section::Chat => self.render_chat(cx),
            Section::Diffs => self.render_diffs(cx),
            Section::Keys => self.render_keys(cx).into_any_element(),
            Section::Accounts => self.render_accounts(cx),
            Section::Usage => div().children(self.usage.clone()).into_any_element(),
            Section::Scheduled => div().children(self.scheduled.clone()).into_any_element(),
            Section::Projects => self.render_projects(cx),
            Section::SourceControl => self.render_github(cx),
            Section::Worktrees => self.render_worktrees(cx),
            Section::About => self.render_about(cx),
        };
        div()
            .id("settings-body")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .a11y_scroll_area()
            .child(
                v_flex()
                    .w_full()
                    .max_w(crate::ui::SETTINGS_MAX_W)
                    .mx_auto()
                    .px_8()
                    .pt_7()
                    .pb_12()
                    .gap_8()
                    .children(self.render_error(cx))
                    .children(
                        matches!(self.section, Section::Chat | Section::Worktrees)
                            .then(|| self.render_scope(cx))
                            .flatten(),
                    )
                    .child(body),
            )
            .into_any_element()
    }

    fn centered_empty(&self, title: &'static str, hint: &'static str) -> gpui::AnyElement {
        div()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .child(crate::ui::EmptyState::new(title).hint(hint))
            .into_any_element()
    }
}

impl Render for Settings {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let _render = crate::debug_renders::count("Settings");
        let nav = self.render_nav(cx);
        let header = self.render_header(cx);
        let section = self.render_section(cx);
        let (bg, fg) = (cx.theme().background, cx.theme().foreground);
        div()
            .id("settings")
            .role(gpui::accesskit::Role::Dialog)
            .aria_label("Settings")
            .accessibility_id("settings-modal")
            .key_context(CONTEXT)
            // The form's keys act only where the form shows.
            .when(self.section == Section::Accounts, |d| {
                d.on_action(cx.listener(Self::save_account))
            })
            .when(
                matches!(self.section, Section::Accounts | Section::Projects),
                |d| d.on_action(cx.listener(Self::bind_folder)),
            )
            .on_action(cx.listener(Self::cancel_edit))
            .on_action(cx.listener(Self::focus_search))
            .on_action(cx.listener(Self::edit_account))
            .on_action(cx.listener(Self::remove_account))
            .on_action(cx.listener(Self::gh_log_in))
            .on_action(cx.listener(Self::remove_binding))
            .on_action(cx.listener(Self::reconfirm_binding))
            .size_full()
            .flex()
            .bg(bg)
            .text_color(fg)
            .child(nav)
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .child(header)
                    .child(section),
            )
            // Tab cycles inside the modal instead of reaching the sidebar behind it.
            .focus_trap("settings-trap", &self.focus)
    }
}
