//! SkyeLab's design system, on top of gpui-component's theme: the spacing, radius and type rules every view uses,
//! the semantic colors (an account's color, a thread status's color), and a few small components.
//!
//! Spacing stays on gpui's 4 px helpers (`1` = 4 px, `2p5` = 10 px, `6` = 24 px, …). Colors only ever come from the
//! theme.

#[cfg(test)]
mod a11y_audit_test;
mod a11y_disabled;
mod a11y_scroll;
pub(crate) use a11y_scroll::A11yScrollArea;

/// `view` drawn as a cached view, except while an accessibility client is attached: GPUI 0.3.14 doesn't re-emit a
/// reused view's a11y nodes, so a cached view would drop out of the tree on every frame that redraws only others.
pub fn cached_view(
    view: gpui::AnyView,
    style: gpui::StyleRefinement,
    window: &gpui::Window,
) -> gpui::AnyElement {
    use gpui::IntoElement as _;
    if window.is_a11y_active() {
        view.into_any_element()
    } else {
        view.cached(style).into_any_element()
    }
}
#[cfg(test)]
mod a11y_test;
mod button;
#[cfg(test)]
mod button_test;
mod choice;
mod command_row;
mod components;
#[cfg(test)]
mod contrast_test;
mod dialog;
#[cfg(test)]
mod dialog_close_test;
#[cfg(test)]
mod dialog_fade_click_test;
#[cfg(test)]
mod dialog_focus_test;
#[cfg(test)]
mod dialog_keymap_reload_test;
mod dialog_layer;
#[cfg(test)]
mod dialog_look_test;
#[cfg(test)]
mod dialog_selection_test;
#[cfg(test)]
mod dialog_stack_test;
#[cfg(test)]
mod dialog_test;
#[cfg(test)]
mod dialog_title_strip_test;
mod dropdown;
#[cfg(test)]
mod dropdown_keys_test;
pub mod gallery;
#[cfg(test)]
mod gallery_test;
#[cfg(test)]
mod icon_layout_test;
mod kbd;
#[cfg(test)]
mod kit_boundaries_test;
#[cfg(test)]
mod kit_boundary_test;
mod menu;
#[cfg(test)]
mod menu_keys_test;
#[cfg(test)]
mod menu_nested_click_test;
#[cfg(test)]
mod menu_parity_test;
mod menu_render;
pub mod motion;
#[cfg(test)]
mod motion_test;
mod popover;
#[cfg(test)]
mod popover_follow_test;
#[cfg(test)]
mod popover_test;
mod surface;
pub mod syntax;
#[cfg(test)]
mod terminal_colors_test;
#[cfg(test)]
mod tests;
mod text_field;
pub mod theme;
mod theme_bridge;
#[cfg(test)]
mod theme_font_test;
pub(crate) mod theme_load;
#[cfg(test)]
mod theme_load_test;
#[cfg(test)]
mod theme_match_test;
#[cfg(test)]
mod theme_switch_test;
pub mod themes;
#[cfg(test)]
mod themes_test;
pub mod toast;
#[cfg(test)]
mod toast_test;
mod toggle;
#[cfg(test)]
mod toggle_test;
pub(crate) mod tooltip;
pub mod window_controls;
mod window_root;

pub use button::{Button, Variant, icon_button};
pub use choice::{SegmentedControl, Tabs};
pub use command_row::CommandRow;
pub use components::{
    AccountBadge, EmptyState, KeyHint, Monogram, Pill, SectionHeader, StatusDot, group, group_row,
    header, mono, page_title, row, row_surface, section_title, subsection_title,
};
pub use dialog::{
    DialogEntrance, close_dialog, dialog_cancel, dialog_footer, has_active_dialog, open_dialog,
};
pub fn bind_keys(cx: &mut gpui::App) {
    dialog_layer::bind_keys(cx);
    menu::bind_keys(cx);
    popover::bind_keys(cx);
}
pub use kbd::{Kbd, keys};
pub use menu::{PopupMenu, PopupMenuItem};
pub use popover::{HoverCard, Popover};
pub use surface::{Card, Divider};
pub use text_field::TextField;
pub use toggle::{Checkbox, Switch};
pub use tooltip::Tooltip;
pub use window_controls::{WindowControls, drag_area, no_drag};
pub use window_root::window_root;

