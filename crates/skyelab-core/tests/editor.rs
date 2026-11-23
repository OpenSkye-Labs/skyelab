//! Acceptance (package D): which editor "Open in editor" runs.

use skyelab_core::editor_argv;

fn vars<'a>(pairs: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<String> + 'a {
    move |k| {
        pairs
            .iter()
            .find(|(n, _)| *n == k)
            .map(|(_, v)| (*v).to_owned())
    }
}

#[test]
fn settings_win_then_visual_then_editor_then_code_or_zed() {
    let none = |_: &str| false;
    let env = [("VISUAL", "subl -w"), ("EDITOR", "vim")];
    assert_eq!(
        editor_argv(Some("  zed --new "), vars(&env), none),
        Some(vec!["zed".to_owned(), "--new".to_owned()]),
        "settings.toml first, split on whitespace"
    );
    assert_eq!(
        editor_argv(None, vars(&env), none),
        Some(vec!["subl".to_owned(), "-w".to_owned()])
    );
    assert_eq!(
        editor_argv(Some("   "), vars(&[("EDITOR", "vim")]), none),
        Some(vec!["vim".to_owned()]),
        "a blank setting counts as unset"
    );
    assert_eq!(
        editor_argv(None, vars(&[("VISUAL", "")]), |n| n == "zed"),
        Some(vec!["zed".to_owned()])
    );
    assert_eq!(
        editor_argv(None, vars(&[]), |n| n == "code" || n == "zed"),
        Some(vec!["code".to_owned()]),
        "code before zed"
    );
    assert_eq!(editor_argv(None, vars(&[]), none), None);
}
