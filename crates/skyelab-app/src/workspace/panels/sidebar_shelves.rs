//! The sidebar's folded shelves (Snoozed, Settled, paged), foldable sections (Working) and footer icon row.

use super::*;
use crate::ui::SkyeIcon;

/// The room an opened shelf needs below its head (about five rows); with less, opening scrolls it up.
const SHELF_ROOM: gpui::Pixels = gpui::px(160.);

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
        let opening = !set.remove(title);
        if opening {
            set.insert(title.clone());
        }
        // An opened shelf near the list's bottom edge would open out of sight: bring its head to the top. Read where
        // it is before the rebuild, which forgets the drawn rows' bounds. Rows before it don't change.
        let head = self
            .entries
            .iter()
            .position(|e| matches!(*e, Entry::ShelfHead(k) if &self.shelves[k].title == title));
        let near_bottom = head
            .filter(|_| opening && shelf)
            .and_then(|h| self.list.bounds_for_item(h))
            .is_some_and(|b| b.bottom() + SHELF_ROOM > self.list.viewport_bounds().bottom());
        self.rows_changed(cx);
        if let Some(head) = head.filter(|_| near_bottom) {
            self.list.scroll_to(gpui::ListOffset {
                item_ix: head,
                offset_in_item: gpui::px(0.),
            });
        }
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
            .aria_label(format!("{title} ({count})"))
            .aria_expanded(open)
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
                .on_click(|_, window, cx| {
                    crate::workspace::dispatch(Box::new(RefreshThreads), window, cx)
                }),
        )
}

/// The archived toggle and rows, then the bottom padding.
fn entries(archived: Option<&ArchivedRows>) -> Vec<Entry> {
    let mut out = Vec::new();
    // "Archived" at the end of the list, as the user guide says.
    if let Some(a) = archived.filter(|a| !a.items.is_empty()) {
        out.push(Entry::ArchivedHead);
        if a.open {
            out.extend((0..a.items.len()).map(Entry::Archived));
        }
    }
    out.push(Entry::End);
    out
}

/// One line of the sidebar's list: section `s`'s header, its row `i` or its empty state, the archived toggle and
/// rows, then the bottom padding.
#[derive(Clone, Copy)]
pub(super) enum Entry {
    Header(usize),
    Row(usize, usize),
    Empty(usize),
    /// Shelf `k`'s toggle row, its row `i`, its "Show N more" row.
    ShelfHead(usize),
    ShelfRow(usize, usize),
    ShelfMore(usize),
    ArchivedHead,
    Archived(usize),
    End,
}
