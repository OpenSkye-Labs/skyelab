//! The Settings modal's left column (T3's settings nav): a search field with a `/` chip, then the sections with
//! their icons, grouped under small muted headers. Typing in the search lists the matching settings instead.

use super::page::GROUPS;
use super::*;
use crate::ui::A11yScrollArea as _;
use crate::ui::Kbd;
use crate::ui::SkyeIcon;
use gpui_component::{Icon, h_flex, v_flex};

/// Every setting the search finds: its section, its row title, and other words it answers to.
const INDEX: &[(Section, &str, &str)] = &[
    (
        Section::General,
        "Open in editor",
        "editor command code zed vscode",
    ),
    (
        Section::General,
        "Terminal close confirmation",
        "ask before stop process shell console",
    ),
    (
        Section::General,
        "Developer mode",
        "debug overlay advanced profiling performance fps frames log trace leak",
    ),
    (
        Section::General,
        "Check for updates",
        "update release version",
    ),
    (
        Section::Appearance,
        "Theme",
        "color scheme skye graphite dawn grove skyelab",
    ),
    (Section::Appearance, "Mode", "light dark system appearance"),
    (
        Section::Appearance,
        "Accent color",
        "primary blue violet teal green amber rose",
    ),
    (
        Section::Appearance,
        "Density",
        "compact comfortable spacing rows",
    ),
    (
        Section::Appearance,
        "Text size",
        "font size zoom ui interface",
    ),
    (Section::Appearance, "Code text size", "mono font terminal"),
    (
        Section::Chat,
        "Default permissions",
        "mode ask plan accept edits",
    ),
    (
        Section::Chat,
        "Claude model",
        "model effort opus sonnet haiku",
    ),
    (Section::Chat, "Codex model", "model effort reasoning"),
    (
        Section::Chat,
        "Antigravity model",
        "model effort thinking gemini",
    ),
    (Section::Chat, "Generate thread titles", "title name"),
    (
        Section::Chat,
        "Send shortcut",
        "enter return newline new line multiline composer prompt cmd ctrl submit",
    ),
    (
        Section::Chat,
        "Visual replies",
        "html render page chart cdn host allow script style font https width standard wide full",
    ),
    (
        Section::Chat,
        "Permission rules",
        "allow for this project approval always command prefix path",
    ),
    (Section::Diffs, "Side by side", "split diff unified"),
    (Section::Diffs, "Wrap long lines", "diff wrap"),
    (Section::Diffs, "Ignore whitespace", "diff whitespace"),
    (
        Section::Diffs,
        "Fetch remote",
        "git fetch upstream behind interval",
    ),
    (
        Section::Diffs,
        "Generate commit messages and PRs",
        "commit pull request sparkle",
    ),
    (
        Section::Keys,
        "Keyboard shortcuts",
        "keybindings keymap keys",
    ),
    (
        Section::Accounts,
        "Accounts",
        "claude codex antigravity login provider config folder",
    ),
    (Section::Accounts, "Add account", "new account"),
    (Section::Usage, "Usage", "cost limits tokens"),
    (
        Section::Projects,
        "Folder bindings",
        "bind folder account project stale",
    ),
    (Section::Projects, "Test a folder", "which account resolve"),
    (
        Section::SourceControl,
        "GitHub login",
        "gh github push pull request",
    ),
    (
        Section::Worktrees,
        "New threads start in a worktree",
        "worktree branch",
    ),
    (
        Section::Machines,
        "Add by MagicDNS name",
        "machine device remote connect switch tailnet forget",
    ),
    (
        Section::Sharing,
        "Share on your tailnet",
        "tailscale remote devices access",
    ),
    (Section::About, "Version", "about update"),
];

/// The index rows matching every word of `query`.
pub(super) fn search(query: &str) -> Vec<(Section, &'static str)> {
    let words: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
    INDEX
        .iter()
        .filter(|(section, title, more)| {
            let hay = format!("{title} {} {more}", section.label()).to_lowercase();
            words.iter().all(|w| hay.contains(w.as_str()))
        })
        .map(|(s, t, _)| (*s, *t))
        .collect()
}

