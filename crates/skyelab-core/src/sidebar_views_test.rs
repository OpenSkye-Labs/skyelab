use super::*;

#[test]
fn sidebar_views_round_trip_and_an_update_keeps_the_rest_of_the_file() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("settings.toml");
    std::fs::write(&path, "editor = \"zed\"\n").expect("seed");
    let view = SidebarView {
        name: "Work".into(),
        projects: vec![PathBuf::from("/w/a"), PathBuf::from("/w/b")],
        search: "login".into(),
    };
    AppSettings::update(&path, |s| s.sidebar_views.push(view.clone())).expect("save");
    let text = std::fs::read_to_string(&path).expect("read");
    assert!(text.contains("[[sidebar_views]]"), "{text}");
    let loaded = AppSettings::load(&path).expect("load");
    assert_eq!(loaded.sidebar_views, vec![view]);
    assert_eq!(loaded.editor.as_deref(), Some("zed"));
}
