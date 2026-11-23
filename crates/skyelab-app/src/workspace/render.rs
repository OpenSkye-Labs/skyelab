//! How the main window draws: the dock and the status bar, with every workspace action wired.

use super::*;

impl Render for Workspace {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        crate::debug_renders::count("Workspace");
        let ring = self.render_ring(window, cx);
        let theme = cx.theme();
        v_flex()
            .id("workspace")
            .key_context(CONTEXT)
            .track_focus(&self.focus)
            .on_action(cx.listener(Self::toggle_sidebar))
            .on_action(cx.listener(Self::toggle_bottom))
            .on_action(cx.listener(Self::toggle_right))
            .on_action(cx.listener(Self::open_in_editor_with))
            .on_action(cx.listener(Self::new_terminal))
            .on_action(cx.listener(Self::split_terminal))
            .on_action(cx.listener(Self::on_run_project_script))
            .on_action(cx.listener(Self::open_settings))
            .on_action(cx.listener(Self::new_window))
            .on_action(|_: &crate::updates::CheckForUpdates, _, cx| crate::updates::check(true, cx))
            .on_action(cx.listener(Self::open_usage))
            .on_action(cx.listener(Self::set_up_accounts))
            .on_action(cx.listener(Self::open_browser))
            .on_action(cx.listener(Self::pick_project))
            .on_action(cx.listener(Self::select_project))
            .on_action(cx.listener(Self::new_thread))
            .on_action(cx.listener(Self::new_thread_in))
            .on_action(cx.listener(Self::import_sessions))
            .on_action(cx.listener(Self::toggle_project_scope))
            .on_action(cx.listener(Self::show_all_projects))
            .on_action(cx.listener(Self::open_thread))
            .on_action(cx.listener(Self::open_devices))
            .on_action(cx.listener(Self::open_remote))
            .on_action(cx.listener(Self::toggle_sharing))
            .on_action(cx.listener(Self::disconnect_peer))
            .on_action(cx.listener(Self::allow_device))
            .on_action(cx.listener(Self::deny_device))
            .on_action(cx.listener(Self::revoke_device))
            .on_action(cx.listener(Self::open_palette))
            .on_action(cx.listener(Self::go_back))
            .on_action(cx.listener(Self::go_forward))
            .on_action(cx.listener(Self::open_settings_section))
            .on_action(cx.listener(Self::focus_next_area))
            .on_action(cx.listener(Self::focus_previous_area))
            .on_action(cx.listener(Self::focus_composer))
            .capture_any_mouse_down(cx.listener(|this, _, _, cx| this.clear_ring(cx)))
            .map(|el| self.env_actions(el, cx))
            .map(|el| self.manage_actions(el, cx))
            .map(|el| self.attention_actions(el, cx))
            .map(|el| self.file_actions(el, cx))
            .map(|el| self.scratch_actions(el, cx))
            .size_full()
            .bg(theme.background)
            .text_color(theme.foreground)
            // Windows: the titlebar is transparent, so draw the window controls in theme colors.
            .when(cfg!(target_os = "windows"), |el| {
                el.child(gpui_component::TitleBar::new().child("SkyeLab"))
            })
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .relative()
                    .child(self.dock.clone())
                    .children(ring),
            )
            .child(self.render_status(cx))
    }
}
