//! The details card's rows for threads this one started on other machines: "on <machine>", opened in place through
//! that linked machine.

use super::super::delegated_label::short_machine;
use super::super::env_linked::OpenLinkedThread;
use super::super::thread::ThreadView;
use super::On;
use super::card::row;
use crate::ui;
use crate::ui::SkyeIcon;
use gpui::{AnyElement, App, SharedString, div, prelude::*};
use gpui_component::ActiveTheme as _;

impl ThreadView {
    pub(super) fn remote_rows(&self, on: &On, cx: &App) -> Vec<AnyElement> {
        let theme = cx.theme();
        self.details
            .remote
            .iter()
            .map(|c| {
                let (host, id) = (c.machine.clone(), c.thread_id);
                let title: SharedString = match c.title.trim() {
                    "" => format!("Thread {id}").into(),
                    t => t.to_owned().into(),
                };
                row(
                    SharedString::from(format!("details-remote-{}-{id}", c.node_id)),
                    SkyeIcon::Bot,
                    title.clone(),
                    theme,
                )
                .role(gpui::accesskit::Role::Button)
                .aria_label(format!("{title} {}", remote_label(&c.machine)))
                .child(
                    div()
                        .flex_shrink_0()
                        .text_size(ui::META)
                        .text_color(theme.muted_foreground)
                        .child(remote_label(&c.machine)),
                )
                .on_click(on.click(move |this, window, cx| {
                    this.details_home(cx);
                    let open = OpenLinkedThread {
                        host: host.clone(),
                        id,
                    };
                    crate::workspace::dispatch(Box::new(open), window, cx);
                }))
                .into_any_element()
            })
            .collect()
    }
}

/// Where a remote child runs, as its row says.
pub(in crate::workspace) fn remote_label(machine: &str) -> SharedString {
    format!("on {}", short_machine(machine)).into()
}
