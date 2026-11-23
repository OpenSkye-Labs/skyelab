//! A thread row's hover card, after T3's thread tooltip: on hover, after a short delay, a card to the row's right
//! says where the thread runs (project folder, worktree, branch, pull request), on which account and through which
//! folder binding (SkyeLab's point), and when it was created and last updated.

use super::sidebar::{RowAccount, RowCard};
use super::*;
use gpui::{Anchor, AnyElement};
use gpui_component::hover_card::HoverCard;
use gpui_kit_assets::IconName as Lucide;
use std::rc::Rc;
use std::time::Duration;

/// T3's tooltip delay: long enough that moving down the list doesn't flash cards.
const OPEN_DELAY: Duration = Duration::from_millis(500);

/// `row` as the trigger of its card; `row` as it is when it has none.
pub(super) fn with_card(
    id: &SharedString,
    row: impl IntoElement + 'static,
    card: Option<Rc<RowCard>>,
) -> AnyElement {
    let Some(card) = card else {
        return row.into_any_element();
    };
    HoverCard::new(SharedString::from(format!("card-{id}")))
        .anchor(Anchor::LeftCenter)
        .open_delay(OPEN_DELAY)
        .trigger_style({
            let mut s = gpui::StyleRefinement::default();
            s.size.width = Some(gpui::relative(1.).into());
            s
        })
        .trigger(row)
        .content(move |_, _, cx| render_card(&card, cx))
        .into_any_element()
}

fn render_card(card: &RowCard, cx: &App) -> impl IntoElement + use<> {
    let theme = cx.theme();
    let muted = theme.muted_foreground;
    let line = |icon: Lucide, text: SharedString| {
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
        line(Lucide::GitPullRequest, shown)
            .id("card-pr")
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
                .child(line(Lucide::Folder, card.path.clone()))
                .children(card.worktree.clone().map(|w| line(Lucide::FolderGit2, w)))
                .children(card.branch.clone().map(|b| line(Lucide::GitBranch, b)))
                .children(pr)
                .children(account)
                .children(card.binding.clone().map(|b| line(Lucide::Lock, b)))
                .child(line(Lucide::Clock, times.into())),
        )
}
