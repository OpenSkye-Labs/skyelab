//! How the palette draws: a row per entry (its icon by kind or action name), T3's footer of key hints and a page's
//! Back row.

use super::super::match_ranges;
use super::{Entry, Kind, Palette, back};
use crate::ui::{CommandRow, Kbd, SkyeIcon};
use gpui::{
    App, InteractiveElement as _, Keystroke, ParentElement as _, SharedString,
    StatefulInteractiveElement as _, Styled as _, WeakEntity, div, prelude::*,
};
use gpui_component::{ActiveTheme as _, Icon, h_flex};

/// The icon a row shows when its entry names none: by kind, and for actions by a word in their name.
fn icon_for(entry: &Entry) -> SkyeIcon {
    if let Some(icon) = entry.icon {
        return icon;
    }
    let name = match entry.kind {
        Kind::Project => return SkyeIcon::Folder,
        Kind::Thread => return SkyeIcon::MessageCircle,
        Kind::Tab => return SkyeIcon::PanelBottom,
        Kind::Action(name) => name.rsplit("::").next().unwrap_or(name),
    };
    const BY_WORD: &[(&str, SkyeIcon)] = &[
        ("NewThread", SkyeIcon::NewThread),
        ("NewScratch", SkyeIcon::NewThread),
        ("SideChat", SkyeIcon::MessagesSquare),
        ("Usage", SkyeIcon::ChartPie),
        ("Onboarding", SkyeIcon::Sparkles),
        ("Home", SkyeIcon::Home),
        ("Bots", SkyeIcon::Bot),
        ("Settings", SkyeIcon::Settings),
        ("Theme", SkyeIcon::Palette),
        ("Script", SkyeIcon::Play),
        ("GoForward", SkyeIcon::ChevronRight),
        ("GoBack", SkyeIcon::ChevronLeft),
        ("Forward", SkyeIcon::Plug),
        ("Focus", SkyeIcon::Keyboard),
        ("Import", SkyeIcon::ArrowDown),
        ("Reopen", SkyeIcon::Undo2),
        ("Snooze", SkyeIcon::AlarmClock),
        ("Settle", SkyeIcon::Check),
        ("Close", SkyeIcon::X),
        ("Terminal", SkyeIcon::SquareTerminal),
        ("Sidebar", SkyeIcon::PanelLeft),
        ("BottomPanel", SkyeIcon::PanelBottom),
        ("RightPanel", SkyeIcon::PanelRight),
        ("FilePicker", SkyeIcon::File),
        ("Find", SkyeIcon::Search),
        ("Search", SkyeIcon::Search),
        ("Browser", SkyeIcon::Browser),
        ("Device", SkyeIcon::Smartphone),
        ("Environment", SkyeIcon::Remote),
        ("Remote", SkyeIcon::Remote),
        ("Sharing", SkyeIcon::Remote),
        ("PullRequest", SkyeIcon::GitPullRequest),
        ("ThreadPr", SkyeIcon::GitPullRequest),
        ("Issues", SkyeIcon::CircleDot),
        ("Changes", SkyeIcon::FileDiff),
        ("Worktree", SkyeIcon::GitBranch),
        ("Branch", SkyeIcon::GitBranch),
        ("Commit", SkyeIcon::GitCommitVertical),
        ("Copy", SkyeIcon::Copy),
        ("Archive", SkyeIcon::Archive),
        ("Delete", SkyeIcon::Trash),
        ("Pin", SkyeIcon::Pin),
        ("Rename", SkyeIcon::PenLine),
        ("Project", SkyeIcon::FolderPlus),
        ("Folder", SkyeIcon::FolderOpen),
        ("Account", SkyeIcon::User),
        ("Update", SkyeIcon::RefreshCw),
        ("Stop", SkyeIcon::Square),
        ("Interrupt", SkyeIcon::Square),
        ("Schedule", SkyeIcon::Clock),
        ("Dictation", SkyeIcon::Mic),
        ("Editor", SkyeIcon::SquareCode),
        ("Gallery", SkyeIcon::Rows3),
        ("Debug", SkyeIcon::Code),
        ("Keymap", SkyeIcon::Keyboard),
        ("Window", SkyeIcon::Monitor),
        ("Quit", SkyeIcon::X),
        // Provider logos draw only as images in their own colors, never as a palette glyph.
        ("Codex", SkyeIcon::Bot),
        ("Claude", SkyeIcon::Bot),
        ("Antigravity", SkyeIcon::Bot),
        ("Thread", SkyeIcon::MessageCircle),
    ];
    BY_WORD
        .iter()
        .find(|(word, _)| name.contains(word))
        .map_or(SkyeIcon::SquareMenu, |&(_, icon)| icon)
}

