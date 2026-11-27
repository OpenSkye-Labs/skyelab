//! Remote mode: which environment the window shows. On "This Mac" everything is local. On a remote one, the
//! sidebar's projects and threads come from its daemon, thread tabs are `RemoteThreadView`s, and local-only
//! actions (a terminal in the project, worktree Discard) don't apply. The two stores never mix: switching closes
//! every remote tab and drops the client.

use super::Workspace;
use super::center::Page;
use super::env_picker::{
    ConnectTyped, EnvPicker, ForgetEnvironment, MENU_W, ShowEnvironments, ToggleEnvironments,
};
use super::remote_thread::{RemoteThreadEvent, RemoteThreadView};
use crate::environment::{EnvEvent, RemoteEnv};
use crate::ui::Button;
use gpui::{
    Action, AnimationExt as _, App, AppContext as _, Context, Entity, InteractiveElement as _,
    IntoElement, SharedString, Stateful, Subscription, Task, Window, actions, div, prelude::*, px,
};
use gpui_component::{ActiveTheme as _, h_flex};
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

/// The windows that show another machine.
#[derive(Default)]
struct RemoteWindows(std::collections::HashSet<gpui::AnyWindowHandle>);

impl gpui::Global for RemoteWindows {}

/// Whether `window` shows another machine.
pub(super) fn shows_remote(cx: &gpui::App, window: gpui::AnyWindowHandle) -> bool {
    cx.try_global::<RemoteWindows>()
        .is_some_and(|w| w.0.contains(&window))
}

#[path = "env_mode_save.rs"]
mod save;

