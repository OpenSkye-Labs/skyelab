//! The sidebar: the environment picker, then projects with the selected one's threads under it (T3's folder layout).

use super::*;

/// Which account a project row runs on.
#[derive(Clone)]
pub enum RowAccount {
    Bound(skyelab_core::Account),
    Unbound,
    /// Not shown: still resolving in the background, or bound on another machine.
    Hidden,
}

/// A row in a sidebar section. `id` is the domain id (project path, thread id), so it stays stable.
pub struct SidebarItem {
    pub id: SharedString,
    pub label: SharedString,
    pub selected: bool,
    /// Dispatched when the row is clicked.
    pub action: Box<dyn Action>,
    /// Thread rows: the status dot.
    pub status: Option<ui::Status>,
    /// Thread rows: when the thread last changed (ms since the epoch), shown as "3m".
    pub updated_ms: Option<i64>,
    /// Project rows: the account the folder runs on.
    pub account: Option<RowAccount>,
}

/// One titled group in the sidebar, with what to show while it is empty.
pub struct SidebarSection {
    pub title: SharedString,
    pub items: Vec<SidebarItem>,
    pub empty_title: SharedString,
    pub empty_hint: SharedString,
    /// A small button next to the title, e.g. "Add".
    pub button: Option<(SharedString, Box<dyn Action>)>,
    /// Drawn under the previous section's selected row (a project's threads, as in T3). With no row selected
    /// there, it is drawn as its own section.
    pub nested: bool,
}

pub struct Sidebar {
    pub sections: Vec<SidebarSection>,
    /// The environment switcher, drawn in place of the title once the workspace sets it.
    pub picker: Option<Entity<super::super::env_picker::EnvPicker>>,
    pub(super) focus: FocusHandle,
}

impl Sidebar {
    pub fn new(sections: Vec<SidebarSection>, cx: &mut Context<Self>) -> Self {
        Self {
            sections,
            picker: None,
            focus: cx.focus_handle(),
        }
    }
}

// The sidebar draws its own header.
panel!(Sidebar, "Sidebar", closable: false, title_bar: false, toggles: false, |_this| "Sidebar");

fn section_button(s: &SidebarSection) -> Option<Button> {
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
const THREAD_INDENT: f32 = 36.;

fn sidebar_row(
    item: &SidebarItem,
    indent: bool,
    now: i64,
    theme: &gpui_component::Theme,
) -> impl IntoElement {
    let action = item.action.boxed_clone();
    let is_project = item.account.is_some();
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
                .child(item.label.clone()),
        )
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

fn rows(s: &SidebarSection, indent: bool, now: i64, theme: &gpui_component::Theme) -> gpui::Div {
    if s.items.is_empty() {
        return v_flex()
            .when(indent, |this| this.pl(gpui::px(THREAD_INDENT - 8.)))
            .child(ui::EmptyState::new(s.empty_title.clone()).hint(s.empty_hint.clone()));
    }
    v_flex().gap_0p5().children(
        s.items
            .iter()
            .map(|item| sidebar_row(item, indent, now, theme)),
    )
}

impl Render for Sidebar {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let now = ui::now_ms();
        let mut blocks = Vec::new();
        let mut i = 0;
        while let Some(s) = self.sections.get(i) {
            // A project's threads go under its row (T3's folder layout); with no project selected they stand alone.
            let nested = self
                .sections
                .get(i + 1)
                .filter(|n| n.nested && s.items.iter().any(|it| it.selected));
            let body =
                match nested {
                    None => rows(s, false, now, theme),
                    Some(n) => v_flex().gap_0p5().children(s.items.iter().map(|item| {
                        let row = sidebar_row(item, false, now, theme).into_any_element();
                        if !item.selected {
                            return row;
                        }
                        v_flex()
                            .gap_0p5()
                            .child(row)
                            .child(rows(n, true, now, theme))
                            .children(section_button(n).map(|b| {
                                h_flex().pl(gpui::px(THREAD_INDENT - 10.)).pb_1().child(b)
                            }))
                            .into_any_element()
                    })),
                };
            blocks.push(
                v_flex()
                    .gap_1()
                    .child(
                        ui::SectionHeader::new(s.title.clone())
                            .when_some(section_button(s), |h, b| h.trailing(b)),
                    )
                    .child(body),
            );
            i += if nested.is_some() { 2 } else { 1 };
        }
        v_flex()
            .id("sidebar")
            .track_focus(&self.focus)
            .size_full()
            .overflow_y_scroll()
            .px_2()
            .pb_3()
            .gap_4()
            .bg(theme.sidebar)
            .text_color(theme.sidebar_foreground)
            // The window's top row: on macOS the traffic lights sit at its start, so the picker starts after them.
            .child(
                div()
                    .min_h(ui::TOP_ROW_H)
                    .flex()
                    .flex_col()
                    .justify_center()
                    .when(cfg!(target_os = "macos"), |d| {
                        d.pl(ui::TRAFFIC_LIGHTS_W - gpui::px(8.))
                    })
                    .child(match &self.picker {
                        Some(picker) => picker.clone().into_any_element(),
                        None => div()
                            .px_2p5()
                            .text_size(ui::BODY)
                            .font_weight(gpui::FontWeight::MEDIUM)
                            .child("SkyeLab")
                            .into_any_element(),
                    }),
            )
            .children(blocks)
    }
}