use gpui::{Hsla, Pixels, Rems, px, rems};
use std::path::Path;
pub use theme::{ActiveTheme, Theme};

/// Start-up for the UI: gpui-component's init (until stage 8), the app identity OS notifications need (Windows'
/// AppUserModelID; the bundle's id), then our notification click handler, after gpui-component's so ours is the one
/// GPUI keeps.
pub fn init(cx: &mut gpui::App) {
    gpui_component::init(cx);
    cx.set_app_identity("dev.openskye.skyelab", "SkyeLab");
    toast::init_system(cx);
}

/// Checks SkyeLab's theme families (T3 Code's zinc light and true-black dark tokens are the default, see
/// docs/design/t3-reference.md) and applies the default appearance. Call once at start, after `gpui_component::init`
/// (the bridge writes its theme too); then [`themes::load`] applies the saved choice. Views read colors only from the active theme.
pub fn install_theme(cx: &mut gpui::App) -> anyhow::Result<()> {
    themes::register()?;
    themes::apply(cx);
    Ok(())
}

/// The caret blinks while you type, then holds still: each blink re-renders the thread page around the composer (not
/// its timeline rows, docs/gpui/redraw.md), so an idle window should not redraw twice a second forever.
pub const CURSOR_BLINK: gpui_component::input::CursorBlink =
    gpui_component::input::CursorBlink::blinking(std::time::Duration::from_millis(500))
        .with_idle_timeout(std::time::Duration::from_secs(5));

/// Follows the Appearance setting (System follows the window's appearance). Debug builds only:
/// `SKYELAB_VERIFY_THEME=dark|light` forces one, so `/loop-verify-ui` can capture both without changing the system.
pub fn sync_appearance(window: &mut gpui::Window, cx: &mut gpui::App) {
    themes::sync(window, cx);
}

/// The window's top row (T3's 52 px topbar): the sidebar's first row beside the page header ([`HEADER_H`]). There is
/// no separate title strip: on macOS the traffic lights sit in this row over the sidebar (or over the page header's
/// start while the sidebar is hidden), on Windows gpui-component's `TitleBar` draws the window controls in theme
/// colors. Linux keeps its server-side decorations.
pub const TOP_ROW_H: Pixels = HEADER_H;
/// Width of the slim left navigation rail on the far left of the main window.
pub const RAIL_W: Pixels = px(48.);
/// Room the macOS traffic lights extend past the 48 px rail (`76 - 48 = 28 px`).
pub const MAC_TOP_PAD: Pixels = px(76. - 48.);

/// The titlebar every SkyeLab window opens with: transparent (Linux ignores it) so the app's own surfaces reach the top,
/// traffic lights in [`TOP_ROW_H`]. `app_owns_titlebar_drag` stays false, so AppKit still drags the window from its own
/// titlebar band (about 28 px) and zooms on a double click.
pub fn titlebar_options() -> gpui::TitlebarOptions {
    gpui::TitlebarOptions {
        title: Some("SkyeLab".into()),
        appears_transparent: true,
        // Centred in the 52 px top row (the buttons are about 14 px tall).
        traffic_light_position: Some(gpui::point(px(16.), px(19.))),
    }
}

/// The least room an overlay keeps from the window's edges.
pub const SPACE_LG: Pixels = px(16.);
/// Corner radii (docs/design/t3-reference.md §3): badges and keys `SM`, sidebar rows `MD`, controls and cards `BASE`,
/// grouped settings `LG`, the composer and message bubbles `XL`.
pub const RADIUS_SM: Pixels = px(4.);
pub const RADIUS_MD: Pixels = px(6.);
pub const RADIUS: Pixels = px(8.);
pub const RADIUS_LG: Pixels = px(12.);
pub const RADIUS_XL: Pixels = px(16.);

