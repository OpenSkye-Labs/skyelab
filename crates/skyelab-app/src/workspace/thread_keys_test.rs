//! Acceptance (package D): T3's thread keys, and every one listed in the palette with its key.

use super::attention::editor_command;
use super::manage_harness::rig;
use super::*;
use gpui::{TestAppContext, VisualTestContext};

const M: &str = if cfg!(target_os = "macos") {
    "cmd"
} else {
    "ctrl"
};

fn shown(r: &super::manage_harness::Rig, cx: &mut VisualTestContext) -> Option<i64> {
    r.ws.read_with(cx, |ws, cx| ws.shown_thread(cx))
}

#[gpui::test]
fn thread_keys_walk_the_sidebar_order(cx: &mut TestAppContext) {
    let (r, cx) = rig(cx);
    let a = r.thread("a", cx);
    let b = r.thread("b", cx);
    let c = r.thread("c", cx); // listed c, b, a
    cx.simulate_keystrokes(&format!("{M}-1"));
    cx.run_until_parked();
    assert_eq!(shown(&r, cx), Some(c));
    cx.simulate_keystrokes(&format!("{M}-shift-]"));
    cx.run_until_parked();
    assert_eq!(shown(&r, cx), Some(b));
    // Opening a stored thread reports it started; it must not be listed twice.
    assert_eq!(r.ws.read_with(cx, |ws, cx| ws.thread_order(cx)), [c, b, a]);
    cx.simulate_keystrokes(&format!("{M}-3"));
    cx.run_until_parked();
    assert_eq!(shown(&r, cx), Some(a));
    cx.simulate_keystrokes(&format!("{M}-shift-]"));
    cx.run_until_parked();
    assert_eq!(shown(&r, cx), Some(c), "wraps around");
    cx.simulate_keystrokes(&format!("{M}-shift-["));
    cx.run_until_parked();
    assert_eq!(shown(&r, cx), Some(a));
    // How a real macOS keyboard reports ⌘⇧] and ⌘⇧[.
    cx.simulate_keystrokes(&format!("{M}-}}"));
    cx.run_until_parked();
    assert_eq!(shown(&r, cx), Some(c));
    cx.simulate_keystrokes(&format!("{M}-{{"));
    cx.run_until_parked();
    assert_eq!(shown(&r, cx), Some(a));
    cx.simulate_keystrokes(&format!("{M}-9"));
    cx.run_until_parked();
    assert_eq!(shown(&r, cx), Some(a), "no ninth thread: nothing happens");

    let tabs = |cx: &mut VisualTestContext| r.ws.read_with(cx, |ws, _| ws.open_threads.len());
    let before = tabs(cx);
    cx.simulate_keystrokes(&format!("{M}-n"));
    cx.run_until_parked();
    assert_eq!(tabs(cx), before + 1, "new thread");
}

#[gpui::test]
fn every_thread_key_is_unique_and_in_the_palette(cx: &mut TestAppContext) {
    let (r, cx) = rig(cx);
    let entries = r.ws.update_in(cx, |ws, window, cx| {
        let focus = ws.focus.clone();
        ws.palette_entries(&focus, window, cx)
    });
    for name in [
        "New Thread",
        "Previous Thread",
        "Next Thread",
        "Open in Editor",
        "Jump to Thread 1",
        "Jump to Thread 9",
    ] {
        let entry = entries
            .iter()
            .find(|e| e.label.as_ref() == name)
            .unwrap_or_else(|| panic!("{name} listed"));
        assert!(entry.binding.is_some(), "{name} shows its key");
    }
    // No other Workspace binding uses a thread key.
    let keys: Vec<String> = cx.update(|_, cx| {
        let os = cx.platform_info().os;
        cx.key_bindings()
            .borrow()
            .bindings()
            .filter(|b| {
                b.predicate()
                    .is_some_and(|p| p.to_string() == super::CONTEXT)
            })
            .filter(|b| b.keystrokes().len() == 1)
            .map(|b| b.keystrokes()[0].inner().unparse(os))
            .collect()
    });
    for key in super::attention::THREAD_KEYS {
        let full = format!(
            "{}-{key}",
            if cfg!(target_os = "macos") {
                "cmd"
            } else {
                "ctrl"
            }
        );
        assert_eq!(
            keys.iter().filter(|k| **k == full).count(),
            1,
            "{full} bound once: {keys:?}"
        );
    }
}

#[test]
fn the_editor_runs_without_a_shell_and_with_a_terminal_env() {
    let dir = tempfile::tempdir().expect("tempdir");
    // A program every runner has on PATH.
    let bin = if cfg!(windows) { "cmd" } else { "sh" };
    let argv = [bin.to_owned(), "--flag".to_owned()];
    let cmd = editor_command(&argv, dir.path()).expect("on PATH");
    assert!(
        Path::new(cmd.get_program()).is_absolute(),
        "resolved, not left to the child"
    );
    let args: Vec<_> = cmd.get_args().collect();
    assert_eq!(
        args,
        [std::ffi::OsStr::new("--flag"), dir.path().as_os_str()]
    );
    for (k, _) in cmd.get_envs() {
        assert!(
            skyelab_core::env::terminal_allowed(k, cfg!(windows)) || k == "DO_NOT_TRACK",
            "{k:?} leaked"
        );
        assert_ne!(k, "CLAUDE_CONFIG_DIR");
    }
}
