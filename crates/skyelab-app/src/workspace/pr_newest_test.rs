//! A PR read that started before one already taken is older news: it changes neither the UI's status nor the stored
//! state. The store follows the same newest-wins rule as the UI (one decision, made where the read lands). The test
//! executor runs background work one task at a time, so the newer landing is recorded rather than raced.

use super::manage_harness::rig;
use super::pr_watch::{PrLanded, PrStatuses, PrView};
use super::threads_test::wait_until;
use super::*;
use gpui::VisualTestContext;
use skyelab_git::PrState;
use std::time::Duration;

const URL: &str = "https://github.com/acme/demo/pull/42";

fn state(id: i64, cx: &mut VisualTestContext) -> Option<PrState> {
    cx.update(|_, cx| match PrStatuses::view(cx, id, URL) {
        Some(PrView::Status(s)) => Some(s.state),
        _ => None,
    })
}

#[gpui::test]
fn an_older_read_landing_late_changes_nothing(cx: &mut gpui::TestAppContext) {
    let (r, cx) = rig(cx);
    let gh = r.f.project_dir.parent().expect("root").join("gh-acme");
    std::fs::create_dir_all(&gh).expect("mkdir");
    std::fs::write(gh.join("fake-login"), "octo").expect("login");
    std::fs::write(gh.join("hosts.yml"), "github.com:\n    user: octo\n").expect("hosts");
    let mut config = AccountsConfig::load(&r.f.accounts_path).expect("load");
    config.accounts[0].gh_config_dir = Some(gh.clone());
    config.save(&r.f.accounts_path).expect("save");
    let fake =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../skyelab-git/tests/fixtures/fake-gh.sh");
    r.ws.update(cx, |ws, cx| {
        cx.set_global(GhBin(fake));
        cx.set_global(AccountsFile(r.f.accounts_path.clone()));
        ws.config = config;
    });
    let pr = |state: &str, draft: bool| {
        std::fs::write(
            gh.join("fake-pr.json"),
            format!(r#"{{"state":"{state}","isDraft":{draft},"statusCheckRollup":[]}}"#),
        )
        .expect("pr");
    };
    let stored = || {
        r.f.store
            .lock()
            .expect("lock")
            .list_all_threads()
            .expect("threads")
            .into_iter()
            .find(|t| t.pr_url.as_deref() == Some(URL))
            .and_then(|t| t.pr_state)
    };

    pr("OPEN", true);
    let id = r.thread("Has a PR", cx);
    r.f.store
        .lock()
        .expect("lock")
        .set_thread_pr_url(id, Some(URL))
        .expect("link");
    r.reload(cx);
    wait_until(cx, "first read: draft", |cx| {
        state(id, cx) == Some(PrState::Draft)
    });
    assert_eq!(stored().as_deref(), Some("draft"));

    // A read that started later (an action's reread, say) was taken already: this one is older news.
    let later = crate::ui::now_ms() + 60 * 60_000;
    cx.update(|_, cx| PrLanded::get(cx).mark(id, URL, later));
    pr("OPEN", false);
    let reads = || {
        std::fs::read_to_string(gh.join("fake-gh.prview"))
            .unwrap_or_default()
            .lines()
            .count()
    };
    let before = reads();
    r.ws.update(cx, |ws, cx| ws.reread_pr(id, cx));
    wait_until(cx, "the older read ran", |_| reads() > before);
    for _ in 0..20 {
        cx.run_until_parked();
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(
        state(id, cx),
        Some(PrState::Draft),
        "the UI drops the older read"
    );
    assert_eq!(stored().as_deref(), Some("draft"), "the store drops it too");

    // Not older news: a read that started after the last taken one is taken by both.
    cx.update(|_, cx| PrLanded::get(cx).mark(id, URL, 0));
    r.ws.update(cx, |ws, cx| ws.reread_pr(id, cx));
    wait_until(cx, "the next read landed", |cx| {
        state(id, cx) == Some(PrState::Open) && stored().as_deref() == Some("open")
    });
}
