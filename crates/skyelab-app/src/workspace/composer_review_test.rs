//! Regressions from the composer review: queued messages and the folder's binding, refused sends keeping the
//! draft, the attachment cap under racing reads, Codex and PDFs, links in a repo's commands folder.

use super::composer_state::{fits, unfit};
use super::thread::{SendPrompt, ThreadView};
use super::threads_test::test_backend;
use super::*;
use gpui::{TestAppContext, VisualTestContext};
use skyelab_agent::Attachment;
use skyelab_core::{Account, Binding, Provider};
use std::path::{Path, PathBuf};
use std::time::Duration;

pub(super) fn account(root: &Path, id: &str, provider: Provider) -> Account {
    Account {
        id: id.into(),
        label: id.into(),
        config_dir: root.join(format!(".home-{id}")),
        provider,
        gh_config_dir: None,
        ssh_agent: false,
    }
}

/// A project bound to "acme", with a second account "beta", and a view on a new thread in it.
pub(super) fn view_with(
    cx: &mut TestAppContext,
    bin: Option<PathBuf>,
) -> (
    tempfile::TempDir,
    PathBuf,
    AccountsConfig,
    Entity<ThreadView>,
    &mut VisualTestContext,
) {
    cx.executor().allow_parking();
    cx.update(|cx| {
        gpui_component::init(cx);
        crate::settings::init(cx);
        skyelab_terminal::init(cx);
        init(cx);
    });
    let home = tempfile::tempdir().expect("tempdir");
    let root = std::fs::canonicalize(home.path()).expect("canonicalize");
    let dir = root.join("demo");
    std::fs::create_dir_all(&dir).expect("dir");
    let mut config = AccountsConfig::default();
    for id in ["acme", "beta"] {
        config
            .add_account(account(&root, id, Provider::Claude))
            .expect("account");
    }
    config
        .add_binding(Binding::new(dir.clone(), "acme".into()))
        .expect("bind");
    let mut backend = test_backend();
    if let Some(bin) = bin {
        backend.claude_bin = bin;
    }
    let project = backend
        .store
        .lock()
        .expect("lock")
        .create_project("demo", &dir)
        .expect("project");
    let c = config.clone();
    let (view, cx) =
        cx.add_window_view(move |window, cx| ThreadView::new(&project, c, backend, window, cx));
    (home, dir, config, view, cx)
}

pub(super) fn slow_claude(dir: &Path) -> PathBuf {
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
    .expect("write");
    std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).expect("chmod");
    bin
}

pub(super) fn sent(dir: &Path) -> Vec<String> {
    std::fs::read_to_string(dir.join("stdin.txt"))
        .unwrap_or_default()
        .lines()
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .filter(|v| v["type"] == "user")
        .filter_map(|v| v["message"]["content"].as_str().map(str::to_owned))
        .collect()
}

pub(super) fn send(view: &Entity<ThreadView>, text: &'static str, cx: &mut VisualTestContext) {
    view.update_in(cx, |t, window, cx| {
        t.prompt.update(cx, |p, cx| p.set_value(text, window, cx));
        t.send_prompt(&SendPrompt, window, cx);
    });
    cx.run_until_parked();
}

