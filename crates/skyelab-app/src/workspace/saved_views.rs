//! The sidebar's saved views (T3's `SavedViewsMenu`): the project scope and the filter text under a name, picked
//! from the bookmark menu. Kept in `settings.toml`'s `[[sidebar_views]]`, each change applied to the file as it is on
//! disk.

use super::Workspace;
use gpui::{Action, App, AppContext as _, Context, Global, SharedString, Window};
use skyelab_core::{AppSettings, SidebarView};
use std::sync::atomic::{AtomicU64, Ordering};

#[cfg(test)]
#[path = "saved_views_test.rs"]
mod tests;

/// Asks a name, then saves the sidebar's scope and filter under it.
#[derive(Clone, PartialEq, Action)]
#[action(namespace = skyelab, no_json)]
pub struct SaveSidebarView;

#[derive(Clone, PartialEq, Action)]
#[action(namespace = skyelab, no_json)]
pub struct ApplySidebarView {
    pub name: SharedString,
}

#[derive(Clone, PartialEq, Action)]
#[action(namespace = skyelab, no_json)]
pub struct DeleteSidebarView {
    pub name: SharedString,
}

/// The saved views now, app-wide.
#[derive(Default)]
pub(crate) struct SidebarViews(pub Vec<SidebarView>);

impl Global for SidebarViews {}

pub(crate) fn views(cx: &App) -> &[SidebarView] {
    cx.try_global::<SidebarViews>().map_or(&[], |v| &v.0)
}

/// Bumped by every change: a write runs only while it is still the newest, checked under the settings file's lock,
/// so two quick changes can't land out of order.
static SAVES: AtomicU64 = AtomicU64::new(0);

impl Workspace {
    /// Applies `change` to the views now, then writes the whole list into the file as it is on disk (its other
    /// settings untouched). A failed write is told to the user.
    fn edit_views(
        &mut self,
        change: impl FnOnce(&mut Vec<SidebarView>),
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let list = {
            let views = &mut cx.default_global::<SidebarViews>().0;
            change(views);
            views.clone()
        };
        self.refresh_sidebar(cx);
        let Some(path) = super::thread_extras::prefs::path(cx) else {
            return;
        };
        let generation = SAVES.fetch_add(1, Ordering::SeqCst) + 1;
        let write = cx.background_spawn(async move {
            AppSettings::update(&path, |s| {
                if SAVES.load(Ordering::SeqCst) == generation {
                    s.sidebar_views = list;
                }
            })
        });
        cx.spawn_in(window, async move |ws, cx| {
            if let Err(e) = write.await {
                let _ = ws.update_in(cx, |ws, window, cx| {
                    ws.say(
                        format!("Couldn't save the sidebar views: {e:#}"),
                        window,
                        cx,
                    )
                });
            }
        })
        .detach(); // one short write; `ws` is weak
    }

    /// The sidebar's scope and filter as a view named `name`.
    fn sidebar_view(&self, name: String) -> SidebarView {
        SidebarView {
            name,
            projects: self
                .projects
                .iter()
                .filter(|p| self.scope.contains(&p.id))
                .map(|p| p.path.clone())
                .collect(),
            search: self
                .thread_search
                .as_ref()
                .map(|(q, _)| q.clone())
                .unwrap_or_default(),
        }
    }

    /// Whether the sidebar shows `view` now (its missing projects left out).
    pub(super) fn shows_view(&self, view: &SidebarView) -> bool {
        let now = self.sidebar_view(String::new());
        let wanted: Vec<_> = self
            .projects
            .iter()
            .filter(|p| view.projects.contains(&p.path))
            .map(|p| &p.path)
            .collect();
        now.search == view.search.trim() && now.projects.iter().collect::<Vec<_>>() == wanted
    }

    /// The name the save dialog suggests: the scope's projects and the filter.
    fn suggested_view_name(&self) -> String {
        let names: Vec<&str> = self
            .projects
            .iter()
            .filter(|p| self.scope.contains(&p.id))
            .map(|p| p.name.as_str())
            .collect();
        let scope = if names.is_empty() {
            "All projects".to_owned()
        } else {
            names.join(", ")
        };
        match &self.thread_search {
            Some((q, _)) => format!("{scope}: {q}"),
            None => scope,
        }
    }

    pub(super) fn save_sidebar_view(
        &mut self,
        _: &SaveSidebarView,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let suggested = self.suggested_view_name();
        self.prompt_text(
            "Save view",
            suggested,
            window,
            cx,
            |ws, name, window, cx| {
                let view = ws.sidebar_view(name);
                // A name saved again replaces that view.
                ws.edit_views(
                    move |views| match views.iter_mut().find(|v| v.name == view.name) {
                        Some(v) => *v = view,
                        None => views.push(view),
                    },
                    window,
                    cx,
                );
            },
        );
    }

    pub(super) fn apply_sidebar_view(
        &mut self,
        a: &ApplySidebarView,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(view) = views(cx).iter().find(|v| v.name == a.name).cloned() else {
            return;
        };
        self.scope = self
            .projects
            .iter()
            .filter(|p| view.projects.contains(&p.path))
            .map(|p| p.id)
            .collect();
        // A programmatic set_value sends no change event: run the search here.
        let search = self.sidebar.read(cx).search.clone();
        search.update(cx, |s, cx| s.set_value(view.search.clone(), window, cx));
        self.search_threads(view.search.into(), cx);
        if let [id] = self.scope[..] {
            self.select(id, cx);
        }
    }

    pub(super) fn delete_sidebar_view(
        &mut self,
        a: &DeleteSidebarView,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let name = a.name.clone();
        self.edit_views(|views| views.retain(|v| v.name != name), window, cx);
    }
}
