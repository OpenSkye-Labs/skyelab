//! Shared by the Add project tests: a workspace in a `Root` window over a temp home with two accounts (one with its
//! own gh dir), a git repo and plain folders under `~/code`, and an accounts.toml on disk; helpers to drive the page.

use super::palette_test::setup;
use super::threads_test::{test_backend, wait_until};
use super::*;
use gpui::{TestAppContext, VisualTestContext};
use gpui_component::Root;
use skyelab_core::{Account, Binding, Provider};
use std::path::{Path, PathBuf};

pub(super) struct Rig {
    pub _root: tempfile::TempDir,
    pub home: PathBuf,
    pub accounts: PathBuf,
    pub ws: Entity<Workspace>,
}

pub(super) fn account(id: &str, home: &Path, gh: bool) -> Account {
    Account {
        id: id.into(),
        label: id.to_uppercase(),
        provider: Provider::Claude,
        config_dir: home.join(format!(".claude-{id}")),
        gh_config_dir: gh.then(|| home.join(format!(".config/gh-{id}"))),
        ssh_agent: false,
    }
}

/// `~/code/app` (git, on main), `~/code/api`, `~/bound/` bound to `work`; accounts `work` (gh) and `home`.
/// `edit` may change the config and home before the window opens.
pub(super) fn rig(
    cx: &mut TestAppContext,
    edit: impl FnOnce(&Path, &mut AccountsConfig),
) -> (Rig, &mut VisualTestContext) {
    setup(cx);
    let root = tempfile::tempdir().expect("tempdir");
    let home = std::fs::canonicalize(root.path()).expect("canon").join("home");
    for d in ["code/app/.git", "code/api", "bound/inner", ".config/gh-work"] {
        std::fs::create_dir_all(home.join(d)).expect("mkdir");
    }
    std::fs::write(home.join("code/app/.git/HEAD"), "ref: refs/heads/main\n").expect("head");
    let mut config = AccountsConfig {
        accounts: vec![account("work", &home, true), account("home", &home, false)],
        ..Default::default()
    };
    for a in &config.accounts {
        std::fs::create_dir_all(&a.config_dir).expect("mkdir");
    }
    config
        .add_binding(Binding::new(home.join("bound"), "work".into()))
        .expect("bind ~/bound");
    edit(&home, &mut config);
    let accounts = root.path().join("config/accounts.toml");
    std::fs::create_dir_all(accounts.parent().expect("parent")).expect("mkdir");
    config.save(&accounts).expect("save");
    let gh = Path::new(env!("CARGO_MANIFEST_DIR")).join("../skyelab-git/tests/fixtures/fake-gh.sh");
    cx.update(|cx| cx.set_global(github::GhBin(gh)));
    let (mut ws, path, h) = (None, accounts.clone(), home.clone());
    let window = cx.add_window(|window, cx| {
        let view = cx.new(|cx| {
            Workspace::new(h.clone(), Some(path), config, None, test_backend(), window, cx)
        });
        let setup = view.read(cx).welcome.read(cx).setup.clone();
        setup.update(cx, |o, _| o.home = Some(h));
        let focus = view.read(cx).focus.clone();
        window.focus(&focus, cx);
        ws = Some(view.clone());
        Root::new(view, window, cx)
    });
    let cx = VisualTestContext::from_window(window.into(), cx).into_mut();
    cx.run_until_parked();
    let rig = Rig {
        _root: root,
        home,
        accounts,
        ws: ws.expect("workspace built"),
    };
    (rig, cx)
}

impl Rig {
    pub fn saved(&self) -> AccountsConfig {
        AccountsConfig::load(&self.accounts).expect("load")
    }

    pub fn bound_to(&self, dir: &Path) -> Option<String> {
        self.saved().resolve(dir).map(|a| a.id.clone())
    }

    pub fn projects(&self, cx: &mut VisualTestContext) -> Vec<PathBuf> {
        self.ws
            .read_with(cx, |ws, _| ws.projects.iter().map(|p| p.path.clone()).collect())
    }

    pub fn wait_project(&self, dir: &Path, cx: &mut VisualTestContext) {
        wait_until(cx, "the project is stored", |cx| {
            self.ws
                .read_with(cx, |ws, _| ws.projects.iter().any(|p| p.path == dir))
        });
    }

    /// Opens Add project the way the sidebar button does.
    pub fn open_add(&self, cx: &mut VisualTestContext) {
        let focus = self.ws.read_with(cx, |ws, _| ws.focus.clone());
        cx.update(|window, cx| {
            window.focus(&focus, cx);
            crate::workspace::dispatch(Box::new(AddProject), window, cx);
        });
        cx.run_until_parked();
    }
}

pub(super) fn shown(cx: &mut VisualTestContext) -> crate::palette::Shown {
    cx.update(|_, cx| crate::palette::shown(cx)).expect("a palette is open")
}

pub(super) fn dialog_open(cx: &mut VisualTestContext) -> bool {
    cx.update(gpui_component::WindowExt::has_active_dialog)
}

/// Types into the palette's field and lets the folder listing land.
pub(super) fn type_in(text: &str, cx: &mut VisualTestContext) {
    cx.simulate_input(text);
    cx.run_until_parked();
}

pub(super) fn keys(keys: &str, cx: &mut VisualTestContext) {
    cx.simulate_keystrokes(keys);
    cx.run_until_parked();
}

/// The row titled `title`, highlighted with ↓ from the first row, then Enter.
pub(super) fn choose(title: &str, cx: &mut VisualTestContext) {
    let s = shown(cx);
    let at = s
        .rows
        .iter()
        .position(|r| r.1 == title)
        .unwrap_or_else(|| panic!("no row {title:?} in {s:?}"));
    for _ in 0..at {
        keys("down", cx);
    }
    keys("enter", cx);
}