/// Type scale (§2): `MICRO` for counters and compact badges, `META` for times, paths, labels and section captions,
/// `SUBTEXT` for control labels and settings descriptions, `CODE` for mono, `BODY` for rows and prose, `SECTION` for a
/// settings section title, `PAGE` for a page title, `HERO` for onboarding headings, `DISPLAY` for large totals.
/// Weights: titles semibold, labels and selected rows medium. In rems of the theme's font size (gpui-component's `Root`
/// sets the rem to it), so Settings → Appearance's text size scales every view; at the default 14 they are 11, 12, 13,
/// 13, 14, 16, 20, 26 and 30 px.
pub const MICRO: Rems = rems(11. / 14.);
pub const META: Rems = rems(12. / 14.);
pub const SUBTEXT: Rems = rems(13. / 14.);
pub const BODY: Rems = rems(1.);
pub const CODE: Rems = rems(13. / 14.);
pub const SECTION: Rems = rems(16. / 14.);
pub const PAGE: Rems = rems(20. / 14.);
pub const HERO: Rems = rems(26. / 14.);
pub const DISPLAY: Rems = rems(30. / 14.);

/// Control sizes (§6, T3's button `xs` / `sm` / default): the height of a button, text field, segmented control or
/// icon button (square), with its icon size, text size, side padding and gap. One place to resize every control.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Size {
    /// 24 px, 12 px text, 14 px icons: dense rows, cards, inline actions. `Sm` and `Md` use 13 px text; `Sm` keeps 14 px icons, `Md` 16.
    Xs,
    /// 28 px: toolbars, headers, the composer, settings rows.
    #[default]
    Sm,
    /// 32 px: page actions, dialog footers, forms.
    Md,
}

impl Size {
    pub fn height(self) -> Pixels {
        match self {
            Self::Xs => px(24.),
            Self::Sm => px(28.),
            Self::Md => px(32.),
        }
    }

    pub fn icon(self) -> Pixels {
        match self {
            Self::Xs | Self::Sm => px(14.),
            Self::Md => px(16.),
        }
    }

    /// 12 px for `Xs`, 13 px otherwise: controls read a step under body text, as gpui-component's `text_sm` did.
    pub fn text(self) -> Rems {
        match self {
            Self::Xs => META,
            Self::Sm | Self::Md => SUBTEXT,
        }
    }

    pub fn pad_x(self) -> Pixels {
        match self {
            Self::Xs => px(8.),
            Self::Sm => px(10.),
            Self::Md => px(12.),
        }
    }

    pub fn gap(self) -> Pixels {
        match self {
            Self::Xs => px(4.),
            Self::Sm => px(6.),
            Self::Md => px(8.),
        }
    }

    pub fn radius(self) -> Pixels {
        match self {
            Self::Xs => RADIUS_MD,
            Self::Sm | Self::Md => RADIUS,
        }
    }
}

/// A disabled control's opacity (T3: 64%). Disabled controls keep their colors and fade, so every variant dims alike.
pub const DISABLED_OPACITY: f32 = 0.64;

/// Geometry (§3, §4): sidebar and list rows, a view header, the chat column's width, the settings page's width.
pub const ROW_H: Pixels = px(32.);
pub const HEADER_H: Pixels = px(52.);
pub const CHAT_MAX_W: Pixels = px(736.);
pub const PAGE_MAX_W: Pixels = px(896.);
/// The width of the centered Settings column.
pub const SETTINGS_MAX_W: Pixels = px(780.);

/// FNV-1a (64-bit): stable across runs, builds and platforms, unlike std's `DefaultHasher`.
pub(crate) fn fnv1a(s: &str) -> u64 {
    s.bytes().fold(0xcbf2_9ce4_8422_2325, |h, b| {
        (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3)
    })
}

/// The hues an account can take. No red: an account must never read as an error.
pub(crate) fn account_palette(theme: &Theme) -> [Hsla; 5] {
    [
        theme.blue,
        theme.magenta,
        theme.cyan,
        theme.green,
        theme.yellow,
    ]
}

const PALETTE_LEN: usize = 5;

/// An id's palette slot by hash alone. The modulo is < 5, so the cast can't truncate.
fn hash_slot(id: &str) -> usize {
    (fnv1a(id) % PALETTE_LEN as u64) as usize
}

