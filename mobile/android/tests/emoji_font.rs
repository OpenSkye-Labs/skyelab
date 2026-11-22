//! The emoji font the APK bundles renders in colour through the text system gpui-mobile uses on Android
//! (gpui-pre-wgpu's CosmicTextSystem with swash). Runs on the host: the shaper and rasteriser are the same code.

use gpui::{FontRun, PlatformTextSystem, RenderGlyphParams, font, point, px};
use gpui_wgpu::CosmicTextSystem;

/// A Latin font for the primary run, as Roboto is on Android: gpui refuses a primary font without an 'm',
/// so emoji are only ever reached by fallback.
const LATIN: &[(&str, &str)] = &[
    ("/System/Library/Fonts/Geneva.ttf", "Geneva"),
    (
        "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
        "DejaVu Sans",
    ),
];

#[test]
fn bundled_emoji_font_renders_in_colour() {
    // scripts/fetch-gpui-mobile.sh puts it here, and android-host copies it into the APK's assets/fonts/.
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../target/emoji/NotoColorEmoji.ttf"
    );
    let emoji = std::fs::read(path).expect("emoji font missing: run scripts/fetch-gpui-mobile.sh");
    let (latin, family) = LATIN
        .iter()
        .find_map(|(p, family)| Some((std::fs::read(p).ok()?, *family)))
        .expect("no Latin system font found (add this OS's path to LATIN)");

    let text_system = CosmicTextSystem::new_without_system_fonts(family);
    text_system
        .add_fonts(vec![latin.into(), emoji.into()])
        .expect("fonts load");
    let latin_id = text_system
        .font_id(&font(family))
        .expect("Latin family resolves");

    // A face and a flag: gpui-mobile no longer loads the system flags font (upstream/0004), so flags come from here.
    for text in ["😀", "🇪🇺"] {
        let layout = text_system.layout_line(
            text,
            px(32.),
            &[FontRun {
                len: text.len(),
                font_id: latin_id,
            }],
        );
        let run = &layout.runs[0];
        assert_ne!(
            run.font_id, latin_id,
            "{text} did not fall back to the emoji font"
        );
        let glyph = &run.glyphs[0];
        // gpui-pre-wgpu only picks colour sources for a font it knows is an emoji font (by PostScript name).
        assert!(
            glyph.is_emoji,
            "{text} not flagged as emoji: the font's PostScript name must be NotoColorEmoji"
        );

        let params = RenderGlyphParams {
            font_id: run.font_id,
            glyph_id: glyph.id,
            font_size: px(32.),
            subpixel_variant: point(0, 0),
            scale_factor: 2.,
            is_emoji: glyph.is_emoji,
            subpixel_rendering: false,
            dilation: 0,
        };
        let bounds = text_system
            .glyph_raster_bounds(&params)
            .expect("raster bounds");
        let (raster_size, pixels) = text_system
            .rasterize_glyph(&params, bounds)
            .expect("rasterizes");
        let area = (raster_size.width.0 * raster_size.height.0) as usize;
        assert!(area > 0, "{text} has an empty raster");
        assert_eq!(
            pixels.len(),
            area * 4,
            "{text} is not a colour (BGRA) image: it would render as a mask"
        );
        // Both are mostly saturated colour (yellow face, blue flag); a grey mask has every channel equal.
        let coloured = pixels
            .as_chunks::<4>()
            .0
            .iter()
            .filter(|&&[b, g, r, a]| a > 200 && b.max(g).max(r) - b.min(g).min(r) > 100)
            .count();
        assert!(
            coloured > area / 4,
            "{text}: only {coloured} of {area} pixels coloured"
        );
    }
}
