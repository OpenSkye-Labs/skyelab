//! The sidebar's rows: a flat list's two-line thread row, the plain row (remote projects and their threads), and the
//! project scope button.

use super::sidebar::{RowAccount, RowMarks, ScopeMenu, SidebarItem, SidebarSection};
use super::*;
use gpui::AnyElement;
use gpui_component::Selectable as _;
use gpui_component::menu::{ContextMenuExt as _, DropdownMenu as _};
use gpui_kit_assets::IconName as Lucide;

pub(super) fn section_button(s: &SidebarSection) -> Option<Button> {
    s.button.as_ref().map(|(label, action)| {
        let action = action.boxed_clone();
        Button::new(SharedString::from(format!("section-{}", s.title)))
            .ghost()
            .xsmall()
            .icon(IconName::Plus)
            .label(label.clone())
            .on_click(move |_, window, cx| window.dispatch_action(action.boxed_clone(), cx))
    })
}

/// Where a thread's title starts, so it lines up under its project's name (T3's `ps-9`): row padding + monogram + gap.
pub(super) const THREAD_INDENT: f32 = 36.;

pub(super) fn sidebar_row(
    item: &SidebarItem,
    marks: Option<&RowMarks>,
    focus: &FocusHandle,
    indent: bool,
    now: i64,
    theme: &gpui_component::Theme,
) -> AnyElement {
    let row = if item.project.is_some() {
        thread_row(item, marks, now, theme)
    } else {
        plain_row(item, marks, indent, now, theme)
    };
    let Some(menu) = marks.filter(|m| !m.menu.is_empty()) else {
        return row.into_any_element();
    };
    let entries: Vec<(SharedString, Box<dyn Action>)> = menu
        .menu
        .iter()
        .map(|(l, a)| (l.clone(), a.boxed_clone()))
        .collect();
    let focus = focus.clone();
    row.context_menu(move |mut menu, _, _| {
        menu = menu.action_context(focus.clone());
        for (label, action) in &entries {
            menu = menu.menu(label.clone(), action.boxed_clone());
        }
        menu
    })
    .into_any_element()
}

/// A flat list's thread row, after T3's and Synara's: the title with its status or time, the agent's last reply in two
/// muted lines, then a meta line with the project, the account its folder is bound to (SkyeLab's point), the pull
/// request and the branch.
fn thread_row(
    item: &SidebarItem,
    marks: Option<&RowMarks>,
    now: i64,
    theme: &gpui_component::Theme,
) -> gpui::Stateful<gpui::Div> {
    let action = item.action.boxed_clone();
    let empty = RowMarks::default();
    let marks = marks.unwrap_or(&empty);
    let (pinned, unread, approval) = (marks.pinned, marks.unread, marks.approval);
    let project = item.project.clone().unwrap_or_default();
    let status = if approval {
        Some((IconName::TriangleAlert, "Approval", theme.warning))
    } else if let Some((icon, word)) = item.status.and_then(ui::Status::badge) {
        let color = item
            .status
            .map_or(theme.muted_foreground, |s| s.color(theme));
        Some((icon, word, color))
    } else {
        // T3's "Done": a finished turn nobody has looked at yet.
        unread.then_some((IconName::CircleCheck, "Done", theme.success))
    };
    let right = match status {
        Some((icon, word, color)) => h_flex()
            .flex_shrink_0()
            .gap_1()
            .font_weight(gpui::FontWeight::MEDIUM)
            .text_color(ui::ink(theme, color))
            .child(Icon::new(icon).size_3().text_color(color))
            .child(word)
            .into_any_element(),
        // shortcut: the time is computed when the sidebar draws, so an idle sidebar shows it as of its last redraw;
        // add a minute timer if stale times get noticed.
        None => div()
            .flex_shrink_0()
            .text_color(theme.muted_foreground)
            .children(item.updated_ms.map(|t| ui::ago(now, t)))
            .into_any_element(),
    };
    let account = match &item.account {
        Some(RowAccount::Bound(acc)) => Some(ui::AccountBadge::of(acc)),
        Some(RowAccount::Unbound) => Some(ui::AccountBadge::unbound()),
        Some(RowAccount::Hidden) | None => None,
    };
    let muted = theme.muted_foreground;
    ui::row_surface(v_flex(), item.id.clone(), item.selected, theme)
        .py_1p5()
        .gap_0p5()
        .font_weight(gpui::FontWeight::NORMAL)
        .on_click(move |_, window, cx| window.dispatch_action(action.boxed_clone(), cx))
        .child(
            h_flex()
                .h(gpui::px(20.))
                .gap_1p5()
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .when(item.selected || unread, |d| {
                            d.font_weight(gpui::FontWeight::MEDIUM)
                        })
                        .child(item.label.clone()),
                )
                .when(pinned, |this| {
                    this.child(Icon::new(IconName::Star).size_3().text_color(muted))
                })
                .child(div().text_size(ui::META).child(right)),
        )
        .children(marks.preview.clone().map(|p| {
            div()
                .id("preview")
                .text_size(ui::META)
                .line_height(gpui::px(16.))
                .text_color(muted)
                .line_clamp(2)
                .child(p)
        }))
        .child(
            h_flex()
                .h(gpui::px(18.))
                .min_w_0()
                .gap_1p5()
                .text_size(ui::META)
                .text_color(muted)
                .child(ui::Monogram::new(&project))
                .child(div().min_w_0().truncate().child(project))
                .children(account.map(|badge| div().min_w_0().child(badge)))
                .child(div().flex_1())
                .children(marks.pr.clone().map(|pr| {
                    h_flex()
                        .flex_shrink_0()
                        .gap_0p5()
                        .text_color(ui::ink(theme, theme.success))
                        .child(Icon::new(Lucide::GitPullRequest).size_3())
                        .child(pr)
                }))
                .children(marks.branch.clone().map(|b| {
                    h_flex()
                        .min_w_0()
                        .max_w(gpui::px(112.))
                        .gap_0p5()
                        .child(Icon::new(Lucide::GitBranch).size_3().flex_shrink_0())
                        .child(div().min_w_0().truncate().child(b))
                })),
        )
}

