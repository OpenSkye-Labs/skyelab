//! Remote mode: which environment the window shows. On "This Mac" everything is local. On a remote one, the
//! sidebar's projects and threads come from its daemon, thread tabs are `RemoteThreadView`s, and local-only
//! actions (a terminal in the project, worktree Discard) don't apply. The two stores never mix: switching closes
//! every remote tab and drops the client.

use super::Workspace;
use super::env_picker::{ConnectTyped, EnvPicker, ToggleEnvironments};
use super::panels::{SidebarItem, SidebarSection};
use super::remote_thread::{RemoteThreadEvent, RemoteThreadView};
use crate::environment::{ConnState, EnvEvent, RemoteEnv};
use gpui::{
    Action, AppContext as _, Context, Entity, InteractiveElement as _, IntoElement, SharedString,
    Stateful, Subscription, Task, WeakEntity, Window, actions, div, prelude::*, px,
};
use gpui_component::{
    ActiveTheme as _, Sizable as _,
    button::{Button, ButtonVariants as _},
    dock::{DockPlacement, panel_handle},
    h_flex,
};
use skyelab_client::{Options, is_magicdns_name};
use skyelab_core::AppSettings;
use skyelab_daemon::SharedTailnet;
use std::path::PathBuf;

actions!(skyelab, [NewRemoteThread]);

#[derive(Clone, PartialEq, Action)]
#[action(namespace = skyelab, no_json)]
pub struct SwitchEnvironment {
    /// `None` is this machine.
    pub host: Option<String>,
}

/// Trust the machine a remembered name now answers from. Carries the node id the warning showed, so a click
/// can't trust a node that took the name after the warning was drawn.
#[derive(Clone, PartialEq, Action)]
#[action(namespace = skyelab, no_json)]
pub struct TrustRemoteNode {
    pub node_id: String,
}

#[derive(Clone, PartialEq, Action)]
#[action(namespace = skyelab, no_json)]
pub struct SelectRemoteProject {
    pub id: i64,
}

#[derive(Clone, PartialEq, Action)]
#[action(namespace = skyelab, no_json)]
pub struct OpenRemoteThread {
    pub id: i64,
}

#[derive(Default)]
pub(super) struct EnvMode {
    picker: Option<Entity<EnvPicker>>,
    remote: Option<Entity<RemoteEnv>>,
    /// Each remote tab with its subscription, which goes when the tab does.
    tabs: Vec<(WeakEntity<RemoteThreadView>, Subscription)>,
    settings_path: Option<PathBuf>,
    options: Options,
    subs: Vec<Subscription>,
    _save: Option<Task<()>>,
}

impl Workspace {
    /// Shows the environment picker. `options` is how remote machines are dialed (tests allow loopback).
    pub fn set_environments(
        &mut self,
        tailnet: SharedTailnet,
        settings_path: Option<PathBuf>,
        options: Options,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let picker = cx.new(|cx| EnvPicker::new(tailnet, window, cx));
        // Kept as the first link of the save chain, so a save made right after startup can't be overwritten by it.
        let load = settings_path.clone().map(|path| {
            let load = cx.background_spawn(async move { AppSettings::load(&path) });
            let weak = picker.downgrade();
            cx.spawn(async move |_, cx| {
                // A broken file shows no recents (the save path then reports it, and leaves the file alone).
                if let Ok(settings) = load.await {
                    let _ = weak.update(cx, |p, cx| {
                        p.recents = settings.recent_environments;
                        p.recents.retain(|h| is_magicdns_name(h));
                        cx.notify();
                    });
                }
            })
        });
        self.sidebar.update(cx, |s, cx| {
            s.picker = Some(picker.clone());
            cx.notify();
        });
        self.env = EnvMode {
            picker: Some(picker),
            settings_path,
            options,
            _save: load,
            ..EnvMode::default()
        };
    }

    pub(super) fn env_actions(
        &self,
        el: Stateful<gpui::Div>,
        cx: &Context<Self>,
    ) -> Stateful<gpui::Div> {
        el.on_action(cx.listener(Self::toggle_environments))
            .on_action(cx.listener(Self::connect_typed))
            .on_action(cx.listener(Self::switch_environment))
            .on_action(cx.listener(Self::select_remote_project))
            .on_action(cx.listener(Self::open_remote_thread))
            .on_action(cx.listener(Self::new_remote_thread))
            .on_action(cx.listener(Self::trust_remote_node))
    }

    pub(crate) fn is_remote(&self) -> bool {
        self.env.remote.is_some()
    }

    #[cfg(test)]
    pub(crate) fn remote_env(&self) -> Option<Entity<RemoteEnv>> {
        self.env.remote.clone()
    }

    #[cfg(test)]
    pub(crate) fn env_picker(&self) -> Option<Entity<EnvPicker>> {
        self.env.picker.clone()
    }

    pub(crate) fn remote_tabs(&self) -> Vec<Entity<RemoteThreadView>> {
        self.env
            .tabs
            .iter()
            .filter_map(|(t, _)| t.upgrade())
            .collect()
    }

