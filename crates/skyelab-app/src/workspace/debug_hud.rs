//! Debug mode's overlay (docs/debugging.md): frame time, the busiest views, live object counts, and the debug log.
//! It exists only while shown; its one-second tick re-renders the window (a notify dirties every ancestor), so the
//! overlay itself costs one frame a second.

use super::Workspace;
use crate::debug_mode;
use crate::ui::A11yScrollArea as _;
use crate::ui::{self, Button, SkyeIcon};
use gpui::{
    AppContext as _, Context, InteractiveElement as _, IntoElement, ParentElement as _, Render,
    SharedString, StatefulInteractiveElement as _, Styled as _, Task, Window, actions, div,
    prelude::FluentBuilder as _, px,
};
use gpui_component::{ActiveTheme as _, h_flex, v_flex};
use skyelab_core::debug;
use std::time::Duration;

actions!(skyelab, [ToggleDebugOverlay, OpenDebugLog, DumpDebugState]);

/// Rows of the log the overlay shows, newest last.
const LOG_ROWS: usize = 40;
/// Views listed under "Renders/s".
const TOP_VIEWS: usize = 6;

pub(crate) struct DebugHud {
    /// Showing the debug log instead of the counters.
    log: bool,
    /// This process's child processes at the last tick (read off the UI thread).
    children: Option<usize>,
    _tick: Task<()>,
}

impl DebugHud {
    pub(crate) fn new(log: bool, cx: &mut Context<Self>) -> Self {
        let tick = cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(Duration::from_secs(1)).await;
                let children = cx
                    .background_spawn(async { debug_mode::child_processes().len() })
                    .await;
                let shown = this.update(cx, |hud, cx| {
                    hud.children = Some(children);
                    cx.notify()
                });
                if shown.is_err() {
                    break;
                }
            }
        });
        Self {
            log,
            children: None,
            _tick: tick,
        }
    }
}

impl Workspace {
    /// Shows or hides the overlay; showing it turns debug mode on for this run.
    pub(super) fn toggle_debug_overlay(
        &mut self,
        _: &ToggleDebugOverlay,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.show_debug_hud(self.debug_hud.is_none(), false, window, cx);
    }

    /// Opens the overlay on the debug log.
    pub(super) fn open_debug_log(
        &mut self,
        _: &OpenDebugLog,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.show_debug_hud(true, true, window, cx);
    }

    pub(super) fn show_debug_hud(
        &mut self,
        show: bool,
        log: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if show {
            debug_mode::enable();
            match &self.debug_hud {
                Some(hud) => hud.update(cx, |hud, cx| {
                    hud.log = log;
                    cx.notify();
                }),
                None => self.debug_hud = Some(cx.new(|cx| DebugHud::new(log, cx))),
            }
        } else {
            self.debug_hud = None;
        }
        debug_mode::set_gpui_overlay(show, window);
        cx.notify();
    }

    /// Writes the JSON snapshot off the UI thread and says where it went.
    pub(super) fn dump_debug_state(
        &mut self,
        _: &DumpDebugState,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let task = cx.background_spawn(async { debug_mode::dump_state() });
        cx.spawn_in(window, async move |_, cx| {
            let note = match task.await {
                Ok(path) => {
                    eprintln!("skyelab: debug state written to {}", path.display());
                    ui::toast::Toast::success(format!("Debug state written to {}", path.display()))
                }
                Err(e) => ui::toast::Toast::error(format!("Debug state not written: {e:#}")),
            };
            cx.update(|window, cx| ui::toast::push(window, note, cx))
                .ok();
        })
        .detach();
    }

    /// The overlay over the bottom right of the window, above the status bar.
    pub(super) fn debug_overlay(&self) -> Option<impl IntoElement + use<>> {
        let hud = self.debug_hud.clone()?;
        Some(gpui::deferred(
            div()
                .absolute()
                .right(px(12.))
                .bottom(px(36.))
                .child(gpui::AnyView::from(hud)),
        ))
    }
}

