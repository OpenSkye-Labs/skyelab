//! Row and column shorthands every view lays out with.

use gpui::{Div, Styled as _, div};

/// A row whose children center on the cross axis.
pub fn h_flex() -> Div {
    div().flex().flex_row().items_center()
}

/// A column whose children stretch across it.
pub fn v_flex() -> Div {
    div().flex().flex_col()
}