impl Settings {
    /// `/` focuses the search while Settings itself has the focus; anywhere else (a field) the key goes on and types a `/`.
    pub(super) fn focus_search(
        &mut self,
        _: &FocusSearch,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.focus.is_focused(window) {
            return cx.propagate();
        }
        self.search_input.update(cx, |s, cx| s.focus(window, cx));
    }

    pub(super) fn render_nav(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let theme = cx.theme();
        let query = self.search_input.read(cx).value();
        let item = |section: Section, selected: bool, cx: &Context<Self>| {
            let theme = cx.theme();
            crate::ui::row(
                SharedString::from(format!("settings-nav-{}", section.key())),
                selected,
                theme,
            )
            .role(gpui::accesskit::Role::Tab)
            .aria_label(section.label())
            .accessibility_id(format!("settings-nav-{}", section.key()))
            .child(section.icon().size_4().text_color(theme.muted_foreground))
            .child(div().flex_1().min_w_0().truncate().child(section.label()))
            .on_click(cx.listener(move |this, _, window, cx| this.show(section, window, cx)))
        };
        let list = if query.trim().is_empty() {
            v_flex()
                .gap_4()
                .children(GROUPS.iter().map(|(title, sections)| {
                    v_flex()
                        .gap_0p5()
                        .children(title.map(|t| {
                            div()
                                .px_2p5()
                                .pb_1()
                                .text_size(crate::ui::META)
                                .font_weight(gpui::FontWeight::MEDIUM)
                                .text_color(theme.muted_foreground)
                                .child(t)
                        }))
                        .children(sections.iter().map(|s| item(*s, *s == self.section, cx)))
                }))
                .into_any_element()
        } else {
            let found = search(&query);
            let empty = found.is_empty().then(|| {
                div()
                    .id("settings-search-empty")
                    .role(gpui::accesskit::Role::Status)
                    .aria_label("No settings match.")
                    .px_2()
                    .py_6()
                    .text_center()
                    .text_size(crate::ui::META)
                    .text_color(theme.muted_foreground)
                    .child("No settings match.")
            });
            v_flex()
                .gap_0p5()
                .children(found.into_iter().map(|(section, title)| {
                    let theme = cx.theme();
                    crate::ui::row_surface(
                        v_flex().py_1p5(),
                        SharedString::from(format!("settings-found-{}-{title}", section.key())),
                        false,
                        theme,
                    )
                    .role(gpui::accesskit::Role::Button)
                    .aria_label(format!("{title}, {}", section.label()))
                    .child(
                        div()
                            .truncate()
                            .font_weight(gpui::FontWeight::MEDIUM)
                            .child(title),
                    )
                    .child(
                        div()
                            .truncate()
                            .text_size(crate::ui::META)
                            .text_color(theme.muted_foreground)
                            .child(section.label()),
                    )
                    .on_click(
                        cx.listener(move |this, _, window, cx| this.show(section, window, cx)),
                    )
                }))
                .children(empty)
                .into_any_element()
        };
        let theme = cx.theme();
        v_flex()
            .id("settings-nav-panel")
            .role(gpui::accesskit::Role::Navigation)
            .aria_label("Settings sections")
            .accessibility_id("settings-nav")
            .flex_shrink_0()
            .w(gpui::px(248.))
            .h_full()
            .bg(theme.sidebar)
            .border_r_1()
            .border_color(theme.border)
            .child(
                h_flex().flex_shrink_0().h(gpui::px(52.)).px_2().child(
                    h_flex()
                        .w_full()
                        .h_8()
                        .px_2p5()
                        .gap_2()
                        .rounded(crate::ui::RADIUS)
                        .hover(|s| s.bg(theme.sidebar_accent.opacity(0.6)))
                        .child(
                            Icon::new(SkyeIcon::Search)
                                .size_4()
                                .text_color(theme.muted_foreground),
                        )
                        .child(
                            div().flex_1().min_w_0().child(
                                Input::new(&self.search_input)
                                    .appearance(false)
                                    .small()
                                    .aria_label("Search settings")
                                    .accessibility_id("settings-search"),
                            ),
                        )
                        .child(Kbd::new(
                            gpui::Keystroke::parse("/").expect("a plain key parses"),
                        )),
                ),
            )
            .child(
                div()
                    .id("settings-nav")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .a11y_scroll_area()
                    .px_2()
                    .pb_3()
                    .child(list),
            )
            .into_any_element()
    }
}