pub(super) fn settle(
    cx: &mut VisualTestContext,
    mut done: impl FnMut(&mut VisualTestContext) -> bool,
) {
    for _ in 0..500 {
        cx.run_until_parked();
        if done(cx) {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

pub(super) fn rebind(config: &mut AccountsConfig, dir: &Path, to: &str) {
    config.bindings.retain(|b| b.path != dir);
    config
        .add_binding(Binding::new(dir.to_path_buf(), to.into()))
        .expect("rebind");
}

#[cfg(unix)]
#[gpui::test]
fn a_queued_message_isnt_sent_after_the_folder_is_rebound(cx: &mut TestAppContext) {
    let scratch = tempfile::tempdir().expect("tempdir");
    let bin = slow_claude(scratch.path());
    let (_home, dir, mut config, view, cx) = view_with(cx, Some(bin));
    send(&view, "hold one", cx);
    settle(cx, |cx| {
        view.read_with(cx, |t, _| t.turn_running()) && sent(&dir).len() == 1
    });
    send(&view, "two", cx);
    assert_eq!(
        view.read_with(cx, |t, _| t.composer.queue.iter().count()),
        1,
        "queued"
    );
    rebind(&mut config, &dir, "beta");
    view.update(cx, |t, cx| t.set_config(config, cx));
    settle(cx, |cx| !view.read_with(cx, |t, _| t.turn_running()));
    for _ in 0..50 {
        cx.run_until_parked();
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(
        !sent(&dir).iter().any(|s| s == "two"),
        "sent to the old account: {:?}",
        sent(&dir)
    );
    view.read_with(cx, |t, _| {
        assert_eq!(t.composer.queue.iter().count(), 1, "still waiting");
        assert!(t.session.is_none(), "the old agent was ended");
        assert!(
            t.resume_refused
                .as_deref()
                .is_some_and(|e| e.contains("now bound to")),
            "{:?}",
            t.resume_refused
        );
    });
}

#[cfg(unix)]
#[gpui::test]
fn a_refused_follow_up_keeps_its_text_and_files(cx: &mut TestAppContext) {
    let (_home, dir, mut config, view, cx) = view_with(cx, None);
    send(&view, "one", cx);
    settle(cx, |cx| {
        view.read_with(cx, |t, _| t.session.is_some() && !t.turn_running())
    });
    rebind(&mut config, &dir, "beta");
    view.update(cx, |t, cx| t.set_config(config, cx));
    view.update_in(cx, |t, window, cx| {
        t.composer
            .attachments
            .push(Attachment::new("n.txt", b"x".to_vec()).expect("text"));
        t.prompt.update(cx, |p, cx| p.set_value("two", window, cx));
        t.send_prompt(&SendPrompt, window, cx);
    });
    cx.run_until_parked();
    view.read_with(cx, |t, cx| {
        assert!(
            t.resume_refused
                .as_deref()
                .is_some_and(|e| e.contains("now bound to")),
            "{:?}",
            t.resume_refused
        );
        assert_eq!(t.prompt.read(cx).value().as_ref(), "two");
        assert_eq!(t.composer.attachments.len(), 1);
    });
}

#[gpui::test]
fn racing_reads_stay_within_the_cap(cx: &mut TestAppContext) {
    let (home, _dir, _config, view, cx) = view_with(cx, None);
    let files: Vec<PathBuf> = (0..6)
        .map(|i| {
            let p = home.path().join(format!("{i}.txt"));
            std::fs::write(&p, "x").expect("write");
            p
        })
        .collect();
    view.update(cx, |t, cx| {
        t.attach_paths(files.clone(), cx);
        t.attach_paths(files.clone(), cx);
    });
    settle(cx, |cx| {
        view.read_with(cx, |t, _| t.composer.attachments.len() >= 10)
    });
    for _ in 0..30 {
        cx.run_until_parked();
        std::thread::sleep(Duration::from_millis(10));
    }
    let n = view.read_with(cx, |t, _| t.composer.attachments.len());
    assert_eq!(n, skyelab_agent::MAX_ATTACHMENTS);
}

#[test]
fn codex_takes_no_pdf_and_defaults_stay_with_their_provider() {
    let root = Path::new("/tmp");
    let pdf = Attachment::new("a.pdf", b"%PDF-1.7".to_vec()).expect("pdf");
    assert!(
        unfit(
            &account(root, "c", Provider::Codex),
            std::slice::from_ref(&pdf)
        )
        .is_some()
    );
    assert!(unfit(&account(root, "a", Provider::Claude), &[pdf]).is_none());
    assert!(fits(Provider::Claude, "opus"));
    assert!(fits(Provider::Claude, "claude-opus-5-5"));
    assert!(!fits(Provider::Codex, "opus"));
    assert!(fits(Provider::Codex, "gpt-5.5"));
    assert!(!fits(Provider::Claude, "gpt-5.5"));
}

#[cfg(unix)]
#[test]
fn a_link_in_the_repos_commands_folder_is_skipped() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let project = tmp.path().join("proj");
    let cmds = project.join(".claude/commands");
    std::fs::create_dir_all(&cmds).expect("mkdir");
    std::fs::write(tmp.path().join("secret.txt"), "TOP SECRET").expect("write");
    std::os::unix::fs::symlink(tmp.path().join("secret.txt"), cmds.join("leak.md")).expect("link");
    std::fs::write(cmds.join("real.md"), "Real one").expect("write");
    let items = super::composer_menu::command_items(
        Provider::Claude,
        &tmp.path().join("acct"),
        &project,
        false,
    );
    assert!(items.iter().any(|i| i.label == "/real"));
    assert!(
        !items
            .iter()
            .any(|i| i.label == "/leak" || i.detail.contains("SECRET"))
    );
}

#[cfg(unix)]
#[gpui::test]
fn interrupt_from_the_composer_shows_stopping_until_the_turn_ends(cx: &mut TestAppContext) {
    let scratch = tempfile::tempdir().expect("tempdir");
    let bin = slow_claude(scratch.path());
    let (_home, dir, _config, view, cx) = view_with(cx, Some(bin));
    send(&view, "hold one", cx);
    settle(cx, |cx| {
        view.read_with(cx, |t, _| t.turn_running()) && sent(&dir).len() == 1
    });
    view.update_in(cx, |t, window, cx| {
        t.interrupt(&super::thread::Interrupt, window, cx)
    });
    view.read_with(cx, |t, _| {
        assert!(t.composer.interrupting.is_some(), "Stopping… shows")
    });
    let interrupted = |dir: &Path| {
        std::fs::read_to_string(dir.join("stdin.txt"))
            .unwrap_or_default()
            .contains(r#""subtype":"interrupt""#)
    };
    settle(cx, |cx| {
        interrupted(&dir) && !view.read_with(cx, |t, _| t.turn_running())
    });
    assert!(interrupted(&dir), "the agent got the interrupt");
    view.read_with(cx, |t, _| {
        assert!(
            t.composer.interrupting.is_none() && !t.composer.stalled,
            "cleared when the turn ended"
        );
    });
}
