//! Keeps an open thread view's project name in step with the store.

use super::super::thread::ThreadView;
use gpui::{App, Entity};

/// Gives `view` its project's stored name, when it differs.
pub(super) fn name_project(
    projects: &[skyelab_store::Project],
    view: &Entity<ThreadView>,
    cx: &mut App,
) {
    let id = view.read(cx).project_id;
    let Some(p) = projects.iter().find(|p| p.id == id) else {
        return;
    };
    if view.read(cx).project_name.as_ref() != p.name {
        let name = p.name.clone().into();
        view.update(cx, |v, cx| {
            v.project_name = name;
            cx.notify();
        });
    }
}
