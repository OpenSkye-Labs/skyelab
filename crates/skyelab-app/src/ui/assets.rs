//! SkyeLab's artwork and icons, plus the assets gpui-component controls still draw.

use super::{SkyeIcon, icons, provider_logo};
use gpui::{Hsla, Pixels};

const BRAND_MARK: &str = "icons/skyelab-mark.svg";

const PEEK_MARK: &str = "icons/skyelab-peek.svg";

pub fn brand_mark(size: Pixels, color: Hsla) -> gpui::Svg {
    brand_svg(BRAND_MARK, size, color)
}

pub fn peek_mark(size: Pixels, color: Hsla) -> gpui::Svg {
    brand_svg(PEEK_MARK, size, color)
}

fn brand_svg(path: &'static str, size: Pixels, color: Hsla) -> gpui::Svg {
    use gpui::Styled as _;

    gpui::svg()
        .path(path)
        .w(size)
        .h(size)
        .flex_shrink_0()
        .text_color(color)
}

/// SkyeLab artwork plus the assets used by gpui-component controls.
pub struct AppAssets;

impl gpui::AssetSource for AppAssets {
    fn load(&self, path: &str) -> gpui::Result<Option<std::borrow::Cow<'static, [u8]>>> {
        let native_icon = match path {
            "icons/maximize.svg" => Some(SkyeIcon::Expand),
            "icons/minimize.svg" => Some(SkyeIcon::Collapse),
            _ => None,
        };
        let resolved = native_icon.map(SkyeIcon::path);
        let path = resolved.as_deref().unwrap_or(path);
        let bytes: &'static [u8] = match path {
            BRAND_MARK => include_bytes!("../../icons/mark.svg"),
            PEEK_MARK => include_bytes!("../../icons/peek.svg"),
            _ => match icons::bytes(path).or_else(|| provider_logo::bytes(path)) {
                Some(bytes) => bytes,
                None => return gpui_kit_assets::Assets.load(path),
            },
        };
        Ok(Some(std::borrow::Cow::Borrowed(bytes)))
    }

    fn list(&self, path: &str) -> gpui::Result<Vec<gpui::SharedString>> {
        let mut paths = gpui_kit_assets::Assets.list(path)?;
        paths.extend(
            icons::PATHS
                .iter()
                .chain(provider_logo::PATHS)
                .copied()
                .chain([BRAND_MARK, PEEK_MARK])
                .filter(|asset| asset.starts_with(path))
                .map(gpui::SharedString::from),
        );
        paths.sort();
        paths.dedup();
        Ok(paths)
    }
}
