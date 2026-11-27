//! The sidebar's update pill (T3's `SidebarUpdateCapsule`): a 32 px rounded pill in the footer's control row with a
//! muted label ("Update available", "Downloading… 42%", "Ready to restart", "Update failed") and, after a failure, an
//! underlined Retry. Under [`COLLAPSE_BELOW`] of room it is an icon-only circle. A click opens a popover above it
//! with the details and every action (Download and install, Skip this version, Restart to update, Show in folder,
//! Try again, Release page). Shown only while `updates` has one.

use crate::ui;
use crate::ui::ActiveTheme as _;
use crate::ui::Button;
use crate::ui::SkyeIcon;
use crate::updates::{Step, UpdateStatus, Updates};
use gpui::Anchor;
use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, App, EntityId, InteractiveElement as _, IntoElement, ParentElement as _, Pixels,
    RenderOnce, StatefulInteractiveElement as _, Styled as _, Window, div, px, relative,
};
use gpui_component::popover::Popover;
use gpui_component::{Icon, Selectable, h_flex, v_flex};
use std::cell::Cell;
use std::rc::Rc;

/// The pill's height, and its width when collapsed.
const SIZE: Pixels = px(32.);
/// Less room than this in the footer row and the pill is an icon-only circle (T3's 5rem).
pub(super) const COLLAPSE_BELOW: Pixels = px(80.);
/// The popover's width: fits Download and install beside Skip this version.
const POPOVER_W: Pixels = px(280.);

/// The pill's inline action.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Inline {
    Retry,
}

/// What the pill says for an update `status`; `None` when there's nothing to show. `folder`: the staged file is a
/// package the user installs (Show in folder, not Restart).
#[derive(Debug, PartialEq)]
pub(super) struct Look {
    pub label: String,
    /// What shows when `label` doesn't fit.
    pub short: String,
    pub icon: SkyeIcon,
    pub failed: bool,
    pub inline: Option<Inline>,
    /// The pill's tooltip: the popover's headline, with the version.
    pub tip: String,
}

pub(super) fn look(status: &UpdateStatus, folder: bool) -> Option<Look> {
    let (tag, step) = match status {
        UpdateStatus::Available { tag, .. } => (tag, None),
        UpdateStatus::Updating { tag, step, .. } => (tag, Some(step)),
        _ => return None,
    };
    let tip = headline(tag, step);
    let plain = |label: String, short: String, icon| Look {
        label,
        short,
        icon,
        failed: false,
        inline: None,
        tip: tip.clone(),
    };
    Some(match step {
        None if skyelab_core::update::is_nightly(tag) => plain(
            "Nightly available".into(),
            "Nightly".into(),
            SkyeIcon::ArrowDown,
        ),
        None => plain(
            "Update available".into(),
            "Update".into(),
            SkyeIcon::ArrowDown,
        ),
        Some(Step::Downloading { done, total }) => match total {
            Some(t) if done >= t => {
                plain("Checking…".into(), "Checking".into(), SkyeIcon::ArrowDown)
            }
            Some(t) => {
                let p = percent(*done, *t);
                plain(
                    format!("Downloading… {p}%"),
                    format!("{p}%"),
                    SkyeIcon::ArrowDown,
                )
            }
            None => plain("Downloading…".into(), String::new(), SkyeIcon::ArrowDown),
        },
        Some(Step::Ready) if folder => plain(
            "Ready to install".into(),
            "Install".into(),
            SkyeIcon::CircleCheck,
        ),
        Some(Step::Ready) => plain(
            "Ready to restart".into(),
            "Restart".into(),
            SkyeIcon::CircleCheck,
        ),
        Some(Step::Deferred) => plain(
            "Updating when idle".into(),
            "When idle".into(),
            SkyeIcon::CircleCheck,
        ),
        Some(Step::Failed(_)) => Look {
            label: "Update failed".into(),
            short: "Failed".into(),
            icon: SkyeIcon::CircleAlert,
            failed: true,
            inline: Some(Inline::Retry),
            tip,
        },
    })
}

