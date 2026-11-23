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
                None,
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

    fn ix(&self, id: &str, cx: &mut VisualTestContext) -> usize {
        self.setup.read_with(cx, |o, _| {
            o.found
                .iter()
                .position(|f| f.found.id == id)
                .expect("found dir offered")
        })
    }

    fn rename(&self, id: &str, label: &str, cx: &mut VisualTestContext) {
        let (setup, ix) = (self.setup.clone(), self.ix(id, cx));
        cx.update(|window, cx| {
            setup.update(cx, |o, cx| {
                o.rename(ix, window, cx);
                let input = o.found[ix].label.clone().expect("name field");
                input.update(cx, |s, cx| s.set_value(label, window, cx));
            })
        });
    }

    fn add(&self, id: &str, cx: &mut VisualTestContext) {
        let ix = self.ix(id, cx);
        self.setup.update(cx, |o, cx| o.add_account(ix, cx));
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