    fn toggle_environments(
        &mut self,
        _: &ToggleEnvironments,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(picker) = &self.env.picker {
            picker.update(cx, |p, cx| p.toggle(cx));
        }
    }

    fn connect_typed(&mut self, _: &ConnectTyped, window: &mut Window, cx: &mut Context<Self>) {
        let Some(picker) = self.env.picker.clone() else {
            return;
        };
        let host = picker.read(cx).typed(cx);
        if !is_magicdns_name(&host) {
            picker.update(cx, |p, cx| {
                p.error = Some(
                    format!("“{host}” isn't a MagicDNS name. Use machine.tailnet.ts.net.").into(),
                );
                cx.notify();
            });
            return;
        }
        picker.update(cx, |p, cx| p.clear_typed(window, cx));
        self.switch_environment(&SwitchEnvironment { host: Some(host) }, window, cx);
    }

    pub(super) fn switch_environment(
        &mut self,
        a: &SwitchEnvironment,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let current = self.env.remote.as_ref().map(|e| e.read(cx).host.clone());
        let same = match (&a.host, &current) {
            (Some(new), Some(cur)) => new.eq_ignore_ascii_case(cur),
            (None, None) => true,
            _ => false,
        };
        if same {
            // Already there: keep the connection and its tabs, just close the list.
            if let Some(picker) = &self.env.picker {
                picker.update(cx, |p, cx| {
                    p.open = false;
                    cx.notify();
                });
            }
            return;
        }
        // Leaving an environment closes all of its tabs and drops its client: nothing remote stays around.
        self.close_remote_tabs(window, cx);
        self.env.subs.clear();
        self.env.remote = None;
        let mut error = None;
        if let Some(host) = a.host.clone() {
            match skyelab_client::Client::connect(host.clone(), self.env.options.clone()) {
                Ok(client) => {
                    let settings = self.env.settings_path.clone();
                    let env = cx.new(|cx| RemoteEnv::new(host.clone(), client, settings, cx));
                    self.env.subs.push(cx.observe(&env, |this, _, cx| {
                        this.refresh_sidebar(cx);
                        cx.notify();
                    }));
                    self.env.subs.push(cx.subscribe_in(
                        &env,
                        window,
                        |this, _, e: &EnvEvent, window, cx| {
                            if let EnvEvent::NodeChanged = e {
                                this.close_remote_tabs(window, cx);
                            }
                        },
                    ));
                    self.env.remote = Some(env);
                    self.remember(host, cx);
                }
                Err(e) => error = Some(format!("can't start the connection: {e}").into()),
            }
        }
        let current = self.env.remote.as_ref().map(|e| e.read(cx).host.clone());
        if let Some(picker) = &self.env.picker {
            picker.update(cx, |p, cx| {
                p.current = current;
                p.open = false;
                p.error = error;
                cx.notify();
            });
        }
        self.refresh_sidebar(cx);
        cx.notify();
    }

    fn close_remote_tabs(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        for tab in self.remote_tabs() {
            self.dock
                .update(cx, |dock, cx| dock.remove_panel(tab, window, cx));
        }
        self.env.tabs.clear();
    }

    pub(super) fn trust_remote_node(
        &mut self,
        a: &TrustRemoteNode,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(env) = &self.env.remote {
            env.update(cx, |e, cx| e.trust(&a.node_id, cx));
        }
    }

    /// Saves `host` first in the recents, off the UI thread. A file that doesn't parse is left alone.
    fn remember(&mut self, host: String, cx: &mut Context<Self>) {
        let Some(path) = self.env.settings_path.clone() else {
            return;
        };
        let picker = self.env.picker.as_ref().map(|p| p.downgrade());
        let prev = self.env._save.take();
        let write = cx.background_executor().clone();
        // Chained, so quick switches save in the order they happened.
        self.env._save = Some(cx.spawn(async move |_, cx| {
            if let Some(prev) = prev {
                prev.await;
            }
            let saved = write
                .spawn(async move {
                    let (settings, ()) =
                        AppSettings::update(&path, |s| s.remember_environment(&host))?;
                    anyhow::Ok(settings.recent_environments)
                })
                .await;
            let Some(picker) = picker else { return };
            let _ = picker.update(cx, |p, cx| {
                match saved {
                    Ok(recents) => p.recents = recents,
                    Err(e) => {
                        p.error = Some(format!("couldn't save recent machines: {e:#}").into())
                    }
                }
                cx.notify();
            });
        }));
    }