/// The color of account `id` by hash alone, the same in every session.
pub fn account_color(theme: &Theme, id: &str) -> Hsla {
    account_palette(theme)[hash_slot(id)]
}

/// Each configured account's palette slot. Hashed first; on a clash the later id (in sorted order) takes the next free
/// slot, so two accounts share a color only when there are more accounts than colors. The result depends only on the
/// set of ids, so it is the same in every session and whatever order accounts.toml lists them in.
pub(crate) fn account_slots<'a>(
    ids: impl IntoIterator<Item = &'a str>,
) -> std::collections::HashMap<String, usize> {
    let mut ids: Vec<&str> = ids.into_iter().collect();
    ids.sort_unstable();
    ids.dedup();
    let mut taken = [false; PALETTE_LEN];
    let mut slots = std::collections::HashMap::new();
    for id in ids {
        if taken.iter().all(|t| *t) {
            taken = [false; PALETTE_LEN];
        }
        let mut slot = hash_slot(id);
        while taken[slot] {
            slot = (slot + 1) % PALETTE_LEN;
        }
        taken[slot] = true;
        slots.insert(id.to_owned(), slot);
    }
    slots
}

/// The configured accounts' colors, set whenever the accounts config is (re)loaded.
#[derive(Default)]
pub struct AccountSlots(std::collections::HashMap<String, usize>);

impl gpui::Global for AccountSlots {}

/// Records the configured accounts, so each gets its own color (see [`account_slots`]).
pub fn set_accounts(cx: &mut gpui::App, config: &skyelab_core::AccountsConfig) {
    cx.set_global(AccountSlots(account_slots(
        config.accounts.iter().map(|a| a.id.as_str()),
    )));
}

/// The color of account `id`: its configured slot, else its hash slot (an account that is gone from the config).
pub fn account_color_in(cx: &gpui::App, id: &str) -> Hsla {
    let slot = cx
        .try_global::<AccountSlots>()
        .and_then(|s| s.0.get(id).copied())
        .map(|slot| account_palette(crate::ui::ActiveTheme::theme(cx))[slot]);
    slot.unwrap_or_else(|| account_color(crate::ui::ActiveTheme::theme(cx), id))
}

/// A status color used as text. In light themes the 500-weight status colors are too light to read on the canvas
/// (amber is about 2:1), so text takes them 45% toward the foreground, like T3's `-foreground` 700 shades; dots and
/// icons keep the plain color. Dark themes read fine as is.
pub fn ink(theme: &Theme, color: Hsla) -> Hsla {
    use gpui_component::Colorize as _;
    if theme.is_dark() {
        color
    } else {
        color.mix_oklab(theme.foreground, 0.55)
    }
}

/// A thread's state as the UI shows it: the store's status strings, plus a stale binding.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Starting,
    Running,
    Idle,
    Stopped,
    Interrupted,
    Failed,
    Stale,
    /// The account's usage limit holds the agent back.
    Limited,
    Unknown,
}

impl Status {
    pub fn parse(raw: &str) -> Self {
        match raw {
            "starting" => Self::Starting,
            "running" => Self::Running,
            "idle" | "done" => Self::Idle,
            "stopped" | "exited" => Self::Stopped,
            "interrupted" => Self::Interrupted,
            "failed" => Self::Failed,
            _ => Self::Unknown,
        }
    }

    pub fn color(self, theme: &Theme) -> Hsla {
        match self {
            Self::Starting | Self::Running => theme.info,
            Self::Idle => theme.success,
            Self::Interrupted | Self::Stale | Self::Limited => theme.warning,
            Self::Failed => theme.danger,
            Self::Stopped | Self::Unknown => theme.muted_foreground,
        }
    }

