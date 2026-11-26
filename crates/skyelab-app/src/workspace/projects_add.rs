//! Adding folders as projects.

use super::super::{Workspace, lock};
use gpui::{AppContext as _, Context};
use std::path::PathBuf;

impl Workspace {
    /// Stores `dir` as a project, or finds the one already stored there, and selects it.
    pub fn add_project(&mut self, dir: PathBuf, cx: &mut Context<Self>) {
        self.add_projects(vec![dir], cx);
    }

    /// Stores each of `dirs` as a project (or finds the one stored there) in one go, and selects the first.
    pub fn add_projects(&mut self, dirs: Vec<PathBuf>, cx: &mut Context<Self>) {
        let project_count = dirs.len();
        let store = self.backend.store.clone();
        let task = cx.background_spawn(async move {
            let store = lock(&store)?;
            let mut first = None;
            for dir in dirs {
                // A removed project comes back with its threads.
                let id = match store.readd_project(&dir)? {
                    Some(p) => p.id,
                    None => {
                        let name = dir.file_name().map_or_else(
                            || dir.display().to_string(),
                            |n| n.to_string_lossy().into_owned(),
                        );
                        store.create_project(&name, &dir)?.id
                    }
                };
                first.get_or_insert(id);
            }
            first.ok_or_else(|| anyhow::anyhow!("no folder to add"))
        });
        // Detached: each add finishes even when another starts (onboarding's import beside a palette add).
        cx.spawn(async move |this, cx| {
            let added = task.await;
            let _ = this.update(cx, |this, cx| match added {
                Ok(id) => {
                    // The new project's threads must show, and New thread must not start in an older scope.
                    this.scope.clear();
                    this.load_projects(Some(id), cx);
                    crate::posthog::capture(
                        "project_added",
                        [("project_count", serde_json::json!(project_count))],
                    );
                }
                Err(e) => {
                    this.store_error = Some(format!("{e:#}").into());
                    cx.notify();
                }
            });
        })
        .detach();
    }
}
