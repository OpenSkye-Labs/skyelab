//! First-run setup on the Welcome page: shown on a fresh install, nothing written before a click, a project bound
//! only to an account the user picked, and the palette action that reopens it.

use super::onboarding::Onboarding;
use super::threads_test::{fake_bin, setup, test_backend, wait_until};
use super::*;
use gpui::{TestAppContext, VisualTestContext};
use gpui_component::Root;
use std::path::Path;

struct Rig {
    _root: tempfile::TempDir,
    home: PathBuf,
    accounts: PathBuf,
    ws: Entity<Workspace>,
    setup: Entity<Onboarding>,
}

fn mkdirs(home: &Path, dirs: &[&str]) {
    for d in dirs {
        std::fs::create_dir_all(home.join(d)).expect("mkdir");
    }
}

/// A fresh install: no accounts.toml, no projects, and a home with `.claude`, `.claude-work` and `.codex` logins.
fn rig(cx: &mut TestAppContext) -> (Rig, &mut VisualTestContext) {
    rig_with(cx, None)
}

/// `load_error`: accounts.toml failed to load when the app started.
fn rig_with(cx: &mut TestAppContext, load_error: Option<String>) -> (Rig, &mut VisualTestContext) {
    setup(cx);
    let root = tempfile::tempdir().expect("tempdir");
    let home = std::fs::canonicalize(root.path())
        .expect("canonicalize")
        .join("home");
    mkdirs(
        &home,
        &[
            ".claude/projects",
            ".claude-work/projects",
            ".codex/sessions",
            "code/app/.git",
        ],
    );
    let accounts = root.path().join("config/accounts.toml");
    let codex =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../skyelab-agent/tests/fixtures/fake-codex.sh");
    cx.update(|cx| cx.set_global(CodexBin(codex)));
    let (mut ws, path, h) = (None, accounts.clone(), home.clone());
    let window = cx.add_window(|window, cx| {
        let backend = Backend {
            claude_bin: fake_bin(),
            ..test_backend()
        };
        let view = cx.new(|cx| {
            Workspace::new(
                h.clone(),
                Some(path),
                AccountsConfig::default(),
                load_error,
                backend,
                window,
                cx,
            )
        });
        let setup = view.read(cx).welcome.read(cx).setup.clone();
        setup.update(cx, |o, _| o.home = Some(h));
        ws = Some(view.clone());
        Root::new(view, window, cx)
    });
    let cx = VisualTestContext::from_window(window.into(), cx).into_mut();
    let ws = ws.expect("workspace built");
    let setup = ws.read_with(cx, |ws, cx| ws.welcome.read(cx).setup.clone());
    wait_until(cx, "detection", |cx| {
        setup.read_with(cx, |o, _| {
            o.found.len() == 3 && o.agents.iter().all(|a| a.version.is_some())
        })
    });
    let rig = Rig {
        _root: root,
        home,
        accounts,
        ws,
        setup,
    };
    (rig, cx)
}

impl Rig {
    fn shown(&self, cx: &mut VisualTestContext) -> bool {
        self.ws
            .read_with(cx, |ws, cx| ws.welcome.read(cx).show_setup)
    }

    fn saved(&self) -> AccountsConfig {
        AccountsConfig::load(&self.accounts).expect("load")
    }

    fn dir(&self, id: &str, cx: &mut VisualTestContext) -> PathBuf {
        self.setup.read_with(cx, |o, _| {
            let f = o.found.iter().find(|f| f.found.id == id);
            f.expect("found dir offered").found.config_dir.clone()
        })
    }

    fn rename(&self, id: &str, label: &str, cx: &mut VisualTestContext) {
        let (setup, dir) = (self.setup.clone(), self.dir(id, cx));
        cx.update(|window, cx| {
            setup.update(cx, |o, cx| {
                o.rename(&dir, window, cx);
                let row = o.found.iter().find(|f| f.found.config_dir == dir);
                let input = row.and_then(|r| r.label.clone()).expect("name field");
                input.update(cx, |s, cx| s.set_value(label, window, cx));
            })
        });
    }