pub(super) fn row(entry: &Entry, query: &str) -> CommandRow {
    let title = entry.shown().clone();
    let strong = match_ranges(query, &title, entry.bold.clone());
    CommandRow::new(icon_for(entry), title)
        .strong(strong)
        .subtitle(entry.subtitle.clone())
        .checked(entry.checked)
        .shortcut(entry.binding.clone())
        .submenu(entry.submenu.is_some() || entry.page.is_some())
}

/// A key cap with T3's words ("Enter", not ↩).
fn cap(key: &str, label: &'static str) -> Kbd {
    Kbd::new(Keystroke {
        modifiers: Default::default(),
        key: key.into(),
        key_char: None,
    })
    .with_label(label)
}

/// T3's footer: ↑ ↓ Navigate, (Tab Complete), Enter Select, (Backspace Back), Esc Close (Back on a page).
pub(crate) fn footer(paged: bool, completes: bool, cx: &App) -> gpui::Div {
    let theme = cx.theme();
    let hint = |caps: Vec<Kbd>, text: &'static str| {
        h_flex()
            .gap_1()
            .children(caps)
            .child(div().ml_0p5().child(text))
    };
    h_flex()
        .w_full()
        .gap_3()
        .px_4()
        .py_2p5()
        .border_t_1()
        .border_color(theme.foreground.opacity(0.07))
        .bg(theme.foreground.opacity(0.025))
        .text_size(crate::ui::META)
        .font_weight(gpui::FontWeight::MEDIUM)
        .text_color(theme.muted_foreground)
        .child(hint(vec![cap("up", "↑"), cap("down", "↓")], "Navigate"))
        .when(completes, |f| {
            f.child(hint(vec![cap("tab", "Tab")], "Complete"))
        })
        .child(hint(vec![cap("enter", "Enter")], "Select"))
        .when(paged, |f| {
            f.child(hint(vec![cap("backspace", "Backspace")], "Back"))
        })
        .child(hint(
            vec![cap("escape", "Esc")],
            if paged { "Back" } else { "Close" },
        ))
}

/// Above a page's search field: ‹ and the page's title, back one page on click (T3's Back addon).
pub(super) fn back_row(
    title: SharedString,
    palette: WeakEntity<Palette>,
    cx: &App,
) -> gpui::Stateful<gpui::Div> {
    let theme = cx.theme();
    h_flex()
        .id("palette-back")
        .debug_selector(|| "palette-back".into())
        .mx_2()
        .mt_2()
        .px_2()
        .py_1()
        .gap_1p5()
        .rounded(crate::ui::RADIUS_MD)
        .text_size(crate::ui::META)
        .font_weight(gpui::FontWeight::MEDIUM)
        .text_color(theme.muted_foreground)
        .hover(|s| s.bg(theme.foreground.opacity(0.06)))
        .child(Icon::new(SkyeIcon::ChevronLeft).size_3p5())
        .child(title)
        .on_click(move |_, window, cx| {
            if let Some(p) = palette.upgrade() {
                back(&p, window, cx);
            }
        })
}

/// The palette's search field at T3's large size: a 16 px icon and body-size text in a 48 px row over the list.
pub(super) fn search_field(
    field: &gpui::Entity<gpui_component::input::InputState>,
    cx: &App,
) -> gpui::Div {
    let theme = cx.theme();
    h_flex()
        .debug_selector(|| "palette-search".into())
        .h(gpui::px(48.))
        .px_4()
        .gap_2p5()
        .border_b_1()
        .border_color(theme.foreground.opacity(0.07))
        .child(
            Icon::new(SkyeIcon::Search)
                .size_4()
                .text_color(theme.muted_foreground),
        )
        .child(
            div().flex_1().min_w_0().child(
                gpui_component::Sizable::large(
                    gpui_component::input::Input::new(field).appearance(false),
                )
                .p_0(),
            ),
        )
}

#[cfg(test)]
mod icon_test {
    use super::*;
    use crate::palette::Kind;

    #[test]
    fn action_icons_match_whole_words_not_prefixes() {
        let icon = |name: &'static str| {
            icon_for(&Entry::new(
                "x",
                Kind::Action(name),
                std::rc::Rc::new(|_, _| {}),
            ))
        };
        // "Pr" once matched inside Project and Prompt.
        assert_eq!(icon("skyelab::AddProject"), SkyeIcon::FolderPlus);
        assert_eq!(icon("skyelab::SchedulePrompt"), SkyeIcon::Clock);
        assert_eq!(icon("skyelab::OpenThreadPr"), SkyeIcon::GitPullRequest);
        assert_eq!(icon("skyelab::OpenPullRequests"), SkyeIcon::GitPullRequest);
        assert_eq!(icon("skyelab::OpenSettings"), SkyeIcon::Settings);
    }
}
