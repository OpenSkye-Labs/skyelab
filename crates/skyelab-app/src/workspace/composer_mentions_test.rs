//! Mentions in the prompt: parsed out of the text and back, drawn as atomic chips, sent as the plain text.

use super::composer_mentions::{Kind, parse};

fn words(text: &str) -> Vec<(&str, Kind)> {
    parse(text)
        .into_iter()
        .map(|(r, k)| (&text[r], k))
        .collect()
}

#[test]
fn mentions_parse_out_of_the_text_and_slice_back_to_it() {
    let text =
        "/review @src/main.rs with model:claude/opus and $lint-fix\nthen model:codex/gpt-5.1 ";
    assert_eq!(
        words(text),
        [
            ("/review", Kind::Command),
            ("@src/main.rs", Kind::File),
            ("model:claude/opus", Kind::Model),
            ("$lint-fix", Kind::Skill),
            ("model:codex/gpt-5.1", Kind::Model),
        ]
    );
    // Ranges are byte ranges of the text itself, so the chips' text is the text: nothing to convert back.
    let mut rebuilt = String::new();
    let mut at = 0;
    for (r, _) in parse(text) {
        rebuilt.push_str(&text[at..r.start]);
        rebuilt.push_str(&text[r.clone()]);
        at = r.end;
    }
    rebuilt.push_str(&text[at..]);
    assert_eq!(rebuilt, text);
}

#[test]
fn only_finished_mentions_in_the_right_place_are_chips() {
    // Still being typed (no whitespace after): the menu owns it.
    assert!(words("look at @src/ma").is_empty());
    assert!(words("@").is_empty());
    // Not mentions.
    assert!(words("mail a@b.com now").is_empty(), "not after whitespace");
    assert!(words("costs $5 now").is_empty(), "an amount");
    assert!(
        words("run /tmp/x or /review now").is_empty(),
        "a / mid-prompt"
    );
    assert!(
        words("model:gemini/pro now").is_empty(),
        "an unknown provider"
    );
    assert!(words("model:claude/ now").is_empty());
    assert!(words("@\"quoted\" now").is_empty());
    // `/name` counts after leading whitespace, and words are split at any whitespace, unicode included.
    assert_eq!(words("  /init\u{3000}x"), [("/init", Kind::Command)]);
    assert_eq!(words("é @ü.md é"), [("@ü.md", Kind::File)]);
}

#[cfg(unix)]
mod live {
    use super::super::thread::{SendPrompt, ThreadView};
    use super::super::threads_test::{Fixture, fixture, project_thread, setup, wait_until};
    use super::super::{Backend, Workspace};
    use gpui::{Entity, EntityInputHandler as _, TestAppContext, VisualTestContext};
    use skyelab_core::AccountsConfig;
    use std::path::{Path, PathBuf};

    /// A `claude` that logs each stdin line to `stdin.txt` and answers each prompt.
    fn logging_fake(dir: &Path) -> PathBuf {
        use std::os::unix::fs::PermissionsExt as _;
        let bin = dir.join("log-claude.sh");
        std::fs::write(
            &bin,
            r#"#!/bin/sh
printf '{"type":"system","subtype":"init","session_id":"s1","model":"m"}\n'
while IFS= read -r l; do
  printf '%s\n' "$l" >> stdin.txt
  case "$l" in *'"type":"user"'*)
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
        cx: &'a mut TestAppContext,
    ) -> (Entity<ThreadView>, &'a mut VisualTestContext) {
        setup(cx);
        let config = AccountsConfig::load(&f.accounts_path).expect("load");
        let backend = Backend {
            store: f.store.clone(),
            claude_bin: logging_fake(f.project_dir.parent().expect("root")),
        };
        let folder = f.project_dir.parent().expect("parent").to_path_buf();
        let path = f.accounts_path.clone();
        let (ws, cx) = cx.add_window_view(|window, cx| {
            let ws = Workspace::new(folder, Some(path), config, None, backend, window, cx);
            window.focus(&ws.focus, cx);
            ws
        });
        cx.run_until_parked();
        let thread = project_thread(f, &ws, cx);
        thread.update_in(cx, |t, window, cx| t.focus_prompt(window, cx));
        cx.run_until_parked();
        (thread, cx)
    }

