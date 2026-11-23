//! A terminal tab's split tree: splitting both ways, closing, resizing, and the saved form round-tripping through
//! settings.toml.

use skyelab_core::terminal_layout::{Axis, MIN_SHARE, PaneTree, SavedTerminalTab, WHOLE};
use skyelab_core::{AppSettings, WindowLayout};
use std::path::PathBuf;

fn sizes(t: &PaneTree<u32>) -> Vec<u32> {
    match t {
        PaneTree::Split { sizes, .. } => sizes.clone(),
        PaneTree::Leaf(_) => vec![],
    }
}

#[test]
fn same_axis_adds_a_sibling_and_cross_axis_nests() {
    let mut t = PaneTree::Leaf(1);
    assert!(t.split(&1, 2, Axis::Row));
    assert_eq!(sizes(&t), vec![WHOLE / 2, WHOLE / 2]);
    // Splitting pane 2 the same way: a third sibling sharing pane 2's half.
    assert!(t.split(&2, 3, Axis::Row));
    assert_eq!(t.leaves(), vec![&1, &2, &3]);
    assert_eq!(sizes(&t), vec![5_000, 2_500, 2_500]);
    // Stacked under pane 1: pane 1 becomes a column of 1 and 4.
    assert!(t.split(&1, 4, Axis::Column));
    assert_eq!(t.leaves(), vec![&1, &4, &2, &3]);
    let PaneTree::Split { children, .. } = &t else {
        panic!("a split")
    };
    assert!(matches!(
        &children[0],
        PaneTree::Split {
            axis: Axis::Column,
            ..
        }
    ));
    assert!(
        !t.split(&99, 5, Axis::Row),
        "an unknown pane splits nothing"
    );
    assert_eq!(t.leaves().len(), 4);
}

#[test]
fn closing_collapses_and_gives_the_share_to_a_neighbour() {
    let mut t = PaneTree::Leaf(1);
    t.split(&1, 2, Axis::Row);
    t.split(&2, 3, Axis::Row);
    t.split(&1, 4, Axis::Column);
    // Pane 4 leaves its column: the column of one becomes pane 1 again, at its old share.
    let t = t.remove(&4).expect("panes left");
    assert_eq!(t.leaves(), vec![&1, &2, &3]);
    assert_eq!(sizes(&t), vec![5_000, 2_500, 2_500]);
    // A middle pane's share goes to the one before it.
    let t = t.remove(&2).expect("panes left");
    assert_eq!(sizes(&t), vec![7_500, 2_500]);
    // The first pane's share goes to the next.
    let t = t.remove(&1).expect("one left");
    assert_eq!(t, PaneTree::Leaf(3), "a split of one is its child");
    assert_eq!(t.remove(&3), None, "the last pane closes the tab");
}

#[test]
fn resize_moves_one_divider_within_limits() {
    let mut t = PaneTree::Leaf(1);
    t.split(&1, 2, Axis::Row);
    t.split(&2, 3, Axis::Row); // 5000 2500 2500
    assert!(t.resize(&[], 0, 0.3));
    assert_eq!(sizes(&t), vec![3_000, 4_500, 2_500], "the third pane stays");
    assert!(t.resize(&[], 1, 0.99));
    assert_eq!(
        sizes(&t),
        vec![3_000, 4_500 + 2_500 - MIN_SHARE, MIN_SHARE],
        "never below the minimum"
    );
    assert!(t.resize(&[], 0, 0.0));
    assert_eq!(sizes(&t)[0], MIN_SHARE);
    assert_eq!(sizes(&t).iter().sum::<u32>(), WHOLE);
    assert!(!t.resize(&[], 2, 0.5), "no divider after the last pane");
    assert!(!t.resize(&[7], 0, 0.5), "no such split");

    t.split(&1, 4, Axis::Column);
    assert!(t.resize(&[0], 0, 0.25), "a nested split");
    let PaneTree::Split { children, .. } = &t else {
        panic!("a split")
    };
    assert_eq!(sizes(&children[0]), vec![2_500, 7_500]);
}

#[test]
fn a_bad_tree_from_a_file_is_repaired() {
    let bad: PaneTree<u32> = PaneTree::Split {
        axis: Axis::Row,
        children: vec![PaneTree::Leaf(1), PaneTree::Leaf(2), PaneTree::Leaf(3)],
        sizes: vec![1, 2],
    };
    assert_eq!(
        sizes(&bad.repaired().expect("panes")),
        vec![3_334, 3_333, 3_333]
    );
    let empty: PaneTree<u32> = PaneTree::Split {
        axis: Axis::Row,
        children: vec![],
        sizes: vec![],
    };
    assert_eq!(empty.repaired(), None);
}

#[test]
fn saved_tabs_round_trip_through_settings_toml() {
    let mut panes = PaneTree::Leaf(PathBuf::from("/work/app"));
    panes.split(
        &PathBuf::from("/work/app"),
        PathBuf::from("/work/app/web"),
        Axis::Row,
    );
    panes.split(
        &PathBuf::from("/work/app/web"),
        PathBuf::from("/tmp"),
        Axis::Column,
    );
    panes.resize(&[], 0, 0.7);
    let tabs = vec![
        SavedTerminalTab {
            thread: Some(42),
            name: Some("Dev server".into()),
            panes,
        },
        SavedTerminalTab {
            thread: None,
            name: None,
            panes: PaneTree::Leaf(PathBuf::from("/home/me")),
        },
    ];
    let settings = AppSettings {
        windows: vec![WindowLayout {
            width: 1200,
            height: 800,
            terminals: tabs.clone(),
            ..Default::default()
        }],
        ..Default::default()
    };
    let dir = tempfile::tempdir().expect("temp dir");
    let path = dir.path().join("settings.toml");
    settings.save(&path).expect("saved");
    let back = AppSettings::load(&path).expect("loads");
    assert_eq!(back.windows[0].terminals, tabs);

    // A file from before terminals were saved still loads.
    std::fs::write(&path, "[[windows]]\nwidth = 10\nheight = 10\n").expect("write");
    assert!(
        AppSettings::load(&path).expect("old file").windows[0]
            .terminals
            .is_empty()
    );
}

#[test]
fn terminal_close_confirmation_is_on_by_default() {
    assert!(AppSettings::default().terminal.confirm_close);
}