#[derive(Default)]
pub(super) struct EnvMode {
    pub(super) picker: Option<Entity<EnvPicker>>,
    remote: Option<Entity<RemoteEnv>>,
    /// Each remote thread view opened on this machine, with its subscription. Kept (and watching) while another page
    /// shows; dropped when the machine changes.
    pub(super) tabs: Vec<(Entity<RemoteThreadView>, Subscription)>,
    /// Machines whose threads show in this machine's list (`env_linked.rs`), each on its own connection.
    pub(super) linked: Vec<super::env_linked::Linked>,
    /// The linked names as saved, also those whose connection couldn't start, so they can still be unlinked.
    pub(super) linked_hosts: Vec<String>,
    pub(super) settings_path: Option<PathBuf>,
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
            cx.spawn_in(window, async move |this, cx| {
                // A broken file shows no recents (the save path then reports it, and leaves the file alone).
                if let Ok(settings) = load.await {
                    let _ = weak.update(cx, |p, cx| {
                        p.recents = settings.recent_environments;
                        p.recents.retain(|h| is_magicdns_name(h));
                        cx.notify();
                    });
                    let mut linked = settings.linked_environments;
                    linked.retain(|h| is_magicdns_name(h));
                    let _ =
                        this.update_in(cx, |this, window, cx| this.sync_linked(linked, window, cx));
                }
            })
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
            .on_action(cx.listener(Self::show_environments))
            .on_action(cx.listener(Self::forget_environment))
            .on_action(cx.listener(Self::connect_typed))
            .on_action(cx.listener(Self::switch_environment))
            .on_action(cx.listener(Self::select_remote_project))
            .on_action(cx.listener(Self::open_remote_thread))
            .on_action(cx.listener(Self::new_remote_thread))
            .on_action(cx.listener(Self::trust_remote_node))
            .on_action(cx.listener(Self::toggle_linked))
            .on_action(cx.listener(Self::open_linked_thread))
    }

    pub(crate) fn is_remote(&self) -> bool {
        self.env.remote.is_some()
    }

    /// Drops the machine shown and every linked one, with their forwards, tabs and connections (a window closing:
    /// the workspace may outlive it a while, and nothing remote may).
    pub(super) fn leave_machines(&mut self, cx: &mut Context<Self>) {
        let linked = self.env.linked.drain(..).map(|l| l.env);
        for env in self.env.remote.take().into_iter().chain(linked) {
            env.update(cx, |env, _| env.drop_forwards());
        }
        self.env.tabs.clear();
        self.env.subs.clear();
        if let Some(picker) = &self.env.picker {
            picker.update(cx, |p, _| p.remote = None);
        }
    }

    pub(crate) fn remote_env(&self) -> Option<Entity<RemoteEnv>> {
        self.env.remote.clone()
    }

    pub(crate) fn env_picker(&self) -> Option<Entity<EnvPicker>> {
        self.env.picker.clone()
    }

    pub(crate) fn remote_tabs(&self) -> Vec<Entity<RemoteThreadView>> {
        self.env.tabs.iter().map(|(t, _)| t.clone()).collect()
    }

    fn toggle_environments(
        &mut self,
        _: &ToggleEnvironments,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_env_menu(None, window, cx);
    }

    fn show_environments(
        &mut self,
        a: &ShowEnvironments,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_env_menu(Some(gpui::point(px(a.x), px(a.y))), window, cx);
    }

    /// Toggles the menu, except under the Settings modal, which would cover it.
    fn open_env_menu(
        &mut self,
        at: Option<gpui::Point<gpui::Pixels>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.settings_open {
            return;
        }
        if let Some(picker) = &self.env.picker {
            picker.update(cx, |p, cx| p.toggle(at, window, cx));
        }
    }

    /// Closes the machine menu; true when it was open.
    pub(super) fn close_env_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let Some(picker) = self.env.picker.clone() else {
            return false;
        };
        let open = picker.read(cx).open;
        picker.update(cx, |p, cx| p.close(window, cx));
        open
    }

    /// The machine menu over everything, while it is open: a backdrop that closes it on a press, and the card with
    /// its bottom-left corner at the clicked chip (over the bottom of the window from the palette).
    pub(super) fn env_menu(&self, window: &Window, cx: &App) -> Option<impl IntoElement + use<>> {
        let picker = self.env.picker.clone()?;
        let p = picker.read(cx);
        if !p.open {
            return None;
        }
        let size = window.viewport_size();
        let at = p.anchor.map_or(
            gpui::point(size.width / 2. - px(MENU_W / 2.), size.height - px(72.)),
            |a| a - gpui::point(px(8.), px(10.)),
        );
        let weak = picker.downgrade();
        Some(
            gpui::deferred(
                div()
                    .id("env-menu-backdrop")
                    .absolute()
                    .inset_0()
                    .occlude()
                    .on_mouse_down(gpui::MouseButton::Left, move |_, window, cx| {
                        let _ = weak.update(cx, |p, cx| p.close(window, cx));
                    })
                    .child(
                        gpui::anchored()
                            .position(at)
                            .anchor(gpui::Anchor::BottomLeft)
                            .snap_to_window_with_margin(px(8.))
                            .child(
                                div().child(picker).with_animation(
                                    "env-menu-in",
                                    gpui::Animation::new(crate::ui::motion::FADE_FAST)
                                        .with_easing(crate::ui::motion::ease_out_quad),
                                    |el, t| el.opacity(t),
                                ),
                            ),
                    ),
            )
            .with_priority(2),
        )
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
                picker.update(cx, |p, cx| p.close(window, cx));
            }
            return;
        }
        // Leaving an environment closes all of its tabs and drops its client: nothing remote stays around.
        self.close_remote_tabs(window, cx);
        self.env.subs.clear();
        if let Some(env) = self.env.remote.take() {
            env.update(cx, |env, cx| {
                env.drop_forwards();
                cx.notify();
            });
        }
        let mut error = None;
        if let Some(host) = a.host.clone() {
            match self.dial(&host, cx) {
                Ok(env) => {
                    self.env.subs.push(cx.observe(&env, |this, _, cx| {
                        this.refresh_sidebar(cx);
                        cx.notify();
                    }));
                    self.env.subs.push(cx.subscribe_in(
                        &env,
                        window,
                        |this, _, e: &EnvEvent, window, cx| match e {
                            EnvEvent::Untrusted => this.close_remote_tabs(window, cx),
                            EnvEvent::Note(note) => crate::ui::toast::push(
                                window,
                                crate::ui::toast::Toast::new(note.to_string()),
                                cx,
                            ),
                            EnvEvent::Thread(_) => {}
                        },
                    ));
                    self.env.remote = Some(env);
                    self.remember(host, cx);
                }
                Err(e) => error = Some(e),
            }
        }
        let current = self.env.remote.as_ref().map(|e| e.read(cx).host.clone());
        if let Some(picker) = &self.env.picker {
            let remote = self.env.remote.clone();
            picker.update(cx, |p, cx| {
                (p.current, p.remote, p.error) = (current, remote, error);
                p.close(window, cx);
                cx.notify();
            });
        }
        self.sync_setup(cx); // setup is for this machine only
        let (remote, me) = (self.is_remote(), self.window_handle);
        let windows = &mut cx.default_global::<RemoteWindows>().0;
        if remote {
            windows.insert(me);
        } else {
            windows.remove(&me);
        }
        // Views are shared between windows: only the ones this window owns follow it.
        for view in &self.open_threads {
            view.update(cx, |v, cx| {
                if v.window == me {
                    v.remote_window = remote;
                    cx.notify();
                }
            });
        }
        self.refresh_sidebar(cx);
        cx.notify();
    }

    /// A connection to `host`, held until `RemoteEnv` checked its node (nothing is asked of it before).
    pub(super) fn dial(
        &self,
        host: &str,
        cx: &mut Context<Self>,
    ) -> Result<Entity<RemoteEnv>, SharedString> {
        let mut options = self.env.options.clone();
        options.approve_each_connection = true;
        let client = skyelab_client::Client::connect(host.to_owned(), options)
            .map_err(|e| SharedString::from(format!("can't start the connection: {e}")))?;
        let settings = self.env.settings_path.clone();
        Ok(cx.new(|cx| RemoteEnv::new(host.to_owned(), client, settings, cx)))
    }

    fn close_remote_tabs(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        for (tab, _) in std::mem::take(&mut self.env.tabs) {
            tab.update(cx, |t, cx| t.closed(cx));
        }
        if matches!(self.page, Page::RemoteThread(_)) {
            self.page = Page::Welcome;
            self.apply_page(window, cx);
        }
    }

    pub(super) fn trust_remote_node(
        &mut self,
        a: &TrustRemoteNode,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(env) = self.env.remote.clone() else {
            return;
        };
        env.update(cx, |e, cx| e.trust(&a.node_id, cx));
        // The same machine's linked connection was refused on that node too; it isn't asked again by itself.
        let host = env.read(cx).host.clone();
        for linked in self.linked_envs_of(&host, cx) {
            linked.update(cx, |e, cx| e.trust(&a.node_id, cx));
        }
    }

    /// Saves `host` first in the recents, off the UI thread. A file that doesn't parse is left alone.
    fn remember(&mut self, host: String, cx: &mut Context<Self>) {
        self.save_env_settings(move |s| s.remember_environment(&host), cx);
    }

    fn forget_environment(
        &mut self,
        a: &ForgetEnvironment,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        crate::environment::forget_cache(&a.host, cx);
        let host = a.host.clone();
        self.save_env_settings(move |s| s.forget_environment(&host), cx);
    }

    pub(super) fn select_remote_project(
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
        if let Some(env) = self.env.remote.clone() {
            self.open_thread_on(env, a.id, window, cx);
        }
    }

    /// Shows thread `id` of `env`: its open view, else a new one.
    pub(super) fn open_thread_on(
        &mut self,
        env: Entity<RemoteEnv>,
        id: i64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(tab) = self.remote_tabs().into_iter().find(|t| {
            let t = t.read(cx);
            t.thread_id() == Some(id) && *t.env() == env
        }) {
            return self.show(Page::RemoteThread(tab.downgrade()), window, cx);
        }
        let Some(thread) = env.read(cx).threads.iter().find(|t| t.id == id).cloned() else {
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
        let sub = cx.subscribe(&view, |_, view, e: &RemoteThreadEvent, cx| match e {
            RemoteThreadEvent::Started => {
                let env = view.read(cx).env().clone();
                env.update(cx, |e, cx| e.load_threads(cx));
            }
        });
        self.env.tabs.push((view.clone(), sub));
        self.show(Page::RemoteThread(view.downgrade()), window, cx);
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
                .on_click(|e, window, cx| {
                    crate::workspace::dispatch(
                        Box::new(ShowEnvironments::at(e.position())),
                        window,
                        cx,
                    )
                }),
        )
    }
}
