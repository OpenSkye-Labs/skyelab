//! What the command palette lists for this workspace: actions, projects, threads and the docks' tabs.

use super::env_mode::{OpenRemoteThread, SelectRemoteProject};
use super::{OpenThread, SelectProject, Workspace};
use crate::palette::{self, Entry, Kind, OpenCommandPalette};
use crate::ui::dock::DockPlacement;
use crate::ui::{Kbd, SkyeIcon};
use gpui::{
    Action, App, AppContext as _, Context, Entity, FocusHandle, Focusable, SharedString, Window,
};
use std::rc::Rc;

fn dispatch(label: String, kind: Kind, action: Box<dyn Action>) -> Entry {
    Entry::new(
        label,
        kind,
        Rc::new(move |window, cx| window.dispatch_action(action.boxed_clone(), cx)),
    )
}

/// A project's folder as its row's second line, home written `~`.
fn short(path: &std::path::Path) -> String {
    crate::ui::short_path(path, dirs::home_dir().as_deref())
}

/// A row that only opens its submenu.
fn submenu(label: &str, icon: SkyeIcon, items: Vec<Entry>) -> Entry {
    Entry::new(
        label.to_owned(),
        Kind::Action("skyelab::Submenu"),
        Rc::new(|_, _| {}),
    )
    .icon(icon)
    .submenu(items)
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
        let search = match self.is_remote() {
            true => self.forward_search(cx),
            false => Some(self.palette_search()),
        };
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
                    .map(|skyelab_store::SearchHit { thread: t, snippet }| {
                        let other = names
                            .get(&t.project_id)
                            .filter(|_| Some(t.project_id) != current);
                        let label = match other {
                            Some(name) => format!("Open thread: {} · {name}", t.title),
                            None => format!("Open thread: {}", t.title),
                        };
                        let entry =
                            dispatch(label, Kind::Thread, Box::new(OpenThread { id: t.id }))
                                .title(t.title.clone())
                                .recency(t.updated_at);
                        // The matched line, after the project's name when it is another project's thread.
                        let subtitle = match (other, snippet) {
                            (Some(name), Some(s)) => Some(format!("{name} · {s}")),
                            (Some(name), None) => Some(name.clone()),
                            (None, s) => s,
                        };
                        match subtitle {
                            Some(s) => entry.subtitle(s),
                            None => entry,
                        }
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
            let binding = Kbd::for_action_in(action.as_ref(), prev, window);
            let mut entry =
                dispatch(palette::humanize(name), Kind::Action(name), action).binding(binding);
            // A toggle shows where it stands now, as T3's toggle rows do.
            match name {
                "skyelab::ToggleSharing" => entry.checked = self.sharing_enabled(cx),
                "skyelab::ToggleArchivedThreads" => entry.checked = Some(self.show_archived),
                _ => {}
            }
            // T3's "New thread in **project**".
            if let (Some(p), "skyelab::NewThread") = (&self.project, name) {
                let title = format!("New thread in {}", p.name);
                entry = entry
                    .bold(title.len() - p.name.len()..title.len())
                    .title(title);
            }
            actions.push(entry);
        }
        actions.sort_by(|a, b| a.label.cmp(&b.label));

        let mut out = actions;
        // Another machine shown: its projects and the selected one's threads, as the switcher lists them.
        if let Some(env) = self.remote_env() {
            let forwards = self.forward_entries(cx);
            if !forwards.is_empty() {
                out.push(submenu("Forwarded ports", SkyeIcon::Plug, forwards));
            }
            let env = env.read(cx);
            out.extend(env.projects.iter().map(|p| {
                let label = format!("Switch to project: {}", p.name);
                dispatch(
                    label,
                    Kind::Project,
                    Box::new(SelectRemoteProject { id: p.id }),
                )
                .title(p.name.clone())
            }));
            out.extend(env.threads.iter().map(|t| {
                let label = format!("Open thread: {}", t.title);
                dispatch(label, Kind::Thread, Box::new(OpenRemoteThread { id: t.id }))
                    .title(t.title.clone())
            }));
        } else {
            if !self.projects.is_empty() {
                let items = self.new_thread_in_entries();
                out.push(submenu("New thread in…", SkyeIcon::NewThread, items));
            }
            out.extend(self.projects.iter().map(|p| {
                let label = format!("Switch to project: {}", p.name);
                dispatch(label, Kind::Project, Box::new(SelectProject { id: p.id }))
                    .title(p.name.clone())
                    .subtitle(short(&p.path))
            }));
            out.extend(self.scripts.iter().map(|s| {
                let label = format!("Run script: {}", s.name);
                let action = super::scripts::RunProjectScript { id: s.id };
                dispatch(
                    label,
                    Kind::Action("skyelab::RunProjectScript"),
                    Box::new(action),
                )
                .title(s.name.clone())
            }));
            // Every project's threads; another project's name their project, as search results do.
            let current = self.project.as_ref().map(|p| p.id);
            out.extend(self.threads.iter().map(|t| {
                let other = self
                    .projects
                    .iter()
                    .find(|p| p.id == t.project_id && Some(p.id) != current);
                let label = match other {
                    Some(p) => format!("Open thread: {} · {}", t.title, p.name),
                    None => format!("Open thread: {}", t.title),
                };
                let entry = dispatch(label, Kind::Thread, Box::new(OpenThread { id: t.id }))
                    .title(t.title.clone())
                    .recency(t.updated_at);
                match other {
                    Some(p) => entry.subtitle(p.name.clone()),
                    None => entry,
                }
            }));
        }
        // Linked machines' toggles and threads; only on this machine, where their rows show.
        if !self.is_remote() {
            out.extend(self.linked_palette_entries(cx));
        }
        out.extend(self.tabs(cx));
        out.push(submenu(
            "Change theme",
            SkyeIcon::Palette,
            theme_entries(cx),
        ));
        out
    }

    /// "New thread in: <project>" for each project: the "New thread in…" page.
    pub(super) fn new_thread_in_entries(&self) -> Vec<Entry> {
        self.projects
            .iter()
            .map(|p| {
                let action = Box::new(super::NewThreadIn { id: p.id });
                // The account the thread will run on; the folder's binding is still checked when it starts.
                let account = self
                    .config
                    .resolve(&p.path)
                    .map_or("No account", |a| a.label.as_str());
                dispatch(format!("New thread in: {}", p.name), Kind::Project, action)
                    .title(p.name.clone())
                    .subtitle(format!("{} · {account}", short(&p.path)))
            })
            .collect()
    }

    /// The docks' tabs (terminals, Changes, browsers, Devices). The center has pages, not tabs: threads and
    /// Settings are reached through their own entries.
    fn tabs(&self, cx: &mut Context<Self>) -> Vec<Entry> {
        let mut tabs = Vec::new();
        for view in self.changes_views.iter().filter_map(|v| v.upgrade()) {
            let title = view.read(cx).title();
            tabs.push(
                self.tab_in(DockPlacement::Right, title, &view, cx)
                    .icon(SkyeIcon::FileDiff),
            );
        }
        // Another thread's terminals are out of the dock until it shows.
        let docked: Vec<_> = self
            .terminals
            .iter()
            .filter(|p| p.read(cx).docked)
            .cloned()
            .collect();
        for panel in &docked {
            let title = panel.read(cx).tab_label();
            tabs.push(
                self.tab_in(DockPlacement::Bottom, title, panel, cx)
                    .icon(SkyeIcon::SquareTerminal),
            );
        }
        for panel in self.browsers.iter().filter_map(|b| b.upgrade()) {
            let title = panel.read(cx).tab_title(cx);
            tabs.push(
                self.tab_in(DockPlacement::Right, title, &panel, cx)
                    .icon(SkyeIcon::Browser),
            );
        }
        if let Some(devices) = self.devices.as_ref().and_then(|d| d.upgrade()) {
            tabs.push(
                self.tab_in(DockPlacement::Right, "Devices".into(), &devices, cx)
                    .icon(SkyeIcon::Smartphone),
            );
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
        let label = format!("Go to tab: {title}");
        Entry::new(
            label,
            Kind::Tab,
            Rc::new(move |window: &mut Window, cx: &mut App| {
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
        )
        .title(title)
    }
}

/// "Theme: …" (T3's theme items): the modes, then each theme family, the current ones checked. Each applies at once in
/// every window and is saved like Settings → Appearance.
fn theme_entries(cx: &App) -> Vec<Entry> {
    use crate::ui::themes;
    use skyelab_core::ColorMode;
    let now = themes::get(cx);
    let entry =
        |label: &str, on: bool, change: Rc<dyn Fn(&mut skyelab_core::AppearanceSettings)>| {
            let run: palette::Run = Rc::new(move |_, cx| {
                let mut s = themes::get(cx);
                change(&mut s);
                gpui::TaskExt::detach_and_log_err(themes::set(s, cx), cx);
            });
            Entry::new(
                format!("Theme: {label}"),
                Kind::Action("skyelab::Theme"),
                run,
            )
            .title(label.to_owned())
            .checked(Some(on))
        };
    let modes = [
        (ColorMode::System, "System"),
        (ColorMode::Light, "Light"),
        (ColorMode::Dark, "Dark"),
    ];
    let mut out: Vec<Entry> = modes
        .into_iter()
        .map(|(m, l)| entry(l, now.mode == m, Rc::new(move |s| s.mode = m)))
        .collect();
    out.extend(themes::FAMILIES.iter().map(|f| {
        let on = themes::family(&now.theme).key == f.key;
        entry(f.label, on, Rc::new(move |s| s.theme = f.key.into()))
    }));
    out
}

#[cfg(test)]
mod theme_entries_test {
    use gpui::TestAppContext;

    #[gpui::test]
    fn theme_entries_list_modes_and_themes_and_switch(cx: &mut TestAppContext) {
        cx.update(|cx| {
            gpui_component::init(cx);
            crate::ui::install_theme(cx).expect("themes load");
        });
        let cx = cx.add_empty_window();
        let entries = cx.update(|_, cx| super::theme_entries(cx));
        let labels: Vec<_> = entries
            .iter()
            .map(|e| (e.label.to_string(), e.checked))
            .collect();
        assert!(
            labels.contains(&("Theme: System".into(), Some(true))),
            "{labels:?}"
        );
        assert!(
            labels.contains(&("Theme: SkyeLab".into(), Some(true))),
            "{labels:?}"
        );
        let dawn = entries
            .iter()
            .find(|e| e.label.as_ref() == "Theme: Dawn")
            .expect("Dawn listed");
        assert_eq!(dawn.checked, Some(false));
        let run = dawn.run.clone();
        cx.update(|window, cx| run(window, cx));
        cx.update(|_, cx| {
            assert_eq!(
                crate::ui::themes::get(cx).theme,
                "dawn",
                "running it switches"
            )
        });
    }
}
