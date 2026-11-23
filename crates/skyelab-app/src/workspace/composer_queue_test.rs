//! Acceptance: a send while the agent works waits in the queue until the turn ends; a steer goes in now.

use super::composer_queue::{Queue, SendMode, mode};

#[test]
fn queue_drains_in_order_and_steer_takes_one_out() {
    assert_eq!(mode(false, false, "hi"), SendMode::Now);
    assert_eq!(mode(false, true, "hi"), SendMode::Now);
    assert_eq!(mode(true, false, "hi"), SendMode::Queue);
    assert_eq!(mode(true, true, "hi"), SendMode::Steer);
    // A command steered into a turn would replace the task, so it waits.
    assert_eq!(mode(true, true, " /compact"), SendMode::Queue);

    let mut q = Queue::default();
    let a = q.push("a".into(), Vec::new());
    let b = q.push("b".into(), Vec::new());
    let c = q.push("c".into(), Vec::new());
    assert_eq!(q.take(b).map(|m| m.text).as_deref(), Some("b"));
    assert!(q.take(b).is_none(), "taken once");
    let first = q.pop().expect("a");
    assert_eq!(first.id, a);
    q.put_back(first);
    let order: Vec<_> = std::iter::from_fn(|| q.pop()).map(|m| m.id).collect();
    assert_eq!(order, [a, c]);
    assert!(q.is_empty());
}

#[cfg(unix)]
mod live {
    use super::super::thread::ThreadView;
    use super::super::threads_test::{Fixture, fixture, project_thread, setup, wait_until};
    use super::super::{Backend, Workspace};
    use gpui::{Entity, TestAppContext, VisualTestContext};
    use skyelab_core::AccountsConfig;
    use std::path::{Path, PathBuf};

    /// A `claude` that logs each stdin line to `stdin.txt` and answers each prompt with a result, two seconds
    /// late for a prompt saying "hold".
    fn slow_fake(dir: &Path) -> PathBuf {
        use std::os::unix::fs::PermissionsExt as _;
        let bin = dir.join("slow-claude.sh");
        std::fs::write(
            &bin,
            r#"#!/bin/sh
printf '{"type":"system","subtype":"init","session_id":"s1","model":"m"}\n'
while IFS= read -r l; do
  printf '%s\n' "$l" >> stdin.txt
  case "$l" in *'"type":"user"'*)
    case "$l" in *hold*) sleep 2 ;; esac
    printf '{"type":"result","subtype":"success","is_error":false,"duration_ms":1,"result":"ok","session_id":"s1"}\n' ;;
  esac
done
"#,
        )
        .expect("write fake");
        std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).expect("chmod");
        bin
    }

    fn open<'a>(
        f: &Fixture,
        bin: PathBuf,
        cx: &'a mut TestAppContext,
    ) -> (Entity<Workspace>, &'a mut VisualTestContext) {
        let config = AccountsConfig::load(&f.accounts_path).expect("load");
        let backend = Backend {
            store: f.store.clone(),
            claude_bin: bin,
        };
        let folder = f.project_dir.parent().expect("parent").to_path_buf();
        let path = f.accounts_path.clone();
        let (ws, cx) = cx.add_window_view(|window, cx| {
            let ws = Workspace::new(folder, Some(path), config, None, backend, window, cx);
            window.focus(&ws.focus, cx);
            ws
        });
        cx.run_until_parked();
        (ws, cx)
    }

    fn type_text(thread: &Entity<ThreadView>, text: &str, keys: &str, cx: &mut VisualTestContext) {
        thread.update_in(cx, |t, window, cx| {
            t.prompt.update(cx, |p, cx| p.set_value(text, window, cx));
            t.focus_prompt(window, cx);
        });
        cx.simulate_keystrokes(keys);
        cx.run_until_parked();
    }

    /// The prompts the fake got, in order.
    fn sent(f: &Fixture) -> Vec<String> {
        std::fs::read_to_string(f.project_dir.join("stdin.txt"))
            .unwrap_or_default()
            .lines()
            .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
            .filter(|v| v["type"] == "user")
            .filter_map(|v| v["message"]["content"].as_str().map(str::to_owned))
            .collect()
    }

    #[gpui::test]
    fn a_send_during_a_turn_waits_and_a_steer_goes_first(cx: &mut TestAppContext) {
        setup(cx);
        let f = fixture(true);
        let bin = slow_fake(f.project_dir.parent().expect("root"));
        let (ws, cx) = open(&f, bin, cx);
        let thread = project_thread(&f, &ws, cx);
        let (send, steer) = if cfg!(target_os = "macos") {
            ("cmd-enter", "cmd-shift-enter")
        } else {
            ("ctrl-enter", "ctrl-shift-enter")
        };
        type_text(&thread, "hold one", send, cx);
        wait_until(cx, "the first turn to run", |cx| {
            thread.read_with(cx, |t, _| t.turn_running()) && sent(&f).len() == 1
        });
        type_text(&thread, "two", send, cx);
        thread.read_with(cx, |t, cx| {
            let queued: Vec<_> = t.composer.queue.iter().map(|q| q.text.clone()).collect();
            assert_eq!(queued, ["two"], "held while the turn runs");
            assert!(
                t.prompt.read(cx).value().is_empty(),
                "the composer is free again"
            );
        });
        assert_eq!(sent(&f), ["hold one"], "nothing sent yet");
        type_text(&thread, "three", steer, cx);
        thread.read_with(cx, |t, _| {
            assert_eq!(
                t.composer.queue.iter().count(),
                1,
                "the steer skipped the queue"
            )
        });
        // The fake reads stdin only after the held turn, so the order it logs is the order sent: the steer ahead of
        // the message queued before it, which went only once no turn was left.
        wait_until(cx, "the queue to drain after the turns", |_| {
            sent(&f).len() >= 3
        });
        assert_eq!(sent(&f), ["hold one", "three", "two"]);
        thread.read_with(cx, |t, _| assert!(t.composer.queue.is_empty()));
    }
}
