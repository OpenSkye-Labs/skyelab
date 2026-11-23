//! Settle and snooze (T3's): rows move to their shelves and back with undo, the Settled shelf pages 25 at a time, a
//! passed snooze lists as Woke until opened, and the Working section shows its count and folds.

use super::super::super::manage_harness::{Rig, rig};
use super::*;
use gpui::{TestAppContext, VisualTestContext};

fn shelve(id: i64, to: Shelf, cx: &mut VisualTestContext) {
    cx.dispatch_action(ShelveThread { id, to });
    cx.run_until_parked();
}

/// The titles in shelf `title` (empty when it isn't shown).
fn shelf(r: &Rig, title: &str, cx: &mut VisualTestContext) -> Vec<String> {
    let sidebar = r.ws.read_with(cx, |ws, _| ws.sidebar.clone());
    sidebar.read_with(cx, |s, _| {
        s.shelves
            .iter()
            .filter(|s| s.title.as_ref() == title)
            .flat_map(|s| s.items.iter().map(|i| i.label.to_string()))
            .collect()
    })
}

fn sidebar(
    r: &Rig,
    cx: &mut VisualTestContext,
) -> gpui::Entity<super::super::super::panels::Sidebar> {
    r.ws.read_with(cx, |ws, _| ws.sidebar.clone())
}

#[gpui::test]
fn settle_moves_to_the_shelf_with_undo_and_unsettles(cx: &mut TestAppContext) {
    let (r, cx) = rig(cx);
    let a = r.thread("alpha", cx);
    r.thread("beta", cx);
    shelve(a, Shelf::Settle, cx);
    assert_eq!(r.listed(cx), ["beta"]);
    assert_eq!(shelf(&r, "Settled", cx), ["alpha"]);
    assert!(r.row(a).expect("row").settled_at.is_some(), "stored");
    assert!(r.notices(cx).iter().any(|n| n == "Settled “alpha”"));

    // The notice's Undo.
    r.ws.update_in(cx, |ws, window, cx| ws.settle(a, false, false, window, cx));
    cx.run_until_parked();
    assert_eq!(r.listed(cx), ["beta", "alpha"]);
    assert!(
        shelf(&r, "Settled", cx).is_empty(),
        "an empty shelf is not shown"
    );

    shelve(a, Shelf::Settle, cx);
    shelve(a, Shelf::Unsettle, cx);
    assert!(r.row(a).expect("row").settled_at.is_none());
    assert_eq!(r.listed(cx), ["beta", "alpha"]);
}

#[gpui::test]
fn settled_shelf_folds_and_pages(cx: &mut TestAppContext) {
    let (r, cx) = rig(cx);
    let ids: Vec<i64> = (0..30).map(|i| r.thread(&format!("t{i}"), cx)).collect();
    {
        let s = r.f.store.lock().expect("lock");
        ids.iter()
            .for_each(|&id| s.set_thread_settled(id, true).expect("settle"));
    }
    r.reload(cx);
    let side = sidebar(&r, cx);
    assert_eq!(
        shelf(&r, "Settled", cx).len(),
        30,
        "the header counts all 30"
    );
    assert_eq!(
        side.read_with(cx, |s, _| s.drawn("Settled")),
        (0, false),
        "folded at first"
    );
    side.update(cx, |s, cx| s.toggle(&"Settled".into(), cx));
    assert_eq!(
        side.read_with(cx, |s, _| s.drawn("Settled")),
        (25, true),
        "a page, then Show 5 more"
    );
    side.update(cx, |s, cx| s.show_more(cx));
    assert_eq!(side.read_with(cx, |s, _| s.drawn("Settled")), (30, false));
    r.reload(cx);
    assert_eq!(
        side.read_with(cx, |s, _| s.drawn("Settled")),
        (30, false),
        "kept across a reload"
    );
}

#[gpui::test]
fn snooze_hides_then_a_passed_one_is_woke_until_opened(cx: &mut TestAppContext) {
    let (r, cx) = rig(cx);
    let a = r.thread("alpha", cx);
    let b = r.thread("beta", cx);
    let later = crate::ui::now_ms() + 3_600_000;
    shelve(a, Shelf::Snooze(later), cx);
    assert_eq!(r.listed(cx), ["beta"]);
    assert_eq!(shelf(&r, "Snoozed", cx), ["alpha"]);
    assert!(r.notices(cx).iter().any(|n| n == "Snoozed “alpha” for 1h"));
    shelve(a, Shelf::Wake, cx);
    assert_eq!(r.listed(cx), ["beta", "alpha"], "woken by hand");
    assert_eq!(r.row(a).expect("row").snoozed_until, None);

    // Its time passed while nobody looked: listed again, marked woke, until it is opened.
    r.f.store
        .lock()
        .expect("lock")
        .set_thread_snoozed(b, Some(1))
        .expect("snooze");
    r.reload(cx);
    assert!(r.listed(cx).contains(&"beta".to_string()));
    let woke = |cx: &mut VisualTestContext| {
        let side = sidebar(&r, cx);
        side.read_with(cx, |s, _| {
            s.marks[&gpui::SharedString::from(format!("thread-{b}"))]
                .shelf
                .is_some_and(|s| {
                    skyelab_core::snooze::is_woke(s.snoozed_until, crate::ui::now_ms())
                })
        })
    };
    assert!(woke(cx));
    r.open(b, cx);
    cx.run_until_parked();
    assert!(!woke(cx), "opening it clears Woke");
    assert_eq!(r.row(b).expect("row").snoozed_until, None, "stored");
}

#[gpui::test]
fn working_header_counts_and_folds(cx: &mut TestAppContext) {
    let (r, cx) = rig(cx);
    let id = r.thread("busy", cx);
    r.thread("quiet", cx);
    r.f.store
        .lock()
        .expect("lock")
        .set_thread_status(id, "running")
        .expect("status");
    r.reload(cx);
    let side = sidebar(&r, cx);
    assert!(side.read_with(cx, |s, _| {
        s.sections
            .iter()
            .any(|s| s.title.as_ref() == "Working" && s.foldable)
    }));
    assert_eq!(side.read_with(cx, |s, _| s.drawn("Working")), (1, false));
    side.update(cx, |s, cx| s.toggle(&"Working".into(), cx));
    assert_eq!(
        side.read_with(cx, |s, _| s.drawn("Working")),
        (0, false),
        "folded"
    );
    r.reload(cx);
    assert_eq!(
        side.read_with(cx, |s, _| s.drawn("Working")),
        (0, false),
        "stays folded"
    );
}
