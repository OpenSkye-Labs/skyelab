//! The providers' own logos, drawn as images in their official colors: never a tinted icon, never recolored or faded
//! (docs/design/third-party-marks.md). An account's color goes in a badge beside the logo, never on it.

use gpui::{App, Pixels, Styled as _, img};
use gpui_component::ActiveTheme as _;
use skyelab_core::Provider;

const CLAUDE: &str = "brand/claude.svg";
/// OpenAI publishes the Blossom in black and in white: black on light themes, white on dark ones.
const OPENAI_BLACK: &str = "brand/openai-black.svg";
const OPENAI_WHITE: &str = "brand/openai-white.svg";
/// Cursor publishes its cube mark in dark (#26251E) on light themes and light (#EDECEC) on dark ones.
const CURSOR_BLACK: &str = "brand/cursor-black.svg";
const CURSOR_WHITE: &str = "brand/cursor-white.svg";
/// Antigravity provider mark in blue, legible on both light and dark themes.
const ANTIGRAVITY: &str = "brand/antigravity.svg";

pub(super) const PATHS: &[&str] = &[
    CLAUDE,
    OPENAI_BLACK,
    OPENAI_WHITE,
    CURSOR_BLACK,
    CURSOR_WHITE,
    ANTIGRAVITY,
];

pub(super) fn bytes(path: &str) -> Option<&'static [u8]> {
    match path {
        CLAUDE => Some(include_bytes!("../../icons/brand/claude.svg")),
        OPENAI_BLACK => Some(include_bytes!("../../icons/brand/openai-black.svg")),
        OPENAI_WHITE => Some(include_bytes!("../../icons/brand/openai-white.svg")),
        CURSOR_BLACK => Some(include_bytes!("../../icons/brand/cursor-black.svg")),
        CURSOR_WHITE => Some(include_bytes!("../../icons/brand/cursor-white.svg")),
        ANTIGRAVITY => Some(include_bytes!("../../icons/brand/antigravity.svg")),
        _ => None,
    }
}

/// The logo's file for this theme.
fn path(provider: Provider, dark: bool) -> &'static str {
    match provider {
        Provider::Claude => CLAUDE,
        Provider::Codex if dark => OPENAI_WHITE,
        Provider::Codex => OPENAI_BLACK,
        Provider::Cursor if dark => CURSOR_WHITE,
        Provider::Cursor => CURSOR_BLACK,
        Provider::Antigravity => ANTIGRAVITY,
    }
}

/// How much of its file's box each logo fills: OpenAI's file keeps its clear space inside (the Blossom spans 356 of
/// 716 units); Claude's Spark, Cursor's Cube and the Antigravity mark fill their boxes.
fn fill(provider: Provider) -> f32 {
    match provider {
        Provider::Claude | Provider::Cursor | Provider::Antigravity => 1.,
        Provider::Codex => 356. / 716.,
    }
}

/// The provider's logo, its visible mark `size` across, in the variant for the active theme. The file is drawn
/// unchanged; a file with clear space inside is drawn larger, its margins pulled in so it lays out at `size`.
pub fn provider_logo(provider: Provider, size: Pixels, cx: &App) -> gpui::Img {
    let drawn = size / fill(provider);
    let inset = (size - drawn) / 2.;
    img(path(provider, cx.theme().is_dark()))
        .w(drawn)
        .h(drawn)
        .m(inset)
        .flex_shrink_0()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Each logo is the brand's own file with its own fill, so it draws in its official color: an image, never a `currentColor` mask a
    /// theme or account color could tint. Nothing else rides in the file (no `<image>`, styles or scripts).
    #[test]
    fn logos_carry_their_official_colors() {
        let official = [
            (CLAUDE, "#D97757"),
            (OPENAI_BLACK, "black"),
            (OPENAI_WHITE, "white"),
            (CURSOR_BLACK, "#26251E"),
            (CURSOR_WHITE, "#EDECEC"),
            (ANTIGRAVITY, "#4285F4"),
        ];
        for (path, fill) in official {
            let svg = std::str::from_utf8(bytes(path).expect("embedded")).expect("utf-8");
            assert!(
                svg.contains(&format!("fill=\"{fill}\"")),
                "{path} is not {fill}"
            );
            for banned in ["currentColor", "<image", "<style", "<script", "opacity"] {
                assert!(!svg.contains(banned), "{path} has {banned}");
            }
        }
        assert!(PATHS.iter().all(|p| bytes(p).is_some()));
    }

    #[test]
    fn openai_takes_the_variant_for_the_theme_claude_keeps_its_one() {
        assert_eq!(path(Provider::Codex, true), OPENAI_WHITE);
        assert_eq!(path(Provider::Codex, false), OPENAI_BLACK);
        assert_eq!(path(Provider::Cursor, true), CURSOR_WHITE);
        assert_eq!(path(Provider::Cursor, false), CURSOR_BLACK);
        assert_eq!(path(Provider::Claude, true), CLAUDE);
        assert_eq!(path(Provider::Claude, false), CLAUDE);
        assert_eq!(path(Provider::Antigravity, true), ANTIGRAVITY);
        assert_eq!(path(Provider::Antigravity, false), ANTIGRAVITY);
    }
}
