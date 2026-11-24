//! Acceptance: the palette groups and ranks its entries like T3's (CommandPalette.logic.ts).

use super::*;

fn e(label: &str, kind: Kind) -> Entry {
    Entry::new(label, kind, Rc::new(|_, _| {}))
}

fn action(label: &str) -> Entry {
    e(label, Kind::Action("skyelab::Test"))
}

fn thread(label: &str, recency: i64) -> Entry {
    e(label, Kind::Thread).recency(recency)
}

/// Each group's label and its rows' labels.
fn shown(groups: &[Group], entries: &[Entry]) -> Vec<(String, Vec<String>)> {
    groups
        .iter()
        .map(|g| {
            let rows = g
                .rows
                .iter()
                .map(|&i| entries[i].label.to_string())
                .collect();
            (g.label.to_string(), rows)
        })
        .collect()
}

#[test]
fn empty_query_lists_actions_then_recent_threads_newest_first_then_projects() {
    let mut entries = vec![
        e("Switch to project: demo", Kind::Project),
        thread("Open thread: old", 1),
        action("Open Settings"),
        thread("Open thread: new", 9),
        e("Go to tab: Terminal", Kind::Tab),
    ];
    entries.extend((0..20).map(|i| thread(&format!("Open thread: t{i}"), 100 + i)));
    let nested = entries.len();
    entries.push(e("Theme: Dawn", Kind::Action("skyelab::Theme")));
    let groups = arrange("", &entries, nested, entries.len());
    let got = shown(&groups, &entries);
    let labels: Vec<&str> = got.iter().map(|(l, _)| l.as_str()).collect();
    assert_eq!(labels, ["Actions", "Recent threads", "Projects", "Tabs"]);
    let recent = &got[1].1;
    assert_eq!(recent.len(), 12, "T3 lists at most 12 recent threads");
    assert_eq!(recent[0], "Open thread: t19", "newest first");
    assert_eq!(recent[11], "Open thread: t8");
    assert!(
        !got.iter()
            .any(|(_, rows)| rows.iter().any(|r| r == "Theme: Dawn")),
        "a submenu's items stay in their page until searched"
    );
}

#[test]
fn a_query_ranks_every_entry_and_orders_groups_by_their_best_hit() {
    let entries = vec![
        action("Toggle Sidebar"),
        e("Switch to project: mysettings", Kind::Project),
        action("Reset Zoom"),
        action("Open Settings"),
        e("Theme: Dawn", Kind::Action("skyelab::Theme")),
    ];
    let groups = arrange("settings", &entries, 4, 5);
    let got = shown(&groups, &entries);
    assert_eq!(got.len(), 2, "{got:?}");
    assert_eq!(got[0].0, "Actions");
    assert_eq!(got[0].1, ["Open Settings"]);
    assert_eq!(got[1].0, "Projects");

    // A submenu's items are searched too.
    let groups = arrange("dawn", &entries, 4, 5);
    assert_eq!(
        shown(&groups, &entries),
        [("Actions".to_owned(), vec!["Theme: Dawn".to_owned()])]
    );
}

#[test]
fn message_search_results_join_the_threads_group_after_the_ranked_ones() {
    let entries = vec![
        thread("Open thread: walrus notes", 1),
        action("Open Settings"),
        // Found by message text: its title doesn't match, it still lists.
        thread("Open thread: three · other", 2),
    ];
    let groups = arrange("walrus", &entries, 2, 2);
    assert_eq!(
        shown(&groups, &entries),
        [(
            "Threads".to_owned(),
            vec![
                "Open thread: walrus notes".to_owned(),
                "Open thread: three · other".to_owned()
            ]
        )]
    );
}

#[test]
fn prefix_beats_word_start_beats_mid_word() {
    let entries = vec![
        action("Unterminated"),
        action("New Terminal"),
        action("Terminal Here"),
    ];
    let groups = arrange("term", &entries, 3, 3);
    assert_eq!(
        shown(&groups, &entries)[0].1,
        ["Terminal Here", "New Terminal", "Unterminated"]
    );
}

#[test]
fn match_ranges_mark_the_matched_letters_and_the_bold_part() {
    assert_eq!(match_ranges("set", "Open Settings", None), vec![5..8]);
    assert_eq!(match_ranges("os", "Open Settings", None), vec![0..1, 5..6]);
    assert!(match_ranges("", "Open Settings", None).is_empty());
    assert!(match_ranges("xyz", "Open Settings", None).is_empty());
    // "New thread in **demo**", plus a match inside it.
    assert_eq!(
        match_ranges("new", "New thread in demo", Some(14..18)),
        vec![0..3, 14..18]
    );
    // Byte ranges stay on char boundaries.
    assert_eq!(match_ranges("é", "Café", None), vec![3..5]);
}

#[test]
fn a_submenu_item_found_by_a_query_lists_under_its_submenu_not_its_kind() {
    // "New thread in…" items are Kind::Project; listed under Projects they'd look like "Switch to project".
    let entries = flatten(vec![
        e("Switch to project: demo", Kind::Project),
        action("New thread in…")
            .submenu(vec![e("New thread in: demo", Kind::Project).title("demo")]),
    ]);
    let groups = arrange("demo", &entries, 2, entries.len());
    assert_eq!(
        shown(&groups, &entries),
        [
            (
                "Projects".to_owned(),
                vec!["Switch to project: demo".to_owned()]
            ),
            (
                "New thread in…".to_owned(),
                vec!["New thread in: demo".to_owned()]
            ),
        ]
    );
}
