//! Settings → Machines → Delegation: "Let my agents delegate here" for each linked machine, off by default. On, this
//! machine's agents may list projects on, start threads on and drive the threads they started on that machine; it
//! still checks everything itself (its own link approval, logins, bindings and caps).

use super::Workspace;
use crate::ui;
use crate::ui::Switch;
use gpui::{Action, AnyElement, Context, SharedString, Window, div, prelude::*};
use gpui_component::{Theme, v_flex};
use skyelab_core::AppSettings;

/// Turns delegation to `host` on or off: `on` is what the click asks for, from the switch as drawn.
#[derive(Clone, PartialEq, Action)]
#[action(namespace = skyelab, no_json)]
pub struct ToggleDelegationTarget {
    pub host: String,
    pub on: bool,
}

/// The linked machines and whether this machine's agents may delegate to each, as saved now.
pub(super) fn rows(settings: Option<&AppSettings>, linked: &[String]) -> Vec<(String, bool)> {
    linked
        .iter()
        .map(|h| {
            (
                h.clone(),
                settings.is_some_and(|s| s.is_delegation_target(h)),
            )
        })
        .collect()
}

/// The section, `None` without a linked machine.
pub(super) fn section(rows: &[(String, bool)], theme: &Theme) -> Option<AnyElement> {
    if rows.is_empty() {
        return None;
    }
    let items = rows.iter().enumerate().map(|(n, (host, on))| {
        let toggle = ToggleDelegationTarget {
            host: host.clone(),
            on: !*on,
        };
        ui::group_row(n == 0, theme)
            .id(SharedString::from(format!("delegate-row-{host}")))
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .gap_0p5()
                    .child(div().truncate().child(host.clone()))
                    .child(
                        div()
                            .text_size(ui::META)
                            .text_color(theme.muted_foreground)
                            .child("Let my agents delegate here"),
                    ),
            )
            .child(
                Switch::new(SharedString::from(format!("delegate-{host}")))
                    .checked(*on)
                    .accessibility_label(format!("Let my agents delegate to {host}"))
                    .on_click(move |_, window, cx| {
                        crate::workspace::dispatch(Box::new(toggle.clone()), window, cx)
                    }),
            )
            .into_any_element()
    });
    Some(
        v_flex()
            .gap_2p5()
            .child(ui::subsection_title("Delegation", theme))
            .child(
                div()
                    .px_1()
                    .text_size(ui::META)
                    .text_color(theme.muted_foreground)
                    .child(
                        "Agents here may start and drive threads on a linked machine, under that machine's own \
                         account for each folder. It must allow this machine and account too, once, in its \
                         Sharing & access.",
                    ),
            )
            .child(ui::group(theme).children(items))
            .into_any_element(),
    )
}

impl Workspace {
    /// The Delegation rows for Settings → Machines, from settings.toml as saved now.
    pub(super) fn delegation_rows(&self) -> Vec<(String, bool)> {
        let settings = self
            .env
            .settings_path
            .as_deref()
            .and_then(|p| AppSettings::load(p).ok());
        rows(settings.as_ref(), &self.env.linked_hosts)
    }

    pub(super) fn toggle_delegation_target(
        &mut self,
        a: &ToggleDelegationTarget,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(path) = self.env.settings_path.clone() else {
            return;
        };
        // A file that can't be read changes nothing: the switch stays where it was.
        let saved = AppSettings::update(&path, |s| s.set_delegation_target(&a.host, a.on));
        if let Err(e) = saved {
            eprintln!("skyelab: can't save the delegation switch: {e:#}");
        }
        let rows = self.delegation_rows();
        if let Some(page) = self
            .settings
            .as_ref()
            .and_then(|s| s.read(cx).machines.clone())
        {
            page.update(cx, |p, cx| p.set_delegation(rows, cx));
        }
    }
}