fn percent(done: u64, total: u64) -> u32 {
    (done.min(total) * 100 / total.max(1)) as u32
}

/// Whether the pill shows its label in `room` px of the footer row.
pub(super) fn expanded(room: Pixels) -> bool {
    room >= COLLAPSE_BELOW
}

/// What the open pill shows in `room`, given the widths of its full and short labels and of its inline action.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct Fit {
    pub full: bool,
    pub inline: bool,
    pub icon: bool,
}

/// Gives up the trailing icon first, then the full label, then the inline action (the popover has every action).
pub(super) fn fit(full: Pixels, short: Pixels, action: Option<Pixels>, room: Pixels) -> Fit {
    // pl_3 and pr_3; the icon's 32 px column replaces the pr_3, the action adds its pl_1p5.
    let pad = px(24.);
    let icon = SIZE - px(12.);
    let tries = |w: Pixels, full: bool| {
        let mut t = vec![];
        match action {
            Some(a) => t.push((
                w + a + px(6.),
                Fit {
                    full,
                    inline: true,
                    icon: false,
                },
            )),
            None => {
                t.push((
                    w + icon,
                    Fit {
                        full,
                        inline: false,
                        icon: true,
                    },
                ));
                t.push((
                    w,
                    Fit {
                        full,
                        inline: false,
                        icon: false,
                    },
                ));
            }
        }
        t
    };
    tries(full, true)
        .into_iter()
        .chain(tries(short, false))
        .chain([(
            short,
            Fit {
                full: false,
                inline: false,
                icon: false,
            },
        )])
        .find(|(w, _)| *w + pad <= room)
        .map_or(
            Fit {
                full: false,
                inline: false,
                icon: false,
            },
            |(_, f)| f,
        )
}

/// The pill's slot in the footer row (it takes the row's free space), or `None` without an update. `view` is the
/// sidebar, redrawn when the slot's width crosses [`COLLAPSE_BELOW`].
pub(super) fn pill(view: EntityId, window: &mut Window, cx: &mut App) -> Option<AnyElement> {
    let look = look(
        &cx.try_global::<Updates>()?.status,
        crate::updates::staged_is_folder(cx),
    )?;
    let room = window
        .use_keyed_state("update-pill-room", cx, |_, _| Rc::new(Cell::new(px(0.))))
        .read(cx)
        .clone();
    let room_w = room.get();
    let open = expanded(room_w);
    // 0 collapsed, 100 open: the slot's width moves on drag at once, only the collapse and expand spring.
    let t = gpui_base::spring(
        "update-pill-open",
        px(if open { 100. } else { 0. }),
        ui::motion::SPRING_PX,
        window,
        cx,
    ) / px(100.);
    let width = SIZE + (room_w - SIZE).max(px(0.)) * t;
    let measure = gpui::canvas(
        move |bounds, _, cx| {
            let old = room.replace(bounds.size.width);
            if expanded(old) != expanded(bounds.size.width) {
                // After this frame: a notify sent while the window draws can be dropped.
                cx.defer(move |cx| cx.notify(view));
            }
        },
        |_, _, _, _| {},
    )
    .absolute()
    .size_full();
    let trigger = PillTrigger {
        look,
        open,
        t,
        width,
        room: room_w,
        menu_open: false,
    };
    Some(
        h_flex()
            .relative()
            .flex_1()
            .min_w(SIZE)
            .justify_end()
            .child(measure)
            .child(
                Popover::new("update-pill-popover")
                    .anchor(Anchor::BottomLeft)
                    .offset(px(6.))
                    .trigger(trigger)
                    .content(|_, _, cx| details(cx)),
            )
            .into_any_element(),
    )
}

/// The pill itself, the popover's trigger.
#[derive(IntoElement)]
struct PillTrigger {
    look: Look,
    /// The label shows (the slot is wide enough).
    open: bool,
    /// The collapse spring, 0..=1: the label fades with it.
    t: f32,
    width: Pixels,
    /// The slot's width: what the open pill grows to.
    room: Pixels,
    menu_open: bool,
}