    fn add(&self, id: &str, cx: &mut VisualTestContext) {
        let dir = self.dir(id, cx);
        self.setup.update(cx, |o, cx| o.add_account(&dir, cx));
        cx.run_until_parked();
    }
}

#[gpui::test]
fn onboarding_shows_on_a_fresh_install_with_agents_and_found_dirs(cx: &mut TestAppContext) {
    let (rig, cx) = rig(cx);
    assert!(rig.shown(cx));
    rig.setup.read_with(cx, |o, _| {
        let versions: Vec<_> = o
            .agents
            .iter()
            .map(|a| a.version.clone().and_then(Result::ok))
            .collect();
        assert_eq!(
            versions,
            [
                Some("2.1.0 (Claude Code)".into()),
                Some("codex-cli 0.99.0".into())
            ]
        );
        let ids: Vec<_> = o.found.iter().map(|f| f.found.id.as_str()).collect();
        assert_eq!(ids, ["claude", "claude-work", "codex"]);
        assert_eq!(o.repos, [rig.home.join("code/app")]);
    });
}

#[gpui::test]
fn onboarding_writes_nothing_before_confirm(cx: &mut TestAppContext) {
    let (rig, cx) = rig(cx);
    rig.rename("claude-work", "Work", cx);
    rig.rename("codex", "Mine", cx);
    assert!(!rig.accounts.exists(), "renaming writes nothing");
    rig.add("claude-work", cx);
    let saved = rig.saved();
    assert_eq!(
        saved.accounts.len(),
        1,
        "only the dir whose Add was clicked"
    );
    let a = &saved.accounts[0];
    assert_eq!((a.id.as_str(), a.label.as_str()), ("claude-work", "Work"));
    assert_eq!(a.config_dir, rig.home.join(".claude-work"));
    assert!(saved.bindings.is_empty());
    rig.setup.read_with(cx, |o, _| {
        assert_eq!(o.found.len(), 2, "the added dir is no longer offered")
    });
}

#[gpui::test]
fn onboarding_bind_needs_a_picked_account(cx: &mut TestAppContext) {
    let (rig, cx) = rig(cx);
    rig.add("claude", cx);
    rig.add("claude-work", cx);
    let repo = rig.home.join("code/app");
    rig.setup.update(cx, |o, cx| {
        o.set_folder(repo.clone(), cx);
        assert_eq!(o.account, None, "two accounts: none is picked for the user");
        o.add_project(cx);
    });
    cx.run_until_parked();
    assert!(rig.saved().bindings.is_empty(), "no account, no binding");
    assert!(rig.ws.read_with(cx, |ws, _| ws.projects.is_empty()));
    rig.setup.update(cx, |o, cx| {
        o.account = Some("claude-work".into());
        o.add_project(cx);
    });
    wait_until(cx, "project stored", |cx| {
        rig.ws.read_with(cx, |ws, _| ws.projects.len() == 1)
    });
    assert_eq!(
        rig.saved().resolve(&repo).map(|a| a.id.clone()),
        Some("claude-work".into())
    );
    assert!(rig.shown(cx), "stays open until Done");
    rig.setup.update(cx, |o, cx| {
        assert!(o.complete() && o.folder.is_none());
        cx.emit(super::onboarding::OnboardingEvent::Close);
    });
    cx.run_until_parked();
    assert!(!rig.shown(cx), "Done returns Welcome to normal");
}

#[gpui::test]
fn onboarding_preselects_a_single_account_but_waits_for_the_click(cx: &mut TestAppContext) {
    let (rig, cx) = rig(cx);
    rig.add("codex", cx);
    rig.setup.update(cx, |o, cx| {
        o.set_folder(rig.home.join("code/app"), cx);
        assert_eq!(o.account.as_deref(), Some("codex"));
    });
    cx.run_until_parked();
    assert!(rig.saved().bindings.is_empty(), "preselected is not bound");
}

