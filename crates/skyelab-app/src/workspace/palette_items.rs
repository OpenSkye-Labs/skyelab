//! What the command palette lists for this workspace: actions, projects, threads and open tabs.

use super::env_mode::{OpenRemoteThread, SelectRemoteProject};
use super::{OpenThread, SelectProject, Workspace};
use crate::palette::{self, Entry, Kind, OpenCommandPalette};
use gpui::{Action, App, Context, Entity, FocusHandle, Focusable, SharedString, Window};
use gpui_component::{dock::DockPlacement, kbd::Kbd};
use std::rc::Rc;

fn dispatch(label: String, kind: Kind, action: Box<dyn Action>) -> Entry {
    Entry {
        label: label.into(),
        kind,
        binding: None,
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
        palette::open(entries, prev, window, cx);
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
            out.extend(self.threads.iter().map(|t| {
                let label = format!("Open thread: {}", t.title);
                dispatch(label, Kind::Thread, Box::new(OpenThread { id: t.id }))
            }));
        }
        out.extend(self.tabs(cx));
        out
    }

    fn tabs(&self, cx: &mut Context<Self>) -> Vec<Entry> {
        let mut tabs = Vec::new();
        // A queued tab isn't in the dock yet, so there is nothing to go to.
        for view in self
            .open_threads
            .iter()
            .filter_map(|t| t.upgrade())
            .filter(|v| !self.queued_tabs.contains(v))
        {
            let title = view.read(cx).title();
            tabs.push(self.tab(title, &view, cx));
        }
        for view in self.remote_tabs() {
            let title = view.read(cx).title();
            tabs.push(self.tab(title, &view, cx));
        }
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
        if let Some(settings) = self.settings.as_ref().and_then(|s| s.upgrade()) {
            tabs.push(self.tab("Settings".into(), &settings, cx));
        }
        if let Some(devices) = self.devices.as_ref().and_then(|d| d.upgrade()) {
            tabs.push(self.tab_in(DockPlacement::Right, "Devices".into(), &devices, cx));
        }
        tabs
    }

    fn tab<T: Focusable>(
        &self,
        title: SharedString,
        view: &Entity<T>,
        cx: &mut Context<Self>,
    ) -> Entry {
        self.tab_in(DockPlacement::Center, title, view, cx)
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
            run: Rc::new(move |window: &mut Window, cx: &mut App| {
                _ = ws.update(cx, |ws, cx| {
                    ws.dock.update(cx, |dock, cx| {
                        if at != DockPlacement::Center && !dock.is_dock_open(at) {
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