impl Render for DebugHud {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let _render = crate::debug_renders::count("DebugHud");
        let (stats_tab, log_tab) = (
            tab("debug-hud-stats", "Stats", !self.log, cx),
            tab("debug-hud-log", "Log", self.log, cx),
        );
        let theme = cx.theme();
        let (muted, danger) = (theme.muted_foreground, theme.danger);
        let header = h_flex()
            .gap_1()
            .child(
                div()
                    .flex_1()
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .child("Debug"),
            )
            .child(stats_tab)
            .child(log_tab)
            .child(
                Button::new("debug-hud-dump")
                    .ghost()
                    .xsmall()
                    .label("Dump")
                    .tooltip("Write a JSON snapshot for a bug report")
                    .on_click(|_, window, cx| {
                        crate::workspace::dispatch(Box::new(DumpDebugState), window, cx)
                    }),
            )
            .child(
                Button::new("debug-hud-close")
                    .ghost()
                    .xsmall()
                    .icon(SkyeIcon::Close)
                    .tooltip("Hide the overlay")
                    .on_click(|_, window, cx| {
                        crate::workspace::dispatch(Box::new(ToggleDebugOverlay), window, cx)
                    }),
            );
        let body = match self.log {
            true => log_rows(muted, danger).into_any_element(),
            false => stats(self.children, muted).into_any_element(),
        };
        v_flex()
            .id("debug-hud")
            .debug_selector(|| "debug-hud".into())
            .occlude()
            .w(px(if self.log { 560. } else { 300. }))
            .max_h(px(420.))
            .overflow_y_scroll()
            .a11y_scroll_area()
            .gap_2()
            .p_2p5()
            .rounded(ui::RADIUS_LG)
            .border_1()
            .border_color(theme.border)
            .bg(theme.popover)
            .shadow_lg()
            .text_size(ui::META)
            .text_color(theme.popover_foreground)
            .child(header)
            .child(body.into_any_element())
    }
}

fn tab(id: &'static str, label: &'static str, on: bool, cx: &mut Context<DebugHud>) -> Button {
    let log = label == "Log";
    Button::new(id)
        .xsmall()
        .label(label)
        .map(|b| if on { b.primary() } else { b.ghost() })
        .on_click(cx.listener(move |hud, _, _, cx| {
            hud.log = log;
            cx.notify();
        }))
}

fn caption(text: &'static str, muted: gpui::Hsla) -> impl IntoElement {
    div().pt_1().text_color(muted).child(text)
}

fn row(label: impl Into<SharedString>, value: impl Into<SharedString>) -> impl IntoElement {
    h_flex()
        .justify_between()
        .gap_3()
        .child(div().min_w_0().truncate().child(label.into()))
        .child(div().flex_none().child(value.into()))
}

fn ms(d: Duration) -> String {
    format!("{:.1} ms", d.as_secs_f64() * 1000.)
}

fn stats(children: Option<usize>, muted: gpui::Hsla) -> impl IntoElement {
    let frames = debug_mode::frame_stats();
    let renders = crate::debug_renders::last_second();
    let live = debug::live_counts();
    v_flex()
        .gap_0p5()
        // Developer mode was turned off with the overlay open: what it shows has stopped.
        .when(!debug::enabled(), |el| {
            el.child(row(
                "Debug mode is off",
                "hide and show the overlay to turn it on",
            ))
        })
        .child(caption("Frames (root render to paint)", muted))
        .child(row("Frames/s (5 s average)", format!("{:.1}", frames.fps)))
        .child(row("Last frame", ms(frames.last)))
        .child(row("Worst frame (5 s)", ms(frames.worst)))
        .child(caption("Renders/s, busiest views", muted))
        .when(renders.is_empty(), |el| {
            el.child(row("Nothing rendered", ""))
        })
        .children(
            renders
                .into_iter()
                .take(TOP_VIEWS)
                .map(|(view, n)| row(view, n.to_string())),
        )
        .child(caption("Live objects", muted))
        .children(live.into_iter().map(|(kind, n)| row(kind, n.to_string())))
        .children(children.map(|n| row("child processes", n.to_string())))
}

fn log_rows(muted: gpui::Hsla, danger: gpui::Hsla) -> impl IntoElement {
    let entries = debug::recent(LOG_ROWS);
    v_flex()
        .gap_0p5()
        .when(entries.is_empty(), |el| {
            el.child(div().text_color(muted).child(
                "Nothing logged yet: RPC, child processes, tailnet and MCP calls show here.",
            ))
        })
        .children(entries.into_iter().map(move |e| {
            let took = e.ms.map(|m| format!("{m} ms")).unwrap_or_default();
            h_flex()
                .gap_2()
                .child(
                    div()
                        .w(px(56.))
                        .flex_none()
                        .text_color(muted)
                        .child(e.kind.label()),
                )
                .child(div().flex_1().min_w_0().truncate().child(e.what))
                .child(
                    div()
                        .flex_none()
                        .max_w(px(160.))
                        .truncate()
                        .when(!e.ok, |d| d.text_color(danger))
                        .when(e.ok, |d| d.text_color(muted))
                        .child(e.detail),
                )
                .child(div().w(px(56.)).flex_none().text_color(muted).child(took))
        }))
}

#[cfg(test)]
#[path = "debug_hud_test.rs"]
mod tests;
