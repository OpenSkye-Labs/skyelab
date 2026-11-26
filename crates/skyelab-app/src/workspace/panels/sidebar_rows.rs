//! The sidebar's rows: a flat list's thread row (title, reply preview, meta line), the plain row (remote projects and their threads), and the
//! project scope button.

use super::sidebar::{
    Ask, RowAccount, RowMarks, ScopeMenu, SidebarItem, SidebarSection, SnoozeMenu,
};
use super::sidebar_snooze::hover_actions;
use super::*;
use crate::ui::SkyeIcon;
use crate::workspace::dispatch;
use gpui::AnyElement;
use gpui_component::Colorize as _;
use gpui_component::menu::DropdownMenu as _;
use skyelab_core::snooze;

pub(super) fn section_button(s: &SidebarSection) -> Option<Button> {
    s.button.as_ref().map(|(label, action)| {
        let action = action.boxed_clone();
        Button::new(SharedString::from(format!("section-{}", s.title)))
            .ghost()
            .xsmall()
            .icon(SkyeIcon::Plus)
            .label(label.clone())
            .on_click(move |_, window, cx| dispatch(action.boxed_clone(), window, cx))
    })
}

/// How the list draws its rows now: whether rows show their hover card, and whether a search is on (rows then show
/// their matched line).
#[derive(Clone, Copy)]
pub(super) struct RowLook {
    pub cards: bool,
    pub searching: bool,
}

/// Where a thread's title starts, so it lines up under its project's name (T3's `ps-9`): row padding + monogram + gap.
pub(super) const THREAD_INDENT: f32 = 36.;

pub(super) fn sidebar_row(
    item: &SidebarItem,
    marks: Option<&RowMarks>,
    indent: bool,
    look: RowLook,
    snooze: &SnoozeMenu,
    now: i64,
    theme: &gpui_component::Theme,
) -> AnyElement {
    let RowLook { cards, searching } = look;
    let row = if item.project.is_some() {
        thread_row(item, marks, searching, snooze, now, theme)
    } else {
        plain_row(item, marks, indent, now, theme)
    };
    let card = marks.and_then(|m| m.card.clone()).filter(|_| cards);
    let Some(menu) = marks.filter(|m| !m.menu.is_empty() || m.shelf.is_some()) else {
        return super::sidebar_card::with_card(&item.id, row, card);
    };
    let entries: Vec<(SharedString, Box<dyn Action>)> = menu
        .menu
        .iter()
        .map(|(l, a)| (l.clone(), a.boxed_clone()))
        .collect();
    // The sidebar draws the menu above its list: a context menu inside a list row opens but never paints.
    let (sidebar, shelf) = (snooze.sidebar.clone(), menu.shelf.clone());
    let row = row.on_mouse_down(gpui::MouseButton::Right, move |e, window, cx| {
        cx.stop_propagation();
        let entries = entries
            .iter()
            .map(|(l, a)| (l.clone(), a.boxed_clone()))
            .collect();
        let shelf = shelf.clone();
        let _ = sidebar.update(cx, |s, cx| {
            s.open_row_menu(entries, shelf, e.position, window, cx)
        });
    });
    super::sidebar_card::with_card(&item.id, row, card)
}

