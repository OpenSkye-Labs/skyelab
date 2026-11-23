//! Shared rig for package D's tests: a workspace in a `Root` window (dialogs and notices need it) with the
//! fixture's project added and selected.

use super::threads_test::{Fixture, fake_bin, fixture, setup, wait_until};
use super::*;
use gpui::{TestAppContext, VisualTestContext};
use gpui_component::{Root, WindowExt as _};
use skyelab_store::NewThread as NewRow;

pub(super) struct Rig {
    pub f: Fixture,
    pub ws: Entity<Workspace>,
    pub project: i64,
}

pub(super) fn rig(cx: &mut TestAppContext) -> (Rig, &mut VisualTestContext) {
    setup(cx);
    let f = fixture(true);
    let config = AccountsConfig::load(&f.accounts_path).expect("load");
    let backend = Backend {
        store: f.store.clone(),
        claude_bin: fake_bin(),
    };
    let folder = f.project_dir.parent().expect("parent").to_path_buf();
    let path = f.accounts_path.clone();
    let mut ws = None;
    let window = cx.add_window(|window, cx| {
        let view =
            cx.new(|cx| Workspace::new(folder, Some(path), config, None, backend, window, cx));
        let focus = view.read(cx).focus.clone();
        window.focus(&focus, cx);
        ws = Some(view.clone());
        Root::new(view, window, cx)
    });
    let cx = VisualTestContext::from_window(window.into(), cx).into_mut();
    let ws = ws.expect("workspace built");
    cx.update(|window, _| window.activate_window()); // the user is looking at the window
    cx.run_until_parked();
    let dir = f.project_dir.clone();
    ws.update(cx, |ws, cx| ws.add_project(dir, cx));
    wait_until(cx, "project stored", |cx| {
        ws.read_with(cx, |ws, _| ws.project.is_some())
    });
    let project = ws.read_with(cx, |ws, _| ws.projects[0].id);
    (Rig { f, ws, project }, cx)
}

impl Rig {
    /// A stored thread in the rig's project (no agent), listed once the workspace reloads.
    pub fn thread(&self, title: &str, cx: &mut VisualTestContext) -> i64 {
        let id = {
            let store = self.f.store.lock().expect("lock");
            store
                .create_thread(&NewRow {
                    project_id: self.project,
                    title,
                    cwd: &self.f.project_dir,
                    account_id: "acme",
                    status: "idle",
                })
                .expect("thread")
                .id
        };
        self.reload(cx);
        id
    }

    pub fn reload(&self, cx: &mut VisualTestContext) {
        self.ws.update(cx, |ws, cx| ws.load_threads(cx));
        cx.run_until_parked();
    }

    pub fn row(&self, id: i64) -> Option<skyelab_store::Thread> {
        self.f.store.lock().expect("lock").thread(id).expect("read")
    }

    /// The sidebar's thread rows (titles) in drawn order: pinned, working, the rest.
    pub fn listed(&self, cx: &mut VisualTestContext) -> Vec<String> {
        let sidebar = self.ws.read_with(cx, |ws, _| ws.sidebar.clone());
        sidebar.read_with(cx, |s, _| {
            s.sections
                .iter()
                .flat_map(|s| s.items.iter().map(|i| i.label.to_string()))
                .collect()
        })
    }

    pub fn archived(&self, cx: &mut VisualTestContext) -> Vec<String> {
        let sidebar = self.ws.read_with(cx, |ws, _| ws.sidebar.clone());
        sidebar.read_with(cx, |s, _| {
            s.archived
                .as_ref()
                .map(|a| a.items.iter().map(|i| i.label.to_string()).collect())
                .unwrap_or_default()
        })
    }

    /// (pinned, unread) of a thread row.
    pub fn marks(&self, id: i64, cx: &mut VisualTestContext) -> (bool, bool) {
        let sidebar = self.ws.read_with(cx, |ws, _| ws.sidebar.clone());
        sidebar.read_with(cx, |s, _| {
            s.marks
                .get(&SharedString::from(format!("thread-{id}")))
                .map_or((false, false), |m| (m.pinned, m.unread))
        })
    }

    pub fn notices(&self, cx: &mut VisualTestContext) -> Vec<String> {
        self.ws.read_with(cx, |ws, _| {
            ws.notices.iter().map(|n| n.to_string()).collect()
        })
    }

    pub fn dialog_open(&self, cx: &mut VisualTestContext) -> bool {
        cx.update(|window, cx| window.has_active_dialog(cx))
    }

    /// Opens a stored thread's tab (no agent starts until a prompt is sent).
    pub fn open(&self, id: i64, cx: &mut VisualTestContext) -> Entity<thread::ThreadView> {
        cx.dispatch_action(OpenThread { id });
        cx.run_until_parked();
        let ws = self.ws.clone();
        wait_until(cx, "the tab opened", |cx| {
            ws.read_with(cx, |ws, cx| ws.open_view(id, cx).is_some())
        });
        ws.read_with(cx, |ws, cx| ws.open_view(id, cx))
            .expect("open")
    }
}
