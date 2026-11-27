//! Settings → Machines → Waiting for approval: each linked machine that hasn't allowed this device yet, with this
//! device's code, so the user can compare it with the code that machine's Sharing & access shows before allowing it.

use super::Workspace;
use crate::environment::ConnState;
use crate::ui;
use crate::ui::Theme;
use gpui::{AnyElement, App, Context, SharedString, div, prelude::*};
use gpui_component::v_flex;

impl Workspace {
    /// (machine, this device's code) for every linked machine waiting to approve this device.
    pub(super) fn waiting_rows(&self, cx: &App) -> Vec<(String, String)> {
        self.env
            .linked
            .iter()
            .filter_map(|l| {
                let e = l.env.read(cx);
                matches!(e.state, ConnState::NotApproved(_))
                    .then(|| Some((e.host.clone(), e.client().device_code()?)))
                    .flatten()
            })
            .collect()
    }

    /// Hands the Machines page the machines waiting now; it redraws only when they changed.
    pub(super) fn push_waiting(&self, cx: &mut Context<Self>) {
        let rows = self.waiting_rows(cx);
        if let Some(page) = self
            .settings
            .as_ref()
            .and_then(|s| s.read(cx).machines.clone())
        {
            page.update(cx, |p, cx| p.set_waiting(rows, cx));
        }
    }
}

/// The section, or nothing while no machine waits.
pub(super) fn section(rows: &[(String, String)], theme: &Theme) -> Option<AnyElement> {
    if rows.is_empty() {
        return None;
    }
    let rows = rows.iter().enumerate().map(|(n, (host, code))| {
        ui::group_row(n == 0, theme)
            .id(SharedString::from(format!("waiting-{host}")))
            .debug_selector(|| format!("waiting-{host}"))
            .child(ui::StatusDot::new(theme.warning))
            .child(div().flex_1().min_w_0().truncate().child(host.clone()))
            .child(ui::mono(format!("This device's code {code}"), theme))
    });
    Some(
        v_flex()
            .gap_2p5()
            .child(ui::subsection_title("Waiting for approval", theme))
            .child(
                div().px_1().text_size(ui::META).text_color(theme.muted_foreground).child(
                    "Allow this device in Settings → Sharing & access on each machine, only if it shows the same \
                     code.",
                ),
            )
            .child(ui::group(theme).border_color(theme.warning.opacity(0.4)).children(rows))
            .into_any_element(),
    )
}
