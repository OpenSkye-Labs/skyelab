//! SkyeLab's design system, on top of gpui-component's theme: the spacing, radius and type rules every view uses,
//! the semantic colors (an account's color, a thread status's color), and a few small components.
//!
//! Spacing stays on gpui's 4 px helpers (`1` = 4 px, `2p5` = 10 px, `6` = 24 px, …). Colors only ever come from the
//! theme.

mod components;
#[cfg(test)]
mod tests;

pub use components::{
    AccountBadge, EmptyState, KeyHint, Monogram, Pill, SectionHeader, StatusDot, group, group_row,
    header, mono, page_title, row, section_title,
};

use gpui::{Hsla, Pixels, px};
use gpui_component::Theme;
use std::path::Path;

/// SkyeLab's light and dark themes: T3 Code's zinc light and true-black dark tokens, in gpui-component's
/// theme format (see docs/design/t3-reference.md). Views read colors only from the active theme.
const THEMES: &str = include_str!("../../themes/skyelab.json");

/// Registers SkyeLab's themes and makes them the light and dark themes. Call once, after `gpui_component::init`;
/// the workspace then picks light or dark from the system appearance.
pub fn install_theme(cx: &mut gpui::App) -> anyhow::Result<()> {
    let registry = gpui_component::ThemeRegistry::global_mut(cx);
    registry.load_themes_from_str(THEMES)?;
    let find = |name: &str| {
        registry
            .themes()
            .get(name)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("theme {name} is missing from skyelab.json"))
    };
    let (light, dark) = (find("SkyeLab Light")?, find("SkyeLab Dark")?);
    let mode = cx.global::<Theme>().mode;
    Theme::global_mut(cx).light_theme = light;
    Theme::global_mut(cx).dark_theme = dark;
    Theme::change(mode, None, cx);
    Ok(())
}

/// Follows the system appearance. Debug builds only: `SKYELAB_VERIFY_THEME=dark|light` forces one, so
/// `/loop-verify-ui` can capture both themes without changing the system setting.
pub fn sync_appearance(window: &mut gpui::Window, cx: &mut gpui::App) {
    let forced = match std::env::var("SKYELAB_VERIFY_THEME").as_deref() {
        Ok("dark") if cfg!(debug_assertions) => Some(gpui_component::ThemeMode::Dark),
        Ok("light") if cfg!(debug_assertions) => Some(gpui_component::ThemeMode::Light),
        _ => None,
    };
    match forced {
        Some(mode) => Theme::change(mode, Some(window), cx),
        None => Theme::sync_system_appearance(Some(window), cx),
    }
}

/// The window's top row (T3's 52 px topbar): the sidebar's first row beside the page header ([`HEADER_H`]). There is
/// no separate title strip: on macOS the traffic lights sit in this row over the sidebar (or over the page header's
/// start while the sidebar is hidden), on Windows gpui-component's `TitleBar` draws the window controls in theme
/// colors. Linux keeps its server-side decorations.
pub const TOP_ROW_H: Pixels = HEADER_H;
/// Room the macOS traffic lights take at the start of the top row.
pub const TRAFFIC_LIGHTS_W: Pixels = px(76.);

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

/// Corner radii (docs/design/t3-reference.md §3): badges and keys `SM`, sidebar rows `MD`, controls and cards `BASE`,
/// grouped settings `LG`, the composer and message bubbles `XL`.
pub const RADIUS_SM: Pixels = px(4.);
pub const RADIUS_MD: Pixels = px(6.);
pub const RADIUS: Pixels = px(8.);
pub const RADIUS_LG: Pixels = px(12.);
pub const RADIUS_XL: Pixels = px(16.);

/// Type scale (§2): `META` for times, paths, labels and section captions, `BODY` for rows and prose, `CODE` for mono, `SECTION` for a settings section title, `PAGE` for a page title.
/// Weights: titles semibold, labels and selected rows medium, nothing heavier.
pub const META: Pixels = px(12.);
pub const BODY: Pixels = px(14.);
pub const CODE: Pixels = px(13.);
pub const SECTION: Pixels = px(16.);
pub const PAGE: Pixels = px(20.);

/// Geometry (§3, §4): sidebar and list rows, a view header, the chat column's width, the settings page's width.
pub const ROW_H: Pixels = px(32.);
pub const HEADER_H: Pixels = px(52.);
pub const CHAT_MAX_W: Pixels = px(736.);
pub const PAGE_MAX_W: Pixels = px(896.);

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
        .map(|slot| account_palette(gpui_component::ActiveTheme::theme(cx))[slot]);
    slot.unwrap_or_else(|| account_color(gpui_component::ActiveTheme::theme(cx), id))
}

/// A status color used as text. In light themes the 500-weight status colors are too light to read on the canvas
/// (amber is about 2:1), so text takes them 40% toward the foreground, like T3's `-foreground` 700 shades; dots and
/// icons keep the plain color. Dark themes read fine as is.
pub fn ink(theme: &Theme, color: Hsla) -> Hsla {
    use gpui_component::Colorize as _;
    if theme.is_dark() {
        color
    } else {
        color.mix_oklab(theme.foreground, 0.6)
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
            Self::Interrupted | Self::Stale => theme.warning,
            Self::Failed => theme.danger,
            Self::Stopped | Self::Unknown => theme.muted_foreground,
        }
    }

    /// What a thread row shows in place of its time, as T3 does: an icon and a word for states that need a look.
    /// `None` for settled states (idle, stopped), which show the time instead.
    pub fn badge(self) -> Option<(gpui_component::IconName, &'static str)> {
        use gpui_component::IconName as I;
        match self {
            Self::Starting | Self::Running => Some((I::LoaderCircle, "Working")),
            Self::Interrupted => Some((I::TriangleAlert, "Interrupted")),
            Self::Failed => Some((I::CircleAlert, "Failed")),
            Self::Stale => Some((I::TriangleAlert, "Stale")),
            Self::Idle | Self::Stopped | Self::Unknown => None,
        }
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

/// Milliseconds since the epoch, for [`ago`].
pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
}

/// `path` with the home folder written `~`, for display only.
pub fn short_path(path: &Path, home: Option<&Path>) -> String {
    match home.and_then(|h| path.strip_prefix(h).ok()) {
        Some(rest) if rest.as_os_str().is_empty() => "~".into(),
        Some(rest) => format!("~/{}", rest.display()),
        None => path.display().to_string(),
    }
}

// The Lucide icons SkyeLab draws beyond gpui-component's own set (the composer's attach, footer and queue icons).
gpui_kit_assets::icon_assets!(
    ExtraIcons,
    [
        Paperclip, GitBranch, Laptop, Folder, FolderGit2, FileDiff, X, Clock, Square
    ]
);

/// gpui-component's icons plus [`ExtraIcons`].
pub struct AppAssets;

impl gpui::AssetSource for AppAssets {
    fn load(&self, path: &str) -> gpui::Result<Option<std::borrow::Cow<'static, [u8]>>> {
        match ExtraIcons.load(path)? {
            Some(bytes) => Ok(Some(bytes)),
            None => gpui_kit_assets::Assets.load(path),
        }
    }

    fn list(&self, path: &str) -> gpui::Result<Vec<gpui::SharedString>> {
        let mut paths = gpui_kit_assets::Assets.list(path)?;
        paths.extend(ExtraIcons.list(path)?);
        paths.sort();
        paths.dedup();
        Ok(paths)
    }
}
