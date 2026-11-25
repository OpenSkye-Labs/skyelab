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

#[cfg(test)]
pub(crate) const FOOTER_BUTTON_IDS: &[&str] = &["sidebar-refresh"];

/// The sidebar footer: keeps Refresh accessible while primary navigation lives in the left rail. The update pill
/// (T3's `SidebarUpdateCapsule`), when there is one, takes the free space before Refresh.
pub(super) fn footer(update: Option<gpui::AnyElement>) -> impl IntoElement {
    use crate::workspace::thread_list::RefreshThreads;
    h_flex()
        .flex_shrink_0()
        .px_2()
        .py_2()
        .gap_0p5()
        .child(update.unwrap_or_else(|| div().flex_1().into_any_element()))
        .child(
            Button::new("sidebar-refresh")
                .debug_selector(|| "sidebar-refresh".into())
                .ghost()
                .icon(Icon::new(SkyeIcon::RefreshCw))
                .tooltip("Refresh")
                .on_click(|_, window, cx| window.dispatch_action(Box::new(RefreshThreads), cx)),
        )
}
