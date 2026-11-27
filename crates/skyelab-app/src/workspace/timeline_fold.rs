//! A finished turn's fold header, "Worked for 4m 20s ›". The rules (which rows fold, where the header goes) are
//! shared with the phone in `skyelab_agent::timeline::fold`.

use super::thread_items::ItemsView;
use super::thread_model::Item;
use super::timeline_text::divider_text;
use crate::ui;
use crate::ui::ActiveTheme as _;
use crate::ui::SkyeIcon;
use gpui::{AnyElement, Context, SharedString, div, prelude::*};
use gpui_component::{Icon, h_flex, v_flex};
pub(super) use skyelab_agent::timeline::fold::{
    files_key, fold_head, fold_key, folded_by, reply_of, rows_of, turn_start,
};

/// Row `ix` as its turn's fold shows it: hidden while folded, under the "Worked for …" header when it is the first
/// folded row, as drawn by `row` otherwise. A finished turn with nothing to fold still gets its "Worked for …" line
/// above its reply, so every turn reads the same way (its divider row then draws nothing).
pub(super) fn fold_row<V: ItemsView>(
    this: &mut V,
    ix: usize,
    row: impl FnOnce(&mut V, &mut Context<V>) -> AnyElement,
    cx: &mut Context<V>,
) -> AnyElement {
    let items = this.items();
    let Some(done) = folded_by(items, ix) else {
        let plain = reply_of(items, ix)
            .filter(|&done| fold_head(items, done).is_none() && !is_failed(&items[done]))
            .map(|done| header(&items[done], None, cx));
        return match plain {
            Some(head) => v_flex()
                .w_full()
                .child(head)
                .child(row(this, cx))
                .into_any_element(),
            None => row(this, cx),
        };
    };
    let key = fold_key(items, done);
    let open = this.is_expanded(&key);
    let header =
        (fold_head(items, done) == Some(ix)).then(|| header(&items[done], Some((open, key)), cx));
    let body = open.then(|| row(this, cx));
    v_flex()
        .w_full()
        .children(header)
        .children(body)
        .into_any_element()
}

/// A turn that ended in an error or a stop: its divider says so after its rows.
fn is_failed(done: &Item) -> bool {
    matches!(done, Item::Done { is_error: true, .. })
}

/// Whether the finished turn ending at row `done` drew its "Worked for …" line above a row: above its folded work,
/// or above its reply. Its divider row then shows only the turn's changed files.
pub(super) fn headed(items: &super::thread_model::Items, done: usize) -> bool {
    !is_failed(&items[done])
        && (fold_head(items, done).is_some()
            || (turn_start(items, done)..done).any(|j| reply_of(items, j) == Some(done)))
}

/// T3's turn row: "Worked for 4.2s ›", then the turn's tokens and cost in a fainter tone. With `fold` (open, key) it
/// opens or folds the turn's work; without it the turn had nothing to fold and the line has no chevron.
fn header<V: ItemsView>(
    done: &Item,
    fold: Option<(bool, String)>,
    cx: &mut Context<V>,
) -> AnyElement {
    let Item::Done {
        duration_ms,
        cost_usd,
        usage,
        ..
    } = done
    else {
        return div().into_any_element();
    };
    let theme = cx.theme();
    let (muted, fg, border) = (theme.muted_foreground, theme.foreground, theme.border);
    let secs = *duration_ms as f64 / 1000.;
    let label = divider_text("Worked for", secs, None, 0.);
    let full = divider_text("Worked for", secs, *usage, *cost_usd);
    let meta = full
        .strip_prefix(&label)
        .map(|m| m.trim_start_matches(" · ").to_owned())
        .filter(|m| !m.is_empty());
    let toggle = h_flex()
        .id(SharedString::from(match &fold {
            Some((_, key)) => key.clone(),
            None => format!("turn-head-{label}"),
        }))
        .gap_1()
        .px_1()
        .rounded(ui::RADIUS_MD)
        .text_size(ui::BODY)
        .text_color(muted)
        .child(label.clone())
        .when_some(fold, |el, (open, key)| {
            el.role(gpui::accesskit::Role::Button)
                .aria_label(label)
                .aria_expanded(open)
                .debug_selector(|| "turn-fold".into())
                .cursor_pointer()
                .hover(move |s| s.text_color(fg))
                .child(
                    Icon::new(if open {
                        SkyeIcon::ChevronDown
                    } else {
                        SkyeIcon::ChevronRight
                    })
                    .size_3p5(),
                )
                .on_click(cx.listener(move |this, _, _, cx| this.toggle_tool(&key, cx)))
        });
    h_flex()
        .w_full()
        .justify_center()
        .px_6()
        .child(
            h_flex()
                .w_full()
                .max_w(ui::CHAT_MAX_W)
                .pt_2()
                .pb_1()
                .mb_1()
                .gap_2()
                .border_b_1()
                .border_color(border.opacity(0.6))
                .child(toggle)
                .children(meta.map(|m| {
                    div()
                        .text_size(ui::META)
                        .text_color(muted.opacity(0.75))
                        .child(m)
                })),
        )
        .into_any_element()
}
