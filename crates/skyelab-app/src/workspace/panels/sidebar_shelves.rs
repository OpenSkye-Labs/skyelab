//! The sidebar's folded shelves (Snoozed, Settled, paged), foldable sections (Working) and footer icon row.

use super::*;
use crate::ui::SkyeIcon;

impl Sidebar {
    pub(super) fn entries(&self) -> Vec<Entry> {
        let mut out = Vec::new();
        for (s, section) in self.sections.iter().enumerate() {
            out.push(Entry::Header(s));
            if section.items.is_empty() {
                out.push(Entry::Empty(s));
            }
            if !(section.foldable && self.folded.contains(&section.title)) {
                out.extend((0..section.items.len()).map(|i| Entry::Row(s, i)));
            }
        }
        for (k, shelf) in self.shelves.iter().enumerate() {
            out.push(Entry::ShelfHead(k));
            if self.opened.contains(&shelf.title) {
                let shown = self.shelf_shown(shelf);
                out.extend((0..shown).map(|i| Entry::ShelfRow(k, i)));
                if shown < shelf.items.len() {
                    out.push(Entry::ShelfMore(k));
                }
            }
        }
        out.extend(entries(self.archived.as_ref()));
        out
    }

    pub(super) fn shelf_shown(&self, shelf: &Shelf) -> usize {
        match shelf.paged {
            true => shelf.items.len().min(self.paged_rows),
            false => shelf.items.len(),
        }
    }

    /// Opens or folds a shelf, or folds or opens a foldable section, by title.
    pub fn toggle(&mut self, title: &SharedString, cx: &mut Context<Self>) {
        let shelf = self.shelves.iter().any(|s| &s.title == title);
        let set = if shelf {
            &mut self.opened
        } else {
            &mut self.folded
        };
        if !set.remove(title) {
            set.insert(title.clone());
        }
        self.rows_changed(cx);
    }

    /// How many rows section or shelf `title` draws now, and whether a "Show N more" row follows.
    #[cfg(test)]
    pub fn drawn(&self, title: &str) -> (usize, bool) {
        let rows = self.entries.iter().filter(|e| match **e {
            Entry::Row(s, _) => self.sections[s].title.as_ref() == title,
            Entry::ShelfRow(k, _) => self.shelves[k].title.as_ref() == title,
            _ => false,
        });
        let more = self
            .entries
            .iter()
            .any(|e| matches!(*e, Entry::ShelfMore(k) if self.shelves[k].title.as_ref() == title));
        (rows.count(), more)
    }

    /// The paged shelf's "Show N more".
    pub fn show_more(&mut self, cx: &mut Context<Self>) {
        self.paged_rows += SETTLED_PAGE;
        self.rows_changed(cx);
    }

    /// A foldable section's or shelf's header (T3's "Working 1 ⌄", "Settled 227 ›"): title, count, chevron.
    pub(super) fn fold_head(
        &self,
        title: &SharedString,
        count: usize,
        open: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = cx.theme();
        let t = title.clone();
        ui::row(SharedString::from(format!("fold-{title}")), false, theme)
            .debug_selector({
                let s = format!("fold-{title}");
                move || s.clone()
            })
            .text_size(ui::META)
            .font_weight(gpui::FontWeight::MEDIUM)
            .text_color(theme.muted_foreground)
            .on_click(cx.listener(move |this, _, _, cx| this.toggle(&t, cx)))
            .child(div().flex_1().child(title.clone()))
            .child(
                div()
                    .font_weight(gpui::FontWeight::NORMAL)
                    .child(count.to_string()),
            )
            .child(
                Icon::new(if open {
                    SkyeIcon::ChevronDown
                } else {
                    SkyeIcon::ChevronRight
                })
                .size_3(),
            )
            .into_any_element()
    }
}

/// The footer's icon row (T3's `SidebarChrome`): 32 px ghost icon buttons with tooltips.
pub(super) fn footer() -> impl IntoElement {
    use crate::workspace::gh_pages::{OpenIssues, OpenPullRequests};
    use crate::workspace::thread_list::RefreshThreads;
    let button = |id: &'static str, icon: Icon, tip: &'static str, action: Box<dyn Action>| {
        Button::new(id)
            .ghost()
            .icon(icon)
            .tooltip(tip)
            .on_click(move |_, window, cx| window.dispatch_action(action.boxed_clone(), cx))
    };
    h_flex()
        .flex_shrink_0()
        .px_2()
        .py_2()
        .gap_0p5()
        .child(button(
            "sidebar-settings",
            Icon::new(SkyeIcon::Settings),
            "Settings",
            Box::new(crate::workspace::OpenSettings),
        ))
        .child(button(
            "sidebar-prs",
            Icon::new(SkyeIcon::GitPullRequest),
            "Pull requests",
            Box::new(OpenPullRequests),
        ))
        .child(button(
            "sidebar-issues",
            Icon::new(SkyeIcon::CircleDot),
            "Issues",
            Box::new(OpenIssues),
        ))
        .child(button(
            "sidebar-usage",
            Icon::new(SkyeIcon::ChartNoAxesColumn),
            "Usage",
            Box::new(crate::workspace::OpenUsage),
        ))
        .child(div().flex_1())
        .child(button(
            "sidebar-refresh",
            Icon::new(SkyeIcon::RefreshCw),
            "Refresh",
            Box::new(RefreshThreads),
        ))
}