fn plain_row(
    item: &SidebarItem,
    marks: Option<&RowMarks>,
    indent: bool,
    now: i64,
    theme: &gpui_component::Theme,
) -> gpui::Stateful<gpui::Div> {
    let action = item.action.boxed_clone();
    let is_project = item.account.is_some();
    let (pinned, unread) = marks.map_or((false, false), |m| (m.pinned, m.unread));
    // A project row is never lifted (T3 keeps project headers flat): a selected one is only set in medium weight.
    ui::row(item.id.clone(), item.selected && !is_project, theme)
        .when(item.selected && is_project, |this| {
            this.font_weight(gpui::FontWeight::MEDIUM)
        })
        .on_click(move |_, window, cx| window.dispatch_action(action.boxed_clone(), cx))
        .when(indent, |this| this.pl(gpui::px(THREAD_INDENT)))
        .when(is_project, |this| {
            this.child(ui::Monogram::new(&item.label))
        })
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .when(unread, |d| d.font_weight(gpui::FontWeight::MEDIUM))
                .child(item.label.clone()),
        )
        .when(pinned, |this| {
            this.child(
                Icon::new(IconName::Star)
                    .size_3()
                    .text_color(theme.muted_foreground),
            )
        })
        // T3's unread dot: a finished turn nobody has looked at yet.
        .when(unread, |this| this.child(ui::StatusDot::new(theme.info)))
        .children(item.account.as_ref().and_then(|a| {
            let badge = match a {
                RowAccount::Bound(acc) => ui::AccountBadge::new(acc.id.clone(), acc.label.clone()),
                RowAccount::Unbound => ui::AccountBadge::unbound(),
                RowAccount::Hidden => return None,
            };
            Some(div().flex_shrink_0().max_w(gpui::px(104.)).child(badge))
        }))
        .children(match item.status.and_then(ui::Status::badge) {
            Some((icon, word)) => {
                let color = item
                    .status
                    .map_or(theme.muted_foreground, |s| s.color(theme));
                Some(
                    h_flex()
                        .flex_shrink_0()
                        .gap_1()
                        .text_size(ui::META)
                        .font_weight(gpui::FontWeight::MEDIUM)
                        .text_color(ui::ink(theme, color))
                        .child(Icon::new(icon).size_3().text_color(color))
                        .child(word)
                        .into_any_element(),
                )
            }
            // shortcut: the time is computed when the sidebar draws, so an idle sidebar shows it as of its last redraw;
            // add a minute timer if stale times get noticed.
            None => item.updated_ms.map(|t| {
                div()
                    .flex_shrink_0()
                    .text_size(ui::META)
                    .font_weight(gpui::FontWeight::NORMAL)
                    .text_color(theme.muted_foreground)
                    .child(ui::ago(now, t))
                    .into_any_element()
            }),
        })
}

/// The project scope filter (T3's project combobox): All, each project as a check row, each project's menu, and
/// Add project.
pub(super) fn scope_button(scope: ScopeMenu, focus: &FocusHandle) -> impl IntoElement {
    let (filtered, focus) = (scope.filtered(), focus.clone());
    Button::new("sidebar-scope")
        .ghost()
        .small()
        .icon(IconName::Folder)
        .selected(filtered)
        .tooltip("Projects")
        .dropdown_menu(move |mut menu, window, cx| {
            menu = menu
                .action_context(focus.clone())
                .min_w(gpui::px(220.))
                .menu_with_check(
                    "All projects",
                    !scope.filtered(),
                    Box::new(super::super::ShowAllProjects),
                )
                .separator();
            for p in &scope.projects {
                let toggle = super::super::ToggleProjectScope { id: p.id };
                menu = menu.menu_with_check(p.name.clone(), p.checked, Box::new(toggle));
            }
            if !scope.projects.is_empty() {
                let projects = scope.projects.clone();
                menu = menu.separator().submenu(
                    "Manage projects",
                    window,
                    cx,
                    move |mut menu, window, cx| {
                        for p in &projects {
                            let entries = super::super::manage::project_menu(p.id);
                            menu =
                                menu.submenu(p.name.clone(), window, cx, move |mut menu, _, _| {
                                    for (label, action) in &entries {
                                        menu = menu.menu(label.clone(), action.boxed_clone());
                                    }
                                    menu
                                });
                        }
                        menu
                    },
                );
            }
            menu.separator().menu_with_icon(
                "Add project",
                IconName::Plus,
                Box::new(super::super::AddProject),
            )
        })
}