/// A flat list's thread row, after T3's: the title with its status or time, then a meta line with the project, the
/// account its folder is bound to (SkyeLab's point), the pull request and the branch. While searching, the matched
/// line (or the last reply) sits between them in two muted lines, saying why the row matched; otherwise rows stay two
/// lines tall, as T3's have no reply preview.
fn thread_row(
    item: &SidebarItem,
    marks: Option<&RowMarks>,
    searching: bool,
    snooze: &SnoozeMenu,
    now: i64,
    theme: &gpui_component::Theme,
) -> gpui::Stateful<gpui::Div> {
    let action = item.action.boxed_clone();
    let empty = RowMarks::default();
    let marks = marks.unwrap_or(&empty);
    let (pinned, unread) = (marks.pinned, marks.unread);
    let project = item.project.clone().unwrap_or_default();
    let snoozed = marks.shelf.as_ref().and_then(|s| s.snoozed_until);
    let running = matches!(
        item.status,
        Some(ui::Status::Starting | ui::Status::Running)
    );
    let status = if let Some(ask) = marks.ask {
        // T3's colors: amber for a permission, indigo for a question, violet for a plan.
        let (icon, color) = match ask {
            Ask::Approval => (SkyeIcon::TriangleAlert, theme.warning),
            Ask::Input => (
                SkyeIcon::MessageCircle,
                theme.blue.mix_oklab(theme.magenta, 0.7),
            ),
            Ask::Plan => (
                SkyeIcon::ClipboardList,
                theme.magenta.mix_oklab(theme.blue, 0.6),
            ),
        };
        Some((Icon::new(icon), ask.label().into(), color))
    } else if let Some(until) = snoozed.filter(|_| !running) {
        // T3's alarm clock: asleep shows when it wakes; woke (passed, not opened yet) says so.
        if snooze::is_woke(Some(until), now) {
            Some((
                Icon::new(SkyeIcon::AlarmClock),
                "Woke".into(),
                theme.warning,
            ))
        } else {
            let when = snooze::wake_label(until, now).into();
            Some((Icon::new(SkyeIcon::AlarmClock), when, theme.info))
        }
    } else if let Some((icon, word)) = item.status.and_then(ui::Status::badge) {
        let color = item
            .status
            .map_or(theme.muted_foreground, |s| s.color(theme));
        // T3's working timer: how long the turn has run.
        let word: SharedString = match marks.started_ms {
            Some(at) => format!("{word} {}", ui::elapsed(now - at)).into(),
            None => word.into(),
        };
        Some((Icon::new(icon), word, color))
    } else {
        // T3's "Done": a finished turn nobody has looked at yet.
        unread.then_some((
            Icon::new(SkyeIcon::CircleCheck),
            "Done".into(),
            theme.success,
        ))
    };
    let right = match status {
        Some((icon, word, color)) => h_flex()
            .flex_shrink_0()
            .gap_1()
            .font_weight(gpui::FontWeight::MEDIUM)
            .text_color(ui::ink(theme, color))
            .child(icon.size_3().text_color(color))
            .child(word)
            .into_any_element(),
        // Moved on without a reload by the sidebar's clock (`sidebar_clock.rs`).
        None => div()
            .flex_shrink_0()
            .text_color(theme.muted_foreground)
            .children(item.updated_ms.map(|t| ui::ago(now, t)))
            .into_any_element(),
    };
    let account = match &item.account {
        // Without the provider: the meta line is narrow, and the label names the login.
        Some(RowAccount::Bound(acc)) => {
            Some(ui::AccountBadge::new(acc.id.clone(), acc.label.clone()))
        }
        Some(RowAccount::Unbound) => Some(ui::AccountBadge::unbound()),
        Some(RowAccount::Hidden) | None => None,
    };
    let muted = theme.muted_foreground;
    let selector = format!("row-{}", item.id);
    let group = SharedString::from(format!("hover-{}", item.id));
    let menu_open = marks
        .shelf
        .as_ref()
        .is_some_and(|s| snooze.open.as_ref().is_some_and(|o| o.0 == s.id));
    let actions = marks
        .shelf
        .clone()
        .map(|s| hover_actions(s, snooze, now, theme));
    let right = div()
        .relative()
        .text_size(ui::META)
        .child(
            div()
                .when(menu_open, |d| d.invisible())
                .when(actions.is_some(), |d| {
                    d.group_hover(group.clone(), |s| s.invisible())
                })
                .child(right),
        )
        .children(actions.map(|a| {
            // The row's own fill (the hover one is translucent over the sidebar), so the buttons cover the title's end
            // instead of drawing over its letters.
            let fill = if item.selected {
                theme.sidebar_accent
            } else {
                theme.sidebar_accent.opacity(0.7)
            };
            div()
                .absolute()
                .right_0()
                .top(gpui::px(-2.))
                .rounded(ui::RADIUS_MD)
                .bg(theme.sidebar)
                .when(!menu_open, |d| d.invisible())
                .group_hover(group.clone(), |s| s.visible())
                .child(a.pl_1().rounded(ui::RADIUS_MD).bg(fill))
        }));
    ui::row_surface(v_flex(), item.id.clone(), item.selected, theme)
        .group(group.clone())
        .debug_selector(move || selector.clone())
        .py_1p5()
        .gap_0p5()
        .when(marks.nested, |this| this.pl(gpui::px(26.))) // under its group's row, inside the list
        .font_weight(gpui::FontWeight::NORMAL)
        .on_click(move |_, window, cx| dispatch(action.boxed_clone(), window, cx))
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
                .children(
                    marks
                        .group
                        .map(|(count, open, group)| group_badge(count, open, group, theme)),
                )
                .when(pinned, |this| {
                    this.child(Icon::new(SkyeIcon::Pin).size_3().text_color(muted))
                })
                .when(marks.scheduled, |this| this.child(scheduled_badge()))
                .child(right),
        )
        .children(marks.preview.clone().filter(|_| searching).map(|p| {
            div()
                .id("preview")
                .text_size(ui::META)
                .line_height(gpui::px(16.))
                .text_color(muted)
                .line_clamp(2)
                .text_ellipsis()
                .child(p)
        }))
        .child(
            h_flex()
                .h(gpui::px(18.))
                .min_w_0()
                .gap_1p5()
                .text_size(ui::META)
                .text_color(muted)
                // Empty under a one-project filter: every row would repeat it.
                .when(!project.is_empty(), |el| {
                    el.child(ui::Monogram::new(&project))
                        // The account keeps its room (it is what SkyeLab is for); the project name gives way first.
                        .child(div().min_w(gpui::px(24.)).truncate().child(project))
                })
                .children(marks.machine.clone().map(|m| {
                    h_flex()
                        .flex_shrink_0()
                        .max_w(gpui::px(96.))
                        .gap_0p5()
                        .child(Icon::new(SkyeIcon::Monitor).size_3().flex_shrink_0())
                        .child(div().min_w_0().truncate().child(m))
                }))
                .children(account.map(|badge| {
                    div()
                        .flex_shrink_0()
                        .max_w(gpui::px(112.))
                        .truncate()
                        .child(badge)
                }))
                .child(div().flex_1())
                .children(marks.pr.as_ref().map(|pr| {
                    use crate::workspace::pr_watch::{checks_look, state_look};
                    let (icon, color, _) = state_look(pr.state, theme);
                    // Checks only matter while the PR can still merge.
                    let checks = checks_look(pr.checks, theme)
                        .filter(|_| !pr.state.is_some_and(skyelab_git::PrState::is_done));
                    h_flex()
                        .flex_shrink_0()
                        .gap_0p5()
                        .text_color(color)
                        .child(Icon::new(icon).size_3())
                        .child(pr.label.clone())
                        .children(checks.map(|(icon, color, _)| {
                            Icon::new(icon).size_3().ml_0p5().text_color(color)
                        }))
                }))
                .children(marks.branch.clone().map(|b| {
                    h_flex()
                        .min_w_0()
                        .min_w(gpui::px(36.))
                        .max_w(gpui::px(104.))
                        .gap_0p5()
                        .child(Icon::new(SkyeIcon::GitBranch).size_3().flex_shrink_0())
                        .child(div().min_w_0().truncate().child(b))
                })),
        )
}