impl Selectable for PillTrigger {
    fn selected(mut self, selected: bool) -> Self {
        self.menu_open = selected;
        self
    }

    fn is_selected(&self) -> bool {
        self.menu_open
    }
}

impl RenderOnce for PillTrigger {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let Look {
            label,
            short,
            icon,
            failed,
            inline,
            tip,
        } = self.look;
        let size = ui::META.to_pixels(window.rem_size());
        let style = window.text_style();
        // +1 px: the shaped width can round under what layout gives the text.
        let measure = |text: &str| {
            window
                .text_system()
                .shape_line(
                    text.to_owned().into(),
                    size,
                    &[style.to_run(text.len())],
                    None,
                )
                .width
                + px(1.)
        };
        let fit = match self.open {
            true => fit(
                measure(&label),
                measure(&short),
                inline.map(|_| measure("Retry")),
                self.room,
            ),
            false => Fit {
                full: false,
                inline: false,
                icon: true,
            },
        };
        let text = if fit.full { label.clone() } else { short };
        let show_inline = fit.inline;
        let theme = cx.theme();
        div()
            .id("update-pill")
            .flex()
            .items_center()
            .h(SIZE)
            .w(self.width)
            .flex_shrink_0()
            .overflow_hidden()
            .rounded_full()
            .cursor_pointer()
            .text_size(ui::META)
            // Full-strength ink and an accent icon: muted grey on the faint wash read as disabled.
            .text_color(theme.sidebar_foreground)
            // A wash of the sidebar's own ink: `muted` matches some light themes' sidebar exactly.
            .bg(theme.sidebar_foreground.opacity(0.06))
            .when(self.menu_open, |d| {
                d.bg(theme.sidebar_foreground.opacity(0.1))
            })
            .hover(|s| s.bg(theme.sidebar_foreground.opacity(0.1)))
            // T3's faint destructive ring, not a filled block.
            .when(failed, |d| {
                d.border_1().border_color(theme.danger.opacity(0.4))
            })
            .tooltip(ui::Tooltip::text(tip))
            .when(self.open, |d| {
                d.child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .pl_3()
                        .when(!show_inline && !fit.icon, |d| d.pr_3())
                        .truncate()
                        .opacity(self.t)
                        .when(failed, |d| d.text_color(ui::ink(theme, theme.danger)))
                        .child(text),
                )
            })
            .when_some(inline.filter(|_| show_inline), |d, inline| {
                d.child(
                    div()
                        .id("update-pill-action")
                        .role(gpui::accesskit::Role::Button)
                        .aria_label("Retry update")
                        .flex_shrink_0()
                        .pl_1p5()
                        .pr_3()
                        .opacity(self.t)
                        .underline()
                        .child(match inline {
                            Inline::Retry => "Retry",
                        })
                        // Its own click: the pill's opens the popover.
                        .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .on_click(|_, _, cx| crate::updates::install(cx)),
                )
            })
            .when(fit.icon, |d| {
                d.child(
                    div()
                        .flex_shrink_0()
                        .size(SIZE)
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(Icon::new(icon).size_3p5().text_color(if failed {
                            ui::ink(theme, theme.danger)
                        } else {
                            ui::ink(theme, theme.primary)
                        })),
                )
            })
    }
}

