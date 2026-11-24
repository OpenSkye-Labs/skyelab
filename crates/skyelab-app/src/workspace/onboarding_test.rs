//! First-run onboarding: shown on a fresh install, nothing written before a click, accounts added only from a
//! found dir's Add. Also the rig the other onboarding tests share.

use super::onboarding::Onboarding;
use super::threads_test::{fake_bin, setup, test_backend, wait_until};
use super::*;
use gpui::{TestAppContext, VisualTestContext};
use gpui_component::Root;
use std::path::Path;

pub(super) struct Rig {
    _root: tempfile::TempDir,
    pub home: PathBuf,
    pub accounts: PathBuf,
    pub ws: Entity<Workspace>,
    pub setup: Entity<Onboarding>,
}

fn mkdirs(home: &Path, dirs: &[&str]) {
    for d in dirs {
        std::fs::create_dir_all(home.join(d)).expect("mkdir");
    }
}

/// A fresh install: no accounts.toml, no projects, and a home with `.claude`, `.claude-work` and `.codex` logins.
pub(super) fn rig(cx: &mut TestAppContext) -> (Rig, &mut VisualTestContext) {
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
    let gh = Path::new(env!("CARGO_MANIFEST_DIR")).join("../skyelab-git/tests/fixtures/fake-gh.sh");
    cx.update(|cx| {
        // A fresh install: never onboarded, nothing saved.
        super::onboarding::first_run::install(None, cx);
        cx.set_global(CodexBin(codex));
        cx.set_global(github::GhBin(gh));
    });
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
            o.found.len() == 3 && o.agents.len() == 3 && o.agents.iter().all(|a| a.version.is_some())
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
    pub fn shown(&self, cx: &mut VisualTestContext) -> bool {
        self.ws
            .read_with(cx, |ws, cx| ws.welcome.read(cx).show_setup)
    }

    pub fn saved(&self) -> AccountsConfig {
        AccountsConfig::load(&self.accounts).expect("load")
    }

    pub fn dir(&self, id: &str, cx: &mut VisualTestContext) -> PathBuf {
        self.setup.read_with(cx, |o, _| {
            let f = o.found.iter().find(|f| f.found.id == id);
            f.expect("found dir offered").found.config_dir.clone()
        })
    }

    pub fn rename(&self, id: &str, label: &str, cx: &mut VisualTestContext) {
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

    pub fn add(&self, id: &str, cx: &mut VisualTestContext) {
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
                Some("codex-cli 0.99.0".into()),
                Some("gh version 2.99.0 (fake)".into())
            ]
        );
        let ids: Vec<_> = o.found.iter().map(|f| f.found.id.as_str()).collect();
        assert_eq!(ids, ["claude", "claude-work", "codex"]);
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
    // Claude Code 2.1.0 is behind 2.2.0; Codex 0.99.0 is current; gh is never checked.
    assert_eq!(updates, [Some("2.2.0".to_owned()), None, None]);
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

#[gpui::test]
fn added_accounts_say_whether_they_are_signed_in(cx: &mut TestAppContext) {
    let (rig, cx) = rig(cx);
    rig.setup.update(cx, |o, cx| o.go(super::onboarding::Step::Agents, cx));
    std::fs::write(rig.home.join(".codex/fake-login"), "").expect("sign codex in");
    rig.add("claude-work", cx);
    rig.add("codex", cx);
    let login = cx.update(|_, cx| super::login_status::LoginStatus::shared(cx));
    let saved = rig.saved();
    let status = |id: &str, cx: &mut VisualTestContext| {
        let account = saved.accounts.iter().find(|a| a.id == id).expect("added");
        login.read_with(cx, |l, _| l.get(account).cloned())
    };
    use super::login_status::Status;
    wait_until(cx, "both answers", |cx| {
        status("claude-work", cx) == Some(Status::SignedOut)
            && status("codex", cx) == Some(Status::SignedIn(Some("ChatGPT".into())))
    });
    cx.run_until_parked();
    assert!(cx.debug_bounds("setup-login-codex").is_some());
    assert!(cx.debug_bounds("setup-log-in-claude-work").is_some(), "signed out: Log in");
    assert!(cx.debug_bounds("setup-log-in-codex").is_none(), "signed in: no Log in");
}
