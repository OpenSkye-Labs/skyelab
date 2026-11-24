//! A finished turn's fold header, "Worked for 4m 20s ›". The rules (which rows fold, where the header goes) are
//! shared with the phone in `skyelab_agent::timeline::fold`.

use super::thread_items::ItemsView;
use super::thread_model::Item;
use super::timeline_text::divider_text;
use crate::ui;
use crate::ui::SkyeIcon;
use gpui::{AnyElement, Context, SharedString, div, prelude::*};
use gpui_component::{ActiveTheme as _, Icon, h_flex, v_flex};
pub(super) use skyelab_agent::timeline::fold::{
    files_key, fold_head, fold_key, folded_by, reply_of, rows_of,
};

/// Row `ix` as its turn's fold shows it: hidden while folded, under the "Worked for …" header when it is the first
/// folded row, as drawn by `row` otherwise.
pub(super) fn fold_row<V: ItemsView>(
    this: &mut V,
    ix: usize,
    row: impl FnOnce(&mut V, &mut Context<V>) -> AnyElement,
    cx: &mut Context<V>,
) -> AnyElement {
    let items = this.items();
    let Some(done) = folded_by(items, ix) else {
        return row(this, cx);
    };
    let key = fold_key(items, done);
    let open = this.is_expanded(&key);
    let header = (fold_head(items, done) == Some(ix)).then(|| header(&items[done], open, key, cx));
    let body = open.then(|| row(this, cx));
    v_flex()
        .w_full()
        .children(header)
        .children(body)
        .into_any_element()
}

/// "Worked for 4.2s · 12.3k in · 175 out · $0.0421 ›": opens or folds the turn's work.
fn header<V: ItemsView>(done: &Item, open: bool, key: String, cx: &mut Context<V>) -> AnyElement {
    let Item::Done {
        duration_ms,
        cost_usd,
        usage,
        ..
    } = done
    else {
        return div().into_any_element();
    };
    let (muted, fg) = (cx.theme().muted_foreground, cx.theme().foreground);
    let text = divider_text("Worked for", *duration_ms as f64 / 1000., *usage, *cost_usd);
    let id = SharedString::from(key.clone());
    h_flex()
        .w_full()
        .justify_center()
        .px_6()
        .child(
            div().w_full().max_w(ui::CHAT_MAX_W).pt_2().pb_1().child(
                h_flex()
                    .id(id)
                    .debug_selector(|| "turn-fold".into())
                    .gap_1()
                    .px_1()
                    .text_size(ui::META)
                    .text_color(muted)
                    .cursor_pointer()
                    .hover(move |s| s.text_color(fg))
                    .child(text)
                    .child(
                        Icon::new(if open {
                            SkyeIcon::ChevronDown
                        } else {
                            SkyeIcon::ChevronRight
                        })
                        .size_3(),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| this.toggle_tool(&key, cx))),
            ),
        )
        .into_any_element()
}