    /// The sidebar in remote mode: the remote's projects and the selected one's threads. `None` on this machine.
    pub(super) fn remote_sections(&self, cx: &Context<Self>) -> Option<Vec<SidebarSection>> {
        let env = self.env.remote.as_ref()?.read(cx);
        let projects = env
            .projects
            .iter()
            .map(|p| SidebarItem {
                id: format!("remote-project-{}", p.id).into(),
                label: p.name.clone().into(),
                selected: env.project == Some(p.id),
                action: Box::new(SelectRemoteProject { id: p.id }),
            })
            .collect();
        let threads = env
            .threads
            .iter()
            .map(|t| SidebarItem {
                id: format!("remote-thread-{}", t.id).into(),
                label: t.title.clone().into(),
                selected: false,
                action: Box::new(OpenRemoteThread { id: t.id }),
            })
            .collect();
        let connected = env.state == ConnState::Connected;
        let trust = match &env.state {
            // Only a node the connection really presents can be trusted (none: an old server sent no id).
            ConnState::NodeChanged { now, .. } if env.presented_node() == Some(now) => Some((
                SharedString::from("Trust it"),
                Box::new(TrustRemoteNode {
                    node_id: now.clone(),
                }) as Box<_>,
            )),
            _ => None,
        };
        let has_project = env.project.is_some();
        Some(vec![
            SidebarSection {
                title: "Projects".into(),
                items: projects,
                empty_title: match (&env.error, connected) {
                    (Some(e), true) => format!("Couldn't list projects: {e}").into(),
                    (None, true) => "No projects there".into(),
                    _ => env.state.label(),
                },
                empty_hint: match (&trust, &env.error) {
                    (Some(_), Some(e)) => e.clone(),
                    _ => format!("Projects on {}. Add them on that machine.", env.host).into(),
                },
                button: trust,
            },
            SidebarSection {
                title: "Threads".into(),
                items: threads,
                empty_title: match (&env.error, has_project) {
                    (Some(e), true) => format!("Couldn't list threads: {e}").into(),
                    _ => "No threads".into(),
                },
                empty_hint: if has_project {
                    "Start one with New. It runs on the other machine."
                } else {
                    "Select a project to see its threads."
                }
                .into(),
                button: has_project.then(|| {
                    (
                        SharedString::from("New"),
                        Box::new(NewRemoteThread) as Box<_>,
                    )
                }),
            },
        ])
    }

    fn select_remote_project(
        &mut self,
        a: &SelectRemoteProject,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(env) = &self.env.remote {
            env.update(cx, |e, cx| e.select_project(a.id, cx));
        }
    }

    pub(super) fn open_remote_thread(
        &mut self,
        a: &OpenRemoteThread,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(env) = self.env.remote.clone() else {
            return;
        };
        if let Some(tab) = self
            .remote_tabs()
            .into_iter()
            .find(|t| t.read(cx).thread_id() == Some(a.id) && !t.read(cx).closed)
        {
            let id = tab.entity_id().into();
            self.dock
                .update(cx, |dock, cx| dock.select_panel(id, window, cx));
            return;
        }
        let Some(thread) = env.read(cx).threads.iter().find(|t| t.id == a.id).cloned() else {
            return;
        };
        let view = cx.new(|cx| {
            RemoteThreadView::new(
                env,
                thread.project_id,
                Some((thread.id, thread.title)),
                window,
                cx,
            )
        });
        self.add_remote_tab(view, window, cx);
    }

    pub(super) fn new_remote_thread(
        &mut self,
        _: &NewRemoteThread,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(env) = self.env.remote.clone() else {
            return;
        };
        let Some(project_id) = env.read(cx).project else {
            return;
        };
        let view = cx.new(|cx| RemoteThreadView::new(env, project_id, None, window, cx));
        self.add_remote_tab(view, window, cx);
    }

    fn add_remote_tab(
        &mut self,
        view: Entity<RemoteThreadView>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let sub = cx.subscribe(&view, |this, _, e: &RemoteThreadEvent, cx| match e {
            RemoteThreadEvent::Started => {
                if let Some(env) = &this.env.remote {
                    env.update(cx, |e, cx| e.load_threads(cx));
                }
            }
        });
        self.env
            .tabs
            .retain(|(t, _)| t.upgrade().is_some_and(|t| !t.read(cx).closed));
        self.env.tabs.push((view.downgrade(), sub));
        self.dock.update(cx, |dock, cx| {
            dock.add_panel_view(
                panel_handle(view.clone()),
                DockPlacement::Center,
                None,
                window,
                cx,
            );
        });
        view.update(cx, |v, cx| v.focus_prompt(window, cx));
        cx.notify();
    }

    /// "host · state" for the status bar; `None` on this machine.
    pub(crate) fn env_badge_text(&self, cx: &gpui::App) -> Option<String> {
        let env = self.env.remote.as_ref()?.read(cx);
        Some(format!("{} · {}", env.host, env.state.label()))
    }

    pub(super) fn render_env_badge(&self, cx: &Context<Self>) -> Option<impl IntoElement> {
        let text = self.env_badge_text(cx)?;
        let dot = self.env.remote.as_ref()?.read(cx).state.color(cx.theme());
        Some(
            Button::new("status-env")
                .ghost()
                .xsmall()
                .tooltip("You are driving another machine. Click to switch.")
                .child(
                    h_flex()
                        .gap_1p5()
                        .child(div().size(px(6.)).rounded_full().bg(dot))
                        .child(SharedString::from(format!("Remote: {text}"))),
                )
                .on_click(|_, window, cx| window.dispatch_action(Box::new(ToggleEnvironments), cx)),
        )
    }
}
