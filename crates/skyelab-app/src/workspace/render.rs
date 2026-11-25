//! How the main window draws: the dock and the status bar, with every workspace action wired.

use super::*;

impl Render for Workspace {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let _render = crate::debug_renders::count("Workspace");
        // Debug mode's frame timing: from here to the probe's paint (docs/debugging.md).
        let frame_start = skyelab_core::debug::enabled().then(std::time::Instant::now);
        let ring = self.render_ring(window, cx);
        let settings = self.settings_modal(window, cx);
        let theme = cx.theme();
        v_flex()
            .id("workspace")
            .key_context(CONTEXT)
            .track_focus(&self.focus)
            .on_action(cx.listener(Self::toggle_sidebar))
            .on_action(cx.listener(Self::open_home))
            .on_action(cx.listener(Self::open_bots))
            .on_action(cx.listener(Self::open_compare))
            .on_action(cx.listener(Self::open_pr_diff))
            .on_action(cx.listener(Self::toggle_compare_group))
            .on_action(cx.listener(Self::keep_compare))
            .on_action(cx.listener(Self::toggle_bottom))
            .on_action(cx.listener(Self::toggle_right))
            .on_action(cx.listener(Self::toggle_details))
            .on_action(cx.listener(Self::open_in_editor_with))
            .on_action(cx.listener(Self::new_terminal))
            .on_action(cx.listener(Self::split_terminal))
            .on_action(cx.listener(Self::split_terminal_down))
            .on_action(cx.listener(Self::close_terminal))
            .on_action(cx.listener(Self::on_run_project_script))
            .on_action(cx.listener(Self::open_settings))
            .when(
                cfg!(debug_assertions) || crate::debug_mode::developer_mode(cx),
                |d| d.on_action(crate::ui::gallery::on_open),
            )
            .on_action(cx.listener(Self::new_window))
            .on_action(|_: &crate::updates::CheckForUpdates, _, cx| crate::updates::check(true, cx))
            .on_action(cx.listener(Self::open_usage))
            .on_action(cx.listener(Self::schedule_text))
            .on_action(cx.listener(Self::open_pull_requests))
            .on_action(cx.listener(Self::open_issues))
            .on_action(cx.listener(Self::open_onboarding))
            .on_action(cx.listener(Self::open_browser))
            .on_action(cx.listener(Self::reopen_closed_view))
            .on_action(cx.listener(Self::pick_project))
            .on_action(cx.listener(Self::select_project))
            .on_action(cx.listener(Self::new_thread))
            .on_action(cx.listener(Self::new_thread_in))
            .on_action(cx.listener(Self::import_sessions))
            .on_action(cx.listener(Self::toggle_project_scope))
            .on_action(cx.listener(Self::save_sidebar_view))
            .on_action(cx.listener(Self::apply_sidebar_view))
            .on_action(cx.listener(Self::delete_sidebar_view))
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
            .on_action(cx.listener(Self::log_in_account))
            .on_action(cx.listener(Self::focus_next_area))
            .on_action(cx.listener(Self::focus_previous_area))
            .on_action(cx.listener(Self::focus_composer))
            .on_action(cx.listener(Self::toggle_debug_overlay))
            .on_action(cx.listener(Self::open_debug_log))
            .on_action(cx.listener(Self::dump_debug_state))
            .capture_any_mouse_down(cx.listener(|this, _, _, cx| this.clear_ring(cx)))
            .map(|el| self.env_actions(el, cx))
            .map(|el| self.manage_actions(el, cx))
            .map(|el| self.shelf_actions(el, cx))
            .map(|el| self.attention_actions(el, cx))
            .map(|el| self.file_actions(el, cx))
            .map(|el| self.scratch_actions(el, cx))
            .map(|el| self.surface_actions(el, cx))
            .size_full()
            .bg(theme.background)
            .text_color(theme.foreground)
            .child(
                gpui_component::h_flex()
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .child(self.render_rail(cx))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .h_full()
                            .relative()
                            .on_drag_move(cx.listener(Self::restore_drag_move))
                            .child(self.dock.clone())
                            .children(self.sidebar_restore_edge(cx))
                            .children(self.details_card(cx))
                            .children(ring)
                            // Windows: minimize, maximize and close over the top row's end (no separate title strip).
                            .child(crate::ui::WindowControls),
                    ),
            )
            .child(self.render_status(cx))
            .children(self.env_menu(window, cx))
            .children(settings)
            .children(self.debug_overlay())
            .children(crate::ui::toast::layer(window, cx))
            .children(frame_start.map(crate::debug_mode::frame_probe))
    }
}

/// The floating card's width (T3's details card).
const CARD_W: f32 = 320.;
/// Its gap from the page header's toggles and the chat area's right edge.
const CARD_GAP: f32 = 12.;

impl Workspace {
    /// How much of the window an open dock takes (its width, or height for the bottom one); 0 when closed.
    pub(super) fn dock_width(&self, p: DockPlacement, cx: &App) -> gpui::Pixels {
        let dock = self.dock.read(cx);
        dock.is_dock_open(p)
            .then(|| dock.dock_size(p))
            .flatten()
            .unwrap_or_default()
    }

    /// T3's popover mode: the card would cover the chat column, so a click outside it closes it. Beside the column
    /// it stays open while you work.
    pub(super) fn card_covers_chat(&self, window: &Window, cx: &App) -> bool {
        let center = window.viewport_size().width
            - crate::ui::RAIL_W
            - self.dock_width(DockPlacement::Left, cx)
            - self.dock_width(DockPlacement::Right, cx);
        (center - crate::ui::CHAT_MAX_W) / 2. < px(CARD_W + CARD_GAP)
    }

    /// The card asks to close on a press outside it: only in popover mode, and not on the page header, whose toggle
    /// closes it on its own click.
    pub(super) fn details_outside(
        &mut self,
        at: gpui::Point<gpui::Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.details_open && at.y > crate::ui::HEADER_H && self.card_covers_chat(window, cx) {
            self.close_details(window, cx);
        }
    }

    /// The thread details card, floating over the chat at the top right under the page header's toggles, left of the
    /// right dock when it is open. A deferred layer, so it paints over the dock's content. The card is the
    /// `DetailsPanel` entity drawn as a cached view: a streamed delta that redraws the thread doesn't redraw it.
    fn details_card(&self, cx: &App) -> Option<impl IntoElement + use<>> {
        if !self.details_open {
            return None;
        }
        let right = self.dock_width(DockPlacement::Right, cx);
        Some(gpui::deferred(
            div()
                .absolute()
                .top(crate::ui::HEADER_H + px(4.))
                .bottom(px(CARD_GAP))
                .right(right + px(CARD_GAP))
                .w(px(CARD_W))
                .child(
                    gpui::AnyView::from(self.details.clone())
                        .cached(gpui::StyleRefinement::default().size_full()),
                ),
        ))
    }
}
