//! Acceptance: the nav search finds settings by any word, `/` focuses it from the modal, and a `/` typed into a
//! field still lands in the field.

use super::*;
use gpui::TestAppContext;

#[test]
fn search_finds_settings_by_title_section_or_keyword() {
    let found = nav::search("wrap");
    assert!(
        found.contains(&(Section::Diffs, "Wrap long lines")),
        "{found:?}"
    );
    let found = nav::search("dark");
    assert!(
        found.iter().any(|(s, _)| *s == Section::Appearance),
        "a keyword: {found:?}"
    );
    let found = nav::search("github login");
    assert_eq!(found, vec![(Section::SourceControl, "GitHub login")]);
    assert!(nav::search("zzzz").is_empty());
}

#[test]
fn every_section_is_in_the_nav_once() {
    let mut listed: Vec<Section> = page::GROUPS
        .iter()
        .flat_map(|(_, s)| s.iter().copied())
        .collect();
    assert_eq!(listed.len(), Section::ALL.len());
    listed.dedup();
    assert_eq!(listed, Section::ALL.to_vec(), "nav order is ALL's order");
    let titles: Vec<_> = page::GROUPS.iter().filter_map(|(t, _)| *t).collect();
    assert_eq!(titles, ["Agents", "Code", "Connections"]);
}

#[gpui::test]
fn slash_focuses_search_but_types_in_a_field(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_component::init(cx);
        init(cx);
    });
    let (view, cx) = cx.add_window_view(|window, cx| {
        let mut v = Settings::new(None, AccountsConfig::default(), None, window, cx);
        v.section = Section::Projects; // the test field draws, so it can take keys
        window.focus(&v.focus, cx);
        v
    });
    cx.run_until_parked();
    cx.simulate_keystrokes("/");
    cx.run_until_parked();
    let (search, test) = view.read_with(cx, |v, _| (v.search_input.clone(), v.test_input.clone()));
    assert!(
        cx.update(|window, cx| search.read(cx).focus_handle(cx).is_focused(window)),
        "/ focused the search"
    );
    cx.simulate_input("wrap");
    cx.run_until_parked();
    assert_eq!(search.read_with(cx, |s, _| s.value().to_string()), "wrap");

    test.update_in(cx, |s, window, cx| s.focus(window, cx));
    cx.run_until_parked();
    cx.simulate_keystrokes("/ t m p");
    cx.run_until_parked();
    assert_eq!(
        test.read_with(cx, |s, _| s.value().to_string()),
        "/tmp",
        "the field took the slash"
    );
}
