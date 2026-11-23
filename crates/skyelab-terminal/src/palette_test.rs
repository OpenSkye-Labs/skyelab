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
    let p = Palette::new(&theme);
    assert_eq!(p.selection, sentinel(0.1));
    assert_eq!(p.cursor, sentinel(0.2));
    assert_eq!(p.link, sentinel(0.3));
}
