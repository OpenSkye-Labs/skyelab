//! A thread row's hover card, after T3's thread tooltip: on hover, after a short delay, a card to the row's right
//! says where the thread runs (project folder, worktree, branch, pull request), on which account and through which
//! folder binding (SkyeLab's point), and when it was created and last updated.

use super::sidebar::{RowAccount, RowCard};
use super::*;
use crate::ui::HoverCard;
use crate::ui::SkyeIcon;
use gpui::{Anchor, AnyElement};
use std::rc::Rc;
use std::time::Duration;

/// T3's tooltip delay: long enough that moving down the list doesn't flash cards.
const OPEN_DELAY: Duration = Duration::from_millis(500);

/// Local threads' row cards by thread id, as a sidebar last listed each (a thread no sidebar lists keeps its last
/// card), for the composer's thread and PR chips (T3's thread-chip hover cards).
#[derive(Default)]
pub struct ThreadCards(pub std::collections::HashMap<i64, Rc<RowCard>>);

impl gpui::Global for ThreadCards {}

/// `row` as the trigger of its card; `row` as it is when it has none.
pub(super) fn with_card(
    id: &SharedString,
    row: impl IntoElement + 'static,
    card: Option<Rc<RowCard>>,
) -> AnyElement {
    with_card_at(id, row, card, Anchor::LeftCenter)
}

/// [`with_card`], its card placed at `anchor` of the trigger.
pub fn with_card_at(
    id: &SharedString,
    row: impl IntoElement + 'static,
    card: Option<Rc<RowCard>>,
    anchor: Anchor,
) -> AnyElement {
    let Some(card) = card else {
        return row.into_any_element();
    };
    HoverCard::new(SharedString::from(format!("card-{id}")))
        .anchor(anchor)
        .open_delay(OPEN_DELAY)
        .w_full()
        .trigger(row)
        .content(move |_, cx| render_card(&card, cx))
        .into_any_element()
}

fn render_card(card: &RowCard, cx: &App) -> impl IntoElement + use<> {
    let theme = cx.theme();
    let muted = theme.muted_foreground;
    let line = |icon: SkyeIcon, text: SharedString| {
        h_flex()
            .min_w_0()
            .gap_2()
            .child(Icon::new(icon).size_3().flex_shrink_0().text_color(muted))
            .child(div().min_w_0().truncate().child(text))
    };
    let account = card.account.as_ref().and_then(|a| {
        let badge = match a {
            RowAccount::Bound(acc) => ui::AccountBadge::of(acc),
            RowAccount::Unbound => ui::AccountBadge::unbound(),
            RowAccount::Hidden => return None,
        };
        Some(h_flex().min_w_0().gap_2().child(badge))
    });
    let pr = card.pr_url.clone().map(|url| {
        let shown: SharedString = url.trim_start_matches("https://").to_owned().into();
        line(SkyeIcon::GitPullRequest, shown)
            .id("card-pr")
            .role(gpui::accesskit::Role::Link)
            .aria_label("Open pull request")
            .cursor_pointer()
            .text_color(ui::ink(theme, theme.success))
            .on_click(move |_, _, cx| {
                if crate::workspace::thread_render::opens_in_browser(&url) {
                    cx.open_url(&url);
                }
            })
    });
    let times = format!(
        "Created {} · Updated {}",
        ui::clock(card.created_ms),
        ui::clock(card.updated_ms)
    );
    v_flex()
        .debug_selector(|| "thread-card".into())
        .w(gpui::px(320.))
        .gap_2()
        .px_1()
        .py_1()
        .text_size(ui::META)
        .child(
            div()
                .truncate()
                .font_weight(gpui::FontWeight::MEDIUM)
                .text_color(theme.foreground)
                .child(card.title.clone()),
        )
        .child(
            v_flex()
                .gap_1p5()
                .text_color(theme.foreground.opacity(0.75))
                // A state card (a linked machine not connected) has no folder or times, only its note.
                .when(!card.project.is_empty() || !card.path.is_empty(), |d| {
                    d.child(line(
                        SkyeIcon::Folder,
                        format!("{} · {}", card.project, card.path).into(),
                    ))
                })
                .children(card.worktree.clone().map(|w| line(SkyeIcon::FolderGit2, w)))
                .children(card.branch.clone().map(|b| line(SkyeIcon::GitBranch, b)))
                .children(pr)
                .children(account)
                .children(card.binding.clone().map(|b| line(SkyeIcon::Lock, b)))
                .when(card.created_ms > 0, |d| {
                    d.child(line(SkyeIcon::Clock, times.into()))
                }),
        )
        // Wrapped, never cut: it may hold an error or a code to compare.
        .children(
            card.note
                .clone()
                .map(|n| div().text_color(theme.foreground.opacity(0.75)).child(n)),
        )
}
