//! Every open window is saved to settings.toml (frame and page) and opens again at the next launch; an older file
//! with only the single `window` slot still restores that one window.

use super::multi_window_test::{in_window, seed};
use super::threads_test::{fixture, open_workspace, setup};
use super::*;
use gpui::{Bounds, TestAppContext, point, size};
use skyelab_core::{AppSettings, WindowLayout};
use std::time::Duration;

fn layout(x: i32, view: &str) -> WindowLayout {
    WindowLayout {
        x,
        y: 40,
        width: 1000,
        height: 700,
        view: view.into(),
        ..Default::default()
    }
}

#[gpui::test]
fn every_window_is_saved_and_restored(cx: &mut TestAppContext) {
    setup(cx);
    let f = fixture(true);
    let id = seed(&f.store, &f.project_dir);
    let path = f.project_dir.parent().expect("root").join("settings.toml");
    let (a, vcx) = open_workspace(&f, cx);
    let a_window = a.read_with(vcx, |ws, _| ws.window_handle);
    vcx.dispatch_action(NewWindow);
    vcx.run_until_parked();
    let b_window = vcx.update(|_, cx| window_handles(cx))[1];
    let b = vcx.update(|_, cx| windows::workspaces(cx)[1].clone());
    for (w, ws) in [(a_window, &a), (b_window, &b)] {
        let path = path.clone();
        in_window(w, ws, vcx, move |ws, window, cx| {
            ws.restore_layout(Some(path), None, window, cx)
        });
    }
    in_window(a_window, &a, vcx, |ws, window, cx| {
        ws.open_thread(&OpenThread { id }, window, cx)
    });
    in_window(b_window, &b, vcx, |ws, window, cx| {
        ws.show(center::Page::Welcome, window, cx);
        ws.schedule_layout_save(window, cx);
    });
    vcx.run_until_parked();
    let settle = |vcx: &mut gpui::VisualTestContext| {
        vcx.executor().advance_clock(Duration::from_secs(5));
        vcx.run_until_parked();
    };
    settle(vcx);
    let saved = AppSettings::load(&path).expect("settings");
    let views: Vec<_> = saved.windows.iter().map(|w| w.view.clone()).collect();
    assert_eq!(views, vec![format!("thread:{id}"), "welcome".to_owned()]);
    assert!(saved.window.is_none(), "the list replaces the old slot");

    // A window the user closes leaves the list; the one left stays.
    vcx.update(|_, cx| {
        b_window
            .update(cx, |_, window, _| window.remove_window())
            .expect("B open")
    });
    vcx.run_until_parked();
    settle(vcx);
    let saved = AppSettings::load(&path).expect("settings");
    assert_eq!(saved.windows.len(), 1);
    assert_eq!(saved.windows[0].view, format!("thread:{id}"));

    // The next launch: every saved window opens with its frame and page.
    let screen = Bounds::new(point(px(0.), px(0.)), size(px(3000.), px(1200.)));
    let list = vec![layout(10, &format!("thread:{id}")), layout(1200, "welcome")];
    let opened = restore(&f, list, screen, cx);
    assert_eq!(opened.len(), 2);
    let pages: Vec<_> = cx.update(|cx| opened.iter().map(|w| w.read(cx).page.key(cx)).collect());
    assert_eq!(pages, vec![format!("thread:{id}"), "welcome".to_owned()]);
    let xs: Vec<_> = cx.update(|cx| {
        opened
            .iter()
            .map(|w| {
                w.read(cx)
                    .window_handle
                    .update(cx, |_, window, _| window.bounds().origin.x)
                    .expect("open")
            })
            .collect()
    });
    assert_eq!(xs, vec![px(10.), px(1200.)]);

    // Two windows restoring a thread no window has open yet both show it (their reads race).
    let fresh = {
        let s = f.store.lock().expect("store");
        let project = s.list_projects().expect("projects")[0].id;
        s.create_thread(&skyelab_store::NewThread {
            project_id: project,
            title: "unopened",
            cwd: &f.project_dir,
            account_id: "acme",
            status: "idle",
        })
        .expect("thread")
        .id
    };
    let key = format!("thread:{fresh}");
    let both = restore(&f, vec![layout(10, &key), layout(1200, &key)], screen, cx);
    let pages: Vec<_> = cx.update(|cx| both.iter().map(|w| w.read(cx).page.key(cx)).collect());
    assert_eq!(
        pages,
        vec![key.clone(), key],
        "both windows show the restored thread"
    );

    // An older settings.toml with the single slot only.
    let old_path = path.with_file_name("old-settings.toml");
    std::fs::write(
        &old_path,
        "[window]\nx = 5\ny = 5\nwidth = 900\nheight = 600\nview = \"welcome\"\n",
    )
    .expect("old file");
    let old = AppSettings::load(&old_path).expect("an old file loads");
    assert_eq!(old.saved_windows().len(), 1);
    assert_eq!(restore(&f, old.saved_windows(), screen, cx).len(), 1);
}

fn restore(
    f: &super::threads_test::Fixture,
    list: Vec<WindowLayout>,
    screen: Bounds<gpui::Pixels>,
    cx: &mut TestAppContext,
) -> Vec<Entity<Workspace>> {
    let (folder, path, store) = (
        f.project_dir.clone(),
        f.accounts_path.clone(),
        f.store.clone(),
    );
    let opened = cx
        .update(|cx| {
            windows::restore_windows(
                list,
                None,
                &[screen],
                screen,
                move |_, window, cx| {
                    let config = AccountsConfig::load(&path).expect("load");
                    let backend = Backend {
                        store: store.clone(),
                        claude_bin: super::threads_test::fake_bin(),
                    };
                    Workspace::new(
                        folder.clone(),
                        Some(path.clone()),
                        config,
                        None,
                        backend,
                        window,
                        cx,
                    )
                },
                cx,
            )
        })
        .expect("windows");
    cx.run_until_parked();
    opened
}