/// The popover: what's happening and every action that applies.
fn details(cx: &App) -> AnyElement {
    let Some(updates) = cx.try_global::<Updates>() else {
        return div().into_any_element();
    };
    let (tag, step) = match &updates.status {
        UpdateStatus::Available { tag, .. } => (tag, None),
        UpdateStatus::Updating { tag, step, .. } => (tag, Some(step)),
        // The update went away while open (skipped, installed): nothing to say.
        _ => return div().into_any_element(),
    };
    let theme = cx.theme();
    let title = headline(tag, step);
    let progress = match step {
        Some(Step::Downloading {
            done,
            total: Some(t),
        }) => Some(percent(*done, *t)),
        _ => None,
    };
    let button = |id: &'static str, label: &'static str, primary: bool| {
        let b = Button::new(id).xsmall().label(label);
        if primary { b.primary() } else { b.ghost() }
    };
    let actions: Vec<AnyElement> = match step {
        None => vec![
            if crate::updates::can_install(cx) {
                button("update-install", "Download and install", true)
                    .on_click(|_, _, cx| crate::updates::install(cx))
            } else {
                button("update-download", "Download", true)
                    .on_click(|_, _, cx| crate::updates::download(cx))
            }
            .into_any_element(),
            button("update-skip", "Skip this version", false)
                .on_click(|_, _, cx| crate::updates::skip(cx))
                .into_any_element(),
        ],
        Some(Step::Downloading { .. }) => vec![],
        Some(Step::Ready) => vec![
            button(
                "update-restart",
                if crate::updates::staged_is_folder(cx) {
                    "Show in folder"
                } else {
                    "Restart to update"
                },
                true,
            )
            .on_click(|_, _, cx| crate::updates::restart(cx))
            .into_any_element(),
        ],
        Some(Step::Deferred) => vec![
            button("update-restart-now", "Restart now", true)
                .on_click(|_, _, cx| crate::updates::restart_now(cx))
                .into_any_element(),
            button("update-cancel-defer", "Cancel", false)
                .on_click(|_, _, cx| crate::updates::cancel_deferred(cx))
                .into_any_element(),
        ],
        Some(Step::Failed(_)) => vec![
            button("update-retry", "Try again", true)
                .on_click(|_, _, cx| crate::updates::install(cx))
                .into_any_element(),
            button("update-page", "Release page", false)
                .on_click(|_, _, cx| crate::updates::download(cx))
                .into_any_element(),
        ],
    };
    v_flex()
        .w(POPOVER_W)
        .gap_2()
        .child(
            h_flex()
                .gap_2()
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_size(ui::BODY)
                        .font_weight(gpui::FontWeight::MEDIUM)
                        .child(title),
                )
                .children(progress.map(|p| {
                    div()
                        .text_size(ui::META)
                        .text_color(theme.muted_foreground)
                        .child(format!("{p}%"))
                })),
        )
        // A plain bar, not gpui-component's Progress: that animates every value change.
        .when(matches!(step, Some(Step::Downloading { .. })), |d| {
            d.child(
                div()
                    .h(px(4.))
                    .w_full()
                    .rounded_full()
                    .bg(theme.border)
                    .child(
                        div()
                            .h_full()
                            .rounded_full()
                            .bg(ui::ink(theme, theme.info))
                            .w(relative(progress.unwrap_or(0) as f32 / 100.)),
                    ),
            )
        })
        .when(matches!(step, Some(Step::Deferred)), |d| {
            d.child(
                div()
                    .text_size(ui::META)
                    .text_color(theme.muted_foreground)
                    .child("SkyeLab will restart and install once active threads finish."),
            )
        })
        .when_some(
            match step {
                Some(Step::Failed(e)) => Some(e.clone()),
                _ => None,
            },
            |d, e| {
                d.child(
                    div()
                        .text_size(ui::META)
                        .text_color(theme.muted_foreground)
                        .child(e),
                )
            },
        )
        .when(!actions.is_empty(), |d| {
            d.child(h_flex().flex_wrap().justify_end().gap_1().children(actions))
        })
        .into_any_element()
}

/// What the popover's title and the pill's tooltip say about `tag` at `step`.
fn headline(tag: &str, step: Option<&Step>) -> String {
    let version = skyelab_core::update::display_version(tag);
    let version = if skyelab_core::update::is_nightly(tag) {
        format!("Nightly {version}")
    } else {
        version.to_owned()
    };
    match step {
        None => format!("SkyeLab {version} is available"),
        Some(Step::Downloading { done, total }) if total.is_some_and(|t| *done >= t) => {
            format!("Checking {version}…")
        }
        Some(Step::Downloading { .. }) => format!("Downloading {version}"),
        Some(Step::Ready) => format!("SkyeLab {version} is ready"),
        Some(Step::Deferred) => format!("Updating to {version} when idle"),
        Some(Step::Failed(_)) => format!("Couldn't update to {version}"),
    }
}
