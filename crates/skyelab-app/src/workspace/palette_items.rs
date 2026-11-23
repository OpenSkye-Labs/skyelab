//! What the command palette lists for this workspace: actions, projects, threads and the docks' tabs.

use super::env_mode::{OpenRemoteThread, SelectRemoteProject};
use super::{OpenThread, SelectProject, Workspace};
use crate::palette::{self, Entry, Kind, OpenCommandPalette};
use gpui::{
    Action, App, AppContext as _, Context, Entity, FocusHandle, Focusable, SharedString, Window,
};
use gpui_component::{dock::DockPlacement, kbd::Kbd};
use std::rc::Rc;

fn dispatch(label: String, kind: Kind, action: Box<dyn Action>) -> Entry {
    Entry {
        label: label.into(),
        kind,
        binding: None,
        checked: None,
        run: Rc::new(move |window, cx| window.dispatch_action(action.boxed_clone(), cx)),
    }
}

impl Workspace {
    pub(super) fn open_palette(
        &mut self,
        _: &OpenCommandPalette,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(prev) = window.focused(cx) else {
            return;
        };
        let entries = self.palette_entries(&prev, window, cx);
        let search = (!self.is_remote()).then(|| self.palette_search());
        palette::open(entries, search, prev, window, cx);
    }

    /// Threads of every project whose title or message text matches, as "Open thread" entries. The selected
    /// project's are labeled like its fixed entries (so a title match isn't listed twice); others name their project.
    pub(crate) fn palette_search(&self) -> palette::Search {
        let store = self.backend.store.clone();
        let current = self.project.as_ref().map(|p| p.id);
        let names: std::collections::HashMap<i64, String> = self
            .projects
            .iter()
            .map(|p| (p.id, p.name.clone()))
            .collect();
        Rc::new(move |query: String, cx: &mut App| {
            let (store, names) = (store.clone(), names.clone());
            let found = cx.background_spawn(async move {
                super::lock(&store).and_then(|s| s.search_threads(&query, None, 50))
            });
            cx.spawn(async move |_| {
                // A failed search adds nothing: the palette's fixed entries still work.
                found
                    .await
                    .unwrap_or_default()
                    .into_iter()
                    .map(|t| {
                        let label = match names.get(&t.project_id) {
                            Some(name) if Some(t.project_id) != current => {
                                format!("Open thread: {} · {name}", t.title)
                            }
                            _ => format!("Open thread: {}", t.title),
                        };
                        dispatch(label, Kind::Thread, Box::new(OpenThread { id: t.id }))
                    })
                    .collect()
            })
        })
    }

    pub(crate) fn palette_entries(
        &self,
        prev: &FocusHandle,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Vec<Entry> {
        let mut actions: Vec<Entry> = Vec::new();
        let names: Vec<&'static str> = cx.all_action_names().to_vec();
        for name in names.into_iter().filter(|n| palette::listed(n)) {
            let Ok(action) = cx.build_action(name, None) else {
                continue;
            };
            if !window.is_action_available_in(action.as_ref(), prev) {
                continue;
            }
            let binding = Kbd::binding_for_action_in(action.as_ref(), prev, window);
            let mut entry = dispatch(palette::humanize(name), Kind::Action(name), action);
            entry.binding = binding;
            // A toggle shows where it stands now, as T3's toggle rows do.
            if name == "skyelab::ToggleSharing" {
                entry.checked = self.sharing_enabled(cx);
            }
            actions.push(entry);
        }
        actions.sort_by(|a, b| a.label.cmp(&b.label));

        let mut out = actions;
        // Another machine shown: its projects and the selected one's threads, as the switcher lists them.
        if let Some(env) = self.remote_env() {
            let env = env.read(cx);
            out.extend(env.projects.iter().map(|p| {
                let label = format!("Switch to project: {}", p.name);
                dispatch(
                    label,
                    Kind::Project,
                    Box::new(SelectRemoteProject { id: p.id }),
                )
            }));
            out.extend(env.threads.iter().map(|t| {
                let label = format!("Open thread: {}", t.title);
                dispatch(label, Kind::Thread, Box::new(OpenRemoteThread { id: t.id }))
            }));
        } else {
            out.extend(self.projects.iter().map(|p| {
                let label = format!("Switch to project: {}", p.name);
                dispatch(label, Kind::Project, Box::new(SelectProject { id: p.id }))
            }));
            out.extend(self.scripts.iter().map(|s| {
                let label = format!("Run script: {}", s.name);
                let action = super::scripts::RunProjectScript { id: s.id };
                dispatch(
                    label,
                    Kind::Action("skyelab::RunProjectScript"),
                    Box::new(action),
                )
            }));
            out.extend(self.threads.iter().map(|t| {
                let label = format!("Open thread: {}", t.title);
                dispatch(label, Kind::Thread, Box::new(OpenThread { id: t.id }))
            }));
        }
        out.extend(self.tabs(cx));
        out
    }

    /// The docks' tabs (terminals, Changes, browsers, Devices). The center has pages, not tabs: threads and
    /// Settings are reached through their own entries.
    fn tabs(&self, cx: &mut Context<Self>) -> Vec<Entry> {
        let mut tabs = Vec::new();
        for view in self.changes_views.iter().filter_map(|v| v.upgrade()) {
            let title = view.read(cx).title();
            tabs.push(self.tab_in(DockPlacement::Right, title, &view, cx));
        }
        for (i, panel) in self.terminals.iter().enumerate() {
            let title = format!("Terminal {}", i + 1).into();
            tabs.push(self.tab_in(DockPlacement::Bottom, title, panel, cx));
        }
        for panel in self.browsers.iter().filter_map(|b| b.upgrade()) {
            tabs.push(self.tab_in(DockPlacement::Right, "Browser".into(), &panel, cx));
        }
        if let Some(devices) = self.devices.as_ref().and_then(|d| d.upgrade()) {
            tabs.push(self.tab_in(DockPlacement::Right, "Devices".into(), &devices, cx));
        }
        tabs
    }

    fn tab_in<T: Focusable>(
        &self,
        at: DockPlacement,
        title: SharedString,
        view: &Entity<T>,
        cx: &mut Context<Self>,
    ) -> Entry {
        let (ws, id, focus) = (
            cx.entity().downgrade(),
            view.entity_id(),
            view.read(cx).focus_handle(cx),
        );
        Entry {
            label: format!("Go to tab: {title}").into(),
            kind: Kind::Tab,
            binding: None,
            checked: None,
            run: Rc::new(move |window: &mut Window, cx: &mut App| {
                _ = ws.update(cx, |ws, cx| {
                    ws.dock.update(cx, |dock, cx| {
                        if !dock.is_dock_open(at) {
                            dock.toggle_dock(at, window, cx);
                        }
                        dock.select_panel(id.into(), window, cx)
                    })
                });
                focus.focus(window, cx);
            }),
        }
    }
}