    fn state(
        thread: &Entity<ThreadView>,
        cx: &mut VisualTestContext,
    ) -> (String, usize, Vec<String>) {
        thread.read_with(cx, |t, cx| {
            let p = t.prompt.read(cx);
            let chips = p
                .tokens()
                .iter()
                .map(|s| s.token().text().to_string())
                .collect();
            (p.value().to_string(), p.cursor(), chips)
        })
    }

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
    fn a_chip_is_one_unit_for_the_caret_backspace_undo_and_copy(cx: &mut TestAppContext) {
        let f = fixture(true);
        let (thread, cx) = open(&f, cx);
        let undo = if cfg!(target_os = "macos") {
            "cmd-z"
        } else {
            "ctrl-z"
        };
        let (all, copy) = if cfg!(target_os = "macos") {
            ("cmd-a", "cmd-c")
        } else {
            ("ctrl-a", "ctrl-c")
        };

        cx.simulate_input("see @lib.rs ");
        assert_eq!(
            state(&thread, cx).2,
            ["@lib.rs"],
            "a typed mention chips once a space follows"
        );
        // The space after it goes, the chip stays, and no menu opens on it.
        cx.simulate_keystrokes("backspace");
        assert_eq!(
            state(&thread, cx),
            ("see @lib.rs".into(), 11, vec!["@lib.rs".into()])
        );
        thread.read_with(cx, |t, _| {
            assert!(t.composer.menu.is_none(), "no @ menu on a chip")
        });
        // One Backspace takes the whole chip; undo brings it back.
        cx.simulate_keystrokes("backspace");
        assert_eq!(state(&thread, cx), ("see ".into(), 4, vec![]));
        cx.simulate_keystrokes(undo);
        assert_eq!(state(&thread, cx).0, "see @lib.rs");

        // The caret steps over a chip in one move.
        thread.update_in(cx, |t, window, cx| t.set_prompt("x @a.rs y", window, cx));
        assert_eq!(state(&thread, cx).2, ["@a.rs"]);
        cx.simulate_keystrokes("right right");
        assert_eq!(state(&thread, cx).1, 2, "before the chip");
        cx.simulate_keystrokes("right");
        assert_eq!(state(&thread, cx).1, 7, "over the chip in one step");
        cx.simulate_keystrokes("left");
        assert_eq!(state(&thread, cx).1, 2, "back over it in one step");

        // Copying gives the plain text.
        cx.simulate_keystrokes(&format!("{all} {copy}"));
        let copied = cx.read_from_clipboard().and_then(|c| c.text());
        assert_eq!(copied.as_deref(), Some("x @a.rs y"));
    }

    #[gpui::test]
    fn an_ime_composition_next_to_a_chip_commits_and_keeps_it(cx: &mut TestAppContext) {
        let f = fixture(true);
        let (thread, cx) = open(&f, cx);
        cx.simulate_input("@a.rs ");
        // Compose "かな" after the chip: marked text, then the commit, as the IME sends them.
        let prompt = thread.read_with(cx, |t, _| t.prompt.clone());
        prompt.update_in(cx, |p, window, cx| {
            p.replace_and_mark_text_in_range(None, "か", None, window, cx);
            p.replace_and_mark_text_in_range(None, "かな", None, window, cx);
        });
        cx.run_until_parked();
        assert_eq!(
            state(&thread, cx).2,
            ["@a.rs"],
            "the chip holds through the composition"
        );
        prompt.update_in(cx, |p, window, cx| {
            p.replace_text_in_range(None, "仮名", window, cx)
        });
        cx.run_until_parked();
        let (text, cursor, chips) = state(&thread, cx);
        assert_eq!(text, "@a.rs 仮名");
        assert_eq!(cursor, text.len());
        assert_eq!(chips, ["@a.rs"]);
    }

    #[gpui::test]
    fn a_picked_model_chips_with_its_name_and_the_agent_gets_the_plain_text(
        cx: &mut TestAppContext,
    ) {
        let f = fixture(true);
        let (thread, cx) = open(&f, cx);
        cx.simulate_input("ask @opus");
        cx.run_until_parked();
        cx.simulate_keystrokes("enter"); // the menu's accept
        cx.run_until_parked();
        let label = thread.read_with(cx, |t, cx| {
            let p = t.prompt.read(cx);
            assert_eq!(p.value().as_ref(), "ask model:claude/opus ");
            p.tokens().first().map(|s| s.token().label().to_string())
        });
        assert_eq!(label.as_deref(), Some("Opus"));
        cx.simulate_input("about @lib.rs please");
        thread.update_in(cx, |t, window, cx| t.send_prompt(&SendPrompt, window, cx));
        wait_until(cx, "the prompt to reach the agent", |_| {
            !sent(&f).is_empty()
        });
        assert_eq!(sent(&f), ["ask model:claude/opus about @lib.rs please"]);
    }
}
