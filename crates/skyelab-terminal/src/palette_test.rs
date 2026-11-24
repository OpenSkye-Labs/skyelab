use gpui::{Hsla, hsla};
use gpui_component::theme::ThemeColor;

use super::Palette;

#[test]
fn selection_cursor_and_link_come_from_the_theme() {
    let sentinel = |h: f32| -> Hsla { hsla(h, 0.5, 0.5, 0.3) };
    let mut theme = ThemeColor::default();
    theme.selection = sentinel(0.1);
    theme.caret = sentinel(0.2);
    theme.link = sentinel(0.3);
    let p = Palette::new(&theme, None);
    assert_eq!(p.selection, sentinel(0.1));
    assert_eq!(p.cursor, sentinel(0.2));
    assert_eq!(p.link, sentinel(0.3));
}

#[test]
fn the_theme_familys_ansi_colors_win() {
    let mut ansi = [hsla(0., 0., 0., 1.); 16];
    ansi[1] = hsla(0.5, 0.5, 0.5, 1.);
    let p = Palette::new(&ThemeColor::default(), Some(ansi));
    assert_eq!(p.ansi[1], ansi[1]);
}