#[gpui::test]
fn onboarding_reopens_from_the_palette_action(cx: &mut TestAppContext) {
    let (rig, cx) = rig(cx);
    rig.setup.update(cx, |_, cx| {
        cx.emit(super::onboarding::OnboardingEvent::Close)
    });
    cx.run_until_parked();
    assert!(!rig.shown(cx), "skipped");
    let focus = rig.ws.read_with(cx, |ws, _| ws.focus.clone());
    cx.update(|window, cx| {
        window.focus(&focus, cx);
        window.dispatch_action(Box::new(SetUpAccounts), cx)
    });
    cx.run_until_parked();
    assert!(rig.shown(cx));
    assert!(cx.update(|_, cx| cx.all_action_names().contains(&"skyelab::SetUpAccounts")));
    assert_eq!(
        crate::palette::humanize("skyelab::SetUpAccounts"),
        "Set Up Accounts"
    );
}

#[gpui::test]
fn onboarding_writes_once_a_broken_file_is_fixed(cx: &mut TestAppContext) {
    let (rig, cx) = rig_with(cx, Some("broken at startup".into()));
    std::fs::create_dir_all(rig.accounts.parent().expect("parent")).expect("mkdir");
    std::fs::write(&rig.accounts, "not = [toml").expect("write");
    rig.add("claude", cx);
    assert_eq!(
        std::fs::read_to_string(&rig.accounts).expect("read"),
        "not = [toml",
        "a broken file is kept"
    );
    rig.setup.read_with(cx, |o, _| assert!(o.error.is_some()));
    std::fs::write(&rig.accounts, "").expect("fixed by hand");
    rig.add("claude", cx);
    assert_eq!(
        rig.saved().accounts.len(),
        1,
        "the fixed file takes the add"
    );
}

/// The update check's agent fetch, answering `claude` for Claude Code and `codex` for Codex, counting its calls.
fn latest_versions(
    cx: &mut TestAppContext,
    claude: &'static str,
    codex: &'static str,
    check_on: bool,
) -> std::sync::Arc<std::sync::atomic::AtomicUsize> {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let calls = std::sync::Arc::new(AtomicUsize::new(0));
    let seen = calls.clone();
    let release: crate::updates::Fetch =
        std::sync::Arc::new(|| Err(std::io::Error::other("unused")));
    cx.update(|cx| {
        crate::updates::init(None, release, cx);
        crate::updates::set_agent_fetch(
            std::sync::Arc::new(move |p| {
                seen.fetch_add(1, Ordering::SeqCst);
                Ok(match p {
                    skyelab_core::Provider::Claude => claude,
                    skyelab_core::Provider::Codex => codex,
                }
                .to_owned())
            }),
            cx,
        );
        cx.global_mut::<crate::updates::Updates>().enabled = check_on;
    });
    calls
}

#[gpui::test]
fn welcome_names_a_newer_agent_cli_release(cx: &mut TestAppContext) {
    let calls = latest_versions(cx, "2.2.0", "0.99.0", true);
    let (rig, cx) = rig(cx);
    wait_until(cx, "the latest versions", |cx| {
        rig.setup
            .read_with(cx, |o, _| o.agents.iter().any(|a| a.update.is_some()))
    });
    let updates: Vec<_> = rig.setup.read_with(cx, |o, _| {
        o.agents.iter().map(|a| a.update.clone()).collect()
    });
    // Claude Code 2.1.0 is behind 2.2.0; Codex 0.99.0 is current.
    assert_eq!(updates, [Some("2.2.0".to_owned()), None]);
    assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 2);
}

#[gpui::test]
fn welcome_asks_nothing_with_the_update_check_off(cx: &mut TestAppContext) {
    let calls = latest_versions(cx, "9.0.0", "9.0.0", false);
    let (rig, cx) = rig(cx);
    cx.run_until_parked();
    rig.setup.read_with(cx, |o, _| {
        assert!(o.agents.iter().all(|a| a.update.is_none()))
    });
    assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 0);
}
