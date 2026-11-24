//! Acceptance: mod+s stashes the composer's text, files and chips into the store and empties the composer; restoring
//! takes the entry out and appends it after what is typed; delete removes one; past 20 the oldest goes.

use super::composer_context::ChipKind;
use super::composer_stash::PromptStash;
use super::thread::ThreadView;
use super::threads_test::{fixture, open_workspace, project_thread, setup, wait_until};
use gpui::{Entity, TestAppContext, VisualTestContext};
use skyelab_agent::Attachment;

const STASH_KEY: &str = if cfg!(target_os = "macos") {
    "cmd-s"
} else {
    "ctrl-s"
};

fn stored(f: &super::threads_test::Fixture) -> Vec<skyelab_store::DraftRow> {
    let s = f.store.lock().expect("store");
    s.drafts_with_prefix("stash:").expect("list")
}

fn fill(thread: &Entity<ThreadView>, text: &str, cx: &mut VisualTestContext) {
    thread.update_in(cx, |t, window, cx| {
        t.prompt.update(cx, |p, cx| p.set_value(text, window, cx));
        t.composer.attachments =
            vec![Attachment::new("notes.txt", b"note".to_vec()).expect("file")];
        t.add_terminal_context("Terminal 1".into(), "$ make", cx);
        t.focus_prompt(window, cx);
    });
}

fn composer(thread: &Entity<ThreadView>, cx: &mut VisualTestContext) -> (String, usize, usize) {
    thread.read_with(cx, |t, cx| {
        (
            t.prompt.read(cx).value().to_string(),
            t.composer.attachments.len(),
            t.composer.contexts.len(),
        )
    })
}

fn shown_rows(cx: &mut VisualTestContext) -> usize {
    cx.update(|_, cx| PromptStash::rows(cx).len())
}

#[gpui::test]
fn stash_restore_delete_and_cap(cx: &mut TestAppContext) {
    setup(cx);
    let f = fixture(true);
    let (ws, cx) = open_workspace(&f, cx);
    let thread = project_thread(&f, &ws, cx);

    // 1. Stash: everything leaves the composer and lands in the store.
    fill(&thread, "fix the build", cx);
    cx.simulate_keystrokes(STASH_KEY);
    wait_until(cx, "the stash stored", |_| stored(&f).len() == 1);
    assert_eq!(
        composer(&thread, cx),
        (String::new(), 0, 0),
        "STASH_DIDNT_CLEAR"
    );
    let row = &stored(&f)[0];
    assert_eq!(row.files, 1);
    assert!(
        row.text.contains("fix the build") && row.text.contains("$ make"),
        "{}",
        row.text
    );
    wait_until(cx, "the badge's rows", |cx| shown_rows(cx) == 1);

    // 2. Restore appends after what is typed and takes the entry out.
    thread.update_in(cx, |t, window, cx| {
        t.prompt.update(cx, |p, cx| p.set_value("now", window, cx))
    });
    let owner = row.owner.clone();
    thread.update_in(cx, |t, window, cx| t.restore_stash(owner, window, cx));
    wait_until(cx, "restored", |cx| composer(&thread, cx).1 == 1);
    assert_eq!(
        composer(&thread, cx),
        ("now\n\nfix the build".into(), 1, 1),
        "RESTORE_REPLACED"
    );
    let chip = thread.read_with(cx, |t, _| t.composer.contexts[0].clone());
    assert_eq!(
        (chip.kind, chip.text.as_str()),
        (ChipKind::Terminal, "$ make")
    );
    assert!(stored(&f).is_empty(), "RESTORE_LEFT_ROW");
    wait_until(cx, "the badge empties", |cx| shown_rows(cx) == 0);

    // 3. Delete, and the cap of 20.
    for n in 0..21 {
        thread.update_in(cx, |t, window, cx| {
            t.prompt
                .update(cx, |p, cx| p.set_value(format!("p{n}"), window, cx))
        });
        cx.simulate_keystrokes(STASH_KEY);
        wait_until(cx, "stashed", |_| {
            stored(&f)
                .iter()
                .any(|r| r.text.contains(&format!("\"p{n}\"")))
        });
    }
    let rows = stored(&f);
    assert_eq!(rows.len(), 20, "NO_CAP");
    assert!(
        rows.iter().all(|r| !r.text.contains("\"p0\"")),
        "the oldest went"
    );
    let newest = rows[0].owner.clone();
    assert!(rows[0].text.contains("\"p20\""), "newest first");
    thread.update(cx, |t, cx| t.delete_stash(newest.clone(), cx));
    wait_until(cx, "deleted", |_| stored(&f).len() == 19);
    assert!(stored(&f).iter().all(|r| r.owner != newest));
}

#[gpui::test]
fn restore_past_the_chip_cap_is_refused_and_keeps_the_entry(cx: &mut TestAppContext) {
    setup(cx);
    let f = fixture(true);
    let (ws, cx) = open_workspace(&f, cx);
    let thread = project_thread(&f, &ws, cx);
    fill(&thread, "later", cx);
    cx.simulate_keystrokes(STASH_KEY);
    wait_until(cx, "stashed", |cx| {
        stored(&f).len() == 1 && shown_rows(cx) == 1
    });
    thread.update(cx, |t, cx| {
        for n in 0..8 {
            t.add_terminal_context(format!("Terminal {n}").into(), "out", cx);
        }
    });
    let owner = stored(&f)[0].owner.clone();
    thread.update_in(cx, |t, window, cx| t.restore_stash(owner, window, cx));
    cx.run_until_parked();
    assert_eq!(stored(&f).len(), 1, "CAP_LOST_ENTRY");
    let (chips, notice) = thread.read_with(cx, |t, _| {
        (t.composer.contexts.len(), t.composer.notice.clone())
    });
    assert_eq!(chips, 8);
    assert!(notice.is_some_and(|n| n.contains("up to 8 chips")));
}
