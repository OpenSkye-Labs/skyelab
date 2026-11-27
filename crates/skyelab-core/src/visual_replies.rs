//! Settings → Chat → Visual replies: allowed script/style/font hosts and preferred page width.

use serde::{Deserialize, Serialize};

/// Width range `html_render` / `html_preview` accepts, in CSS pixels.
pub const MIN_HTML_WIDTH: u32 = 240;
pub const MAX_HTML_WIDTH: u32 = 1600;

/// Preferred layout width for visual reply (`html_render`) cards in the timeline.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HtmlWidth {
    /// Standard reply column width (728 px).
    #[default]
    Standard,
    /// Wider chart and diagram layout (960 px).
    Wide,
    /// Full available width (1280 px, or the container's width capped to `240..=1600`).
    Full,
}

impl HtmlWidth {
    pub const ALL: [Self; 3] = [Self::Standard, Self::Wide, Self::Full];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Standard => "Standard",
            Self::Wide => "Wide",
            Self::Full => "Full",
        }
    }

    /// Target width in CSS pixels when no explicit container width is provided.
    pub const fn default_px(self) -> u32 {
        match self {
            Self::Standard => 728,
            Self::Wide => 960,
            Self::Full => 1280,
        }
    }

    /// Resolves the render width in CSS pixels, clamped to `MIN_HTML_WIDTH..=MAX_HTML_WIDTH`.
    /// `Full` uses `available_px` when supplied, otherwise `1280`.
    pub fn resolve_px(self, available_px: Option<u32>) -> u32 {
        let raw = match self {
            Self::Standard => 728,
            Self::Wide => 960,
            Self::Full => available_px.unwrap_or(1280),
        };
        raw.clamp(MIN_HTML_WIDTH, MAX_HTML_WIDTH)
    }
}

/// Settings → Chat → Visual replies.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct VisualReplySettings {
    /// https hosts (`cdn.example.com`, `*.example.com`) a page may load scripts, styles and fonts from; checked
    /// with `page_hosts::parse_host` when added and again when a page uses it. Only the user adds one.
    pub allowed_hosts: Vec<String>,
    /// Preferred render width for visual reply cards (`standard`, `wide`, or `full`).
    pub width: HtmlWidth,
}

impl Default for VisualReplySettings {
    fn default() -> Self {
        Self {
            allowed_hosts: crate::page_hosts::DEFAULT_PAGE_HOSTS
                .map(String::from)
                .to_vec(),
            width: HtmlWidth::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::AppSettings;

    #[test]
    fn html_width_defaults_and_clamping() {
        assert_eq!(HtmlWidth::default(), HtmlWidth::Standard);
        assert_eq!(HtmlWidth::Standard.default_px(), 728);
        assert_eq!(HtmlWidth::Wide.default_px(), 960);
        assert_eq!(HtmlWidth::Full.default_px(), 1280);

        assert_eq!(HtmlWidth::Standard.resolve_px(Some(1400)), 728);
        assert_eq!(HtmlWidth::Wide.resolve_px(Some(1400)), 960);
        assert_eq!(HtmlWidth::Full.resolve_px(None), 1280);
        assert_eq!(HtmlWidth::Full.resolve_px(Some(1100)), 1100);
        assert_eq!(HtmlWidth::Full.resolve_px(Some(100)), MIN_HTML_WIDTH);
        assert_eq!(HtmlWidth::Full.resolve_px(Some(2400)), MAX_HTML_WIDTH);
    }

    #[test]
    fn app_settings_visual_replies_round_trip_and_compat() {
        // Older settings.toml without `width` loads with Standard.
        let legacy = r#"
[visual_replies]
allowed_hosts = ["cdn.example.com"]
"#;
        let parsed: AppSettings = toml::from_str(legacy).expect("parse legacy");
        assert_eq!(parsed.visual_replies.allowed_hosts, ["cdn.example.com"]);
        assert_eq!(parsed.visual_replies.width, HtmlWidth::Standard);

        for (width, token) in [
            (HtmlWidth::Standard, "standard"),
            (HtmlWidth::Wide, "wide"),
            (HtmlWidth::Full, "full"),
        ] {
            let mut settings = AppSettings::default();
            settings.visual_replies.width = width;
            let text = toml::to_string_pretty(&settings).expect("serialize");
            assert!(
                text.contains(&format!("width = \"{token}\"")),
                "serialized width missing {token}:\n{text}"
            );
            let back: AppSettings = toml::from_str(&text).expect("round-trip");
            assert_eq!(back.visual_replies.width, width);
        }

        // Unknown width value is rejected.
        let bad = "[visual_replies]\nwidth = \"huge\"\n";
        assert!(toml::from_str::<AppSettings>(bad).is_err());
    }
}