/// A comparison group's "×3": shows or hides its threads under the row, without opening the Compare page.
fn group_badge(
    count: usize,
    open: bool,
    group: i64,
    theme: &gpui_component::Theme,
) -> impl IntoElement {
    h_flex()
        .id(SharedString::from(format!("compare-badge-{group}")))
        .debug_selector(move || format!("compare-badge-{group}"))
        .flex_shrink_0()
        .h(gpui::px(18.))
        .px_1()
        .gap_0p5()
        .rounded(ui::RADIUS_SM)
        .bg(theme.foreground.opacity(0.07))
        .hover(|this| this.bg(theme.foreground.opacity(0.12)))
        .text_size(ui::META)
        .font_weight(gpui::FontWeight::MEDIUM)
        .text_color(theme.muted_foreground)
        .child(format!("×{count}"))
        .child(
            Icon::new(if open {
                SkyeIcon::ChevronUp
            } else {
                SkyeIcon::ChevronDown
            })
            .size_3(),
        )
        .on_click(move |_, window, cx| {
            cx.stop_propagation();
            dispatch(
                Box::new(crate::workspace::compare::ToggleCompareGroup { group }),
                window,
                cx,
            )
        })
}

/// A thread a scheduled task started.
fn scheduled_badge() -> ui::Pill {
    ui::Pill::new("Scheduled")
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
    let scheduled = marks.is_some_and(|m| m.scheduled);
    // A project row is never lifted (T3 keeps project headers flat): a selected one is only set in medium weight.
    ui::row(item.id.clone(), item.selected && !is_project, theme)
        .when(item.selected && is_project, |this| {
            this.font_weight(gpui::FontWeight::MEDIUM)
        })
        .on_click(move |_, window, cx| dispatch(action.boxed_clone(), window, cx))
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
                Icon::new(SkyeIcon::Pin)
                    .size_3()
                    .text_color(theme.muted_foreground),
            )
        })
        .when(scheduled, |this| this.child(scheduled_badge()))
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
        .icon(SkyeIcon::Filter)
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
            // No icon: the check rows above have none, so one would push this label out of line.
            menu.separator()
                .menu("Add project", Box::new(super::super::AddProject))
        })
}