    /// What a thread row shows in place of its time, as T3 does: an icon and a word for states that need a look.
    /// `None` for settled states (idle, stopped), which show the time instead.
    pub fn badge(self) -> Option<(SkyeIcon, &'static str)> {
        use SkyeIcon as I;
        match self {
            Self::Starting | Self::Running => Some((I::LoaderCircle, "Working")),
            Self::Interrupted => Some((I::TriangleAlert, "Interrupted")),
            Self::Failed => Some((I::CircleAlert, "Failed")),
            Self::Stale => Some((I::TriangleAlert, "Stale")),
            Self::Limited => Some((I::TriangleAlert, "Limited")),
            Self::Idle | Self::Stopped | Self::Unknown => None,
        }
    }
}

/// The local time of `ms` (since the epoch): "14:03" today, "Nov 21, 14:03" before.
pub fn clock(ms: i64) -> String {
    use chrono::{Local, TimeZone as _};
    let Some(at) = Local.timestamp_millis_opt(ms).single() else {
        return String::new();
    };
    if at.date_naive() == Local::now().date_naive() {
        at.format("%H:%M").to_string()
    } else {
        at.format("%b %-d, %H:%M").to_string()
    }
}

/// How long ago `then_ms` was, T3 style: `now`, `3m`, `2h`, `5d`, `2w`, `1y`.
pub fn ago(now_ms: i64, then_ms: i64) -> String {
    let secs = now_ms.saturating_sub(then_ms).max(0) / 1000;
    let (m, h, d) = (secs / 60, secs / 3600, secs / 86_400);
    if m < 1 {
        "now".into()
    } else if h < 1 {
        format!("{m}m")
    } else if d < 1 {
        format!("{h}h")
    } else if d < 7 {
        format!("{d}d")
    } else if d < 365 {
        format!("{}w", d / 7)
    } else {
        format!("{}y", d / 365)
    }
}

/// How long a running turn has run, T3's working timer: `12s`, `1m 12s`, `1h 05m`.
pub fn elapsed(ms: i64) -> String {
    let secs = ms.max(0) / 1000;
    let (m, h) = (secs / 60, secs / 3600);
    if m < 1 {
        format!("{secs}s")
    } else if h < 1 {
        format!("{m}m {:02}s", secs % 60)
    } else {
        format!("{h}h {:02}m", m % 60)
    }
}

/// Milliseconds since the epoch, for [`ago`].
pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
}

/// A file a tool touches as a row shows it: relative to the thread's folder `root` when inside it (macOS's
/// `/private/tmp` counts as `/tmp`), else [`short_path`].
pub fn folder_path(path: &str, root: Option<&Path>, home: Option<&Path>) -> String {
    let p = Path::new(path);
    // One side may carry macOS's `/private` and the other not; compare both without it.
    let bare = |p: &Path| {
        p.strip_prefix("/private")
            .map_or(p.to_path_buf(), |r| Path::new("/").join(r))
    };
    let rest = root.and_then(|root| {
        let rest = bare(p).strip_prefix(bare(root)).ok()?.to_path_buf();
        (!rest.as_os_str().is_empty()).then_some(rest)
    });
    match rest {
        Some(rest) => rest.display().to_string(),
        None => short_path(p, home),
    }
}

/// `path` with the home folder written `~`, for display only.
pub fn short_path(path: &Path, home: Option<&Path>) -> String {
    match home.and_then(|h| path.strip_prefix(h).ok()) {
        Some(rest) if rest.as_os_str().is_empty() => "~".into(),
        Some(rest) => format!("~/{}", rest.display()),
        None => path.display().to_string(),
    }
}

mod icon;
mod icons;
mod layout;
pub use icon::Icon;
pub(crate) use icons::SkyeIcon;
pub use layout::{h_flex, v_flex};
mod provider_logo;
pub use provider_logo::provider_logo;

mod assets;
pub use assets::{AppAssets, brand_mark, peek_mark};

mod spinner;
pub use spinner::Spinner;
mod copy_button;
pub use copy_button::CopyButton;
mod status_bar;
pub use status_bar::StatusBar;

#[cfg(test)]
mod os_notification_test;
#[cfg(test)]
mod small_pieces_parity_test;
#[cfg(test)]
mod toggle_disabled_regress_test;
#[cfg(test)]
mod toggle_focus_regress_test;
#[cfg(test)]
mod toggle_keys_test;
