use super::*;
use gpui::{
    Context, FocusHandle, IntoElement, Render, TestAppContext, VisualTestContext, Window, actions,
    div, prelude::*,
};

actions!(keymap_test, [Alpha, Beta]);

/// Logs which action ran; a "Terminal" child with its own focus checks context scoping.
struct Probe {
    outer: FocusHandle,
    inner: FocusHandle,
    ran: Vec<&'static str>,
}

impl Render for Probe {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .key_context("Workspace")
            .track_focus(&self.outer)
            .on_action(cx.listener(|p, _: &Alpha, _, _| p.ran.push("alpha")))
            .on_action(cx.listener(|p, _: &Beta, _, _| p.ran.push("beta")))
            .child(div().key_context("Terminal").track_focus(&self.inner))
    }
}

fn setup(
    path: Option<PathBuf>,
    cx: &mut TestAppContext,
) -> (Entity<Probe>, &mut VisualTestContext) {
    cx.update(|cx| {
        cx.bind_keys([
            KeyBinding::new("x", Alpha, Some("Workspace")),
            KeyBinding::new("y", Beta, Some("Workspace")),
        ]);
        init(path, cx);
    });
    let (probe, cx) = cx.add_window_view(|_, cx| Probe {
        outer: cx.focus_handle(),
        inner: cx.focus_handle(),
        ran: Vec::new(),
    });
    probe.update_in(cx, |p, window, cx| p.outer.focus(window, cx));
    (probe, cx)
}

use gpui::Entity;

fn ran(probe: &Entity<Probe>, cx: &mut VisualTestContext) -> Vec<&'static str> {
    probe.update(cx, |p, _| std::mem::take(&mut p.ran))
}

fn problems(cx: &mut VisualTestContext) -> Vec<String> {
    cx.update(|_, cx| {
        let k = cx.global::<Keymap>();
        k.problems.iter().map(|p| p.to_string()).collect()
    })
}

fn file(text: &str) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().expect("temp dir");
    let path = dir.path().join("keybindings.toml");
    std::fs::write(&path, text).expect("write keybindings");
    (dir, path)
}

#[gpui::test]
fn a_rule_overrides_the_default_on_its_key(cx: &mut TestAppContext) {
    let (_dir, path) = file("[[binding]]\nkey = \"x\"\naction = \"keymap_test::Beta\"\n");
    let (probe, cx) = setup(Some(path), cx);
    cx.simulate_keystrokes("x");
    assert_eq!(
        ran(&probe, cx),
        ["beta"],
        "the file's rule wins over the default"
    );
    cx.simulate_keystrokes("y");
    assert_eq!(ran(&probe, cx), ["beta"], "untouched defaults still work");
    assert!(problems(cx).is_empty());
}

#[gpui::test]
fn none_frees_a_default_key(cx: &mut TestAppContext) {
    let (_dir, path) = file("[[binding]]\nkey = \"x\"\naction = \"none\"\n");
    let (probe, cx) = setup(Some(path), cx);
    cx.simulate_keystrokes("x");
    assert!(ran(&probe, cx).is_empty());
}

#[gpui::test]
fn a_rule_applies_only_in_its_context(cx: &mut TestAppContext) {
    let (_dir, path) =
        file("[[binding]]\nkey = \"z\"\naction = \"keymap_test::Alpha\"\ncontext = \"Terminal\"\n");
    let (probe, cx) = setup(Some(path), cx);
    cx.simulate_keystrokes("z");
    assert!(
        ran(&probe, cx).is_empty(),
        "outside a Terminal, z is unbound"
    );
    probe.update_in(cx, |p, window, cx| p.inner.focus(window, cx));
    cx.simulate_keystrokes("z");
    assert_eq!(ran(&probe, cx), ["alpha"]);
}

#[gpui::test]
fn bad_rules_are_reported_with_their_line_and_the_rest_load(cx: &mut TestAppContext) {
    let text = "[[binding]]\nkey = \"z\"\naction = \"Nope\"\n\n\
                [[binding]]\nkey = \"ctrl-foo-k\"\naction = \"keymap_test::Alpha\"\n\n\
                [[binding]]\nkey = \"cmd+k\"\naction = \"keymap_test::Alpha\"\n\n\
                [[binding]]\nkey = \"z\"\naction = \"keymap_test::Alpha\"\ncontext = \"(\"\n\n\
                [[binding]]\nkey = \"x\"\naction = \"keymap_test::Beta\"\n";
    let (_dir, path) = file(text);
    let (probe, cx) = setup(Some(path), cx);
    let found = problems(cx);
    assert_eq!(found.len(), 4, "{found:?}");
    assert_eq!(found[0], "line 1: unknown action `skyelab::Nope`");
    assert_eq!(found[1], "line 5: bad key `ctrl-foo-k`");
    assert!(found[2].starts_with("line 9: bad key `cmd+k`"), "{found:?}");
    assert!(
        found[3].starts_with("line 13: bad context `(`"),
        "{found:?}"
    );
    cx.simulate_keystrokes("x");
    assert_eq!(ran(&probe, cx), ["beta"], "the good rule still loaded");
}

#[gpui::test]
fn the_file_is_reloaded_when_it_changes(cx: &mut TestAppContext) {
    let (_dir, path) = file("");
    let (probe, cx) = setup(Some(path.clone()), cx);
    let before = cx.update(|_, cx| cx.global::<Keymap>().generation);
    cx.simulate_keystrokes("x");
    assert_eq!(ran(&probe, cx), ["alpha"]);

    std::fs::write(
        &path,
        "[[binding]]\nkey = \"x\"\naction = \"keymap_test::Beta\"\n",
    )
    .expect("rewrite");
    cx.executor().advance_clock(POLL * 2);
    cx.run_until_parked();
    cx.simulate_keystrokes("x");
    assert_eq!(
        ran(&probe, cx),
        ["beta"],
        "the new rule applies without a restart"
    );

    std::fs::write(&path, "[[binding]]\nkey = \"x\"\naction = \"Gone\"\n").expect("break it");
    cx.executor().advance_clock(POLL * 2);
    cx.run_until_parked();
    assert_eq!(problems(cx), ["line 1: unknown action `skyelab::Gone`"]);
    let after = cx.update(|_, cx| cx.global::<Keymap>().generation);
    assert_eq!(
        after,
        before + 2,
        "one load per change, none while it sat still"
    );
    cx.simulate_keystrokes("x");
    assert_eq!(
        ran(&probe, cx),
        ["alpha"],
        "a broken rule falls back to the default"
    );
}
