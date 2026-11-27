//! SkyeLab's dialogs: [`Dialog`] is what one shows (a title, the body, a footer of SkyeLab buttons with Cancel
//! ([`dialog_cancel`]) before the action); the window's [`DialogLayer`](super::dialog_layer::DialogLayer) draws the
//! open ones over everything, with the backdrop, Esc, the focus trap and the fade. Open one with [`open_dialog`].

use super::dialog_layer::DialogLayer;
use super::{BODY, Button};
use gpui::{
    AnyElement, App, Div, FontWeight, IntoElement, ParentElement, Pixels, SharedString,
    StyleRefinement, Styled, Window, div, prelude::*, px,
};
use gpui_component::h_flex;
use std::rc::Rc;

/// How a dialog appears.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DialogEntrance {
    /// Fades in place over [`super::motion::FADE`] and out over [`super::motion::FADE_OUT`].
    #[default]
    Fade,
    /// Drawn at once and gone at once (the command palette).
    None,
}

/// Passed to [`Dialog::on_close`]: the dialog closed, by Esc, its ✕, the backdrop or [`close_dialog`].
pub struct DialogClosed;

pub(super) type OnClose = Rc<dyn Fn(&DialogClosed, &mut Window, &mut App)>;
type Content = Rc<dyn Fn(Div, &mut Window, &mut App) -> Div>;

/// What a dialog shows, rebuilt every frame from the closure given to [`open_dialog`].
pub struct Dialog {
    pub(super) title: Option<SharedString>,
    pub(super) width: Pixels,
    pub(super) margin_top: Option<Pixels>,
    pub(super) content: Option<Content>,
    pub(super) children: Vec<AnyElement>,
    pub(super) footer: Option<AnyElement>,
    pub(super) close_button: bool,
    pub(super) on_close: Option<OnClose>,
    pub(super) entrance: DialogEntrance,
    pub(super) style: StyleRefinement,
}

impl Dialog {
    pub(super) fn new() -> Self {
        Self {
            title: None,
            width: px(448.),
            margin_top: None,
            content: None,
            children: Vec::new(),
            footer: None,
            close_button: true,
            on_close: None,
            entrance: DialogEntrance::Fade,
            style: StyleRefinement::default(),
        }
    }

    /// The heading, and the dialog's accessible name.
    pub fn title(mut self, title: impl Into<SharedString>) -> Self {
        self.title = Some(title.into());
        self
    }

    pub fn w(mut self, width: impl Into<Pixels>) -> Self {
        self.width = width.into();
        self
    }

    /// From the window's top; a tenth of its height by default.
    pub fn margin_top(mut self, margin: impl Into<Pixels>) -> Self {
        self.margin_top = Some(margin.into());
        self
    }

    /// The body built into the dialog's padded column (a view that fills it, like the file picker).
    pub fn content(mut self, f: impl Fn(Div, &mut Window, &mut App) -> Div + 'static) -> Self {
        self.content = Some(Rc::new(f));
        self
    }

    pub fn footer(mut self, footer: impl IntoElement) -> Self {
        self.footer = Some(footer.into_any_element());
        self
    }

    /// The ✕ in the corner, named "Close <title>"; on by default.
    pub fn close_button(mut self, show: bool) -> Self {
        self.close_button = show;
        self
    }

    pub fn on_close(mut self, f: impl Fn(&DialogClosed, &mut Window, &mut App) + 'static) -> Self {
        self.on_close = Some(Rc::new(f));
        self
    }

    pub fn entrance(mut self, entrance: DialogEntrance) -> Self {
        self.entrance = entrance;
        self
    }
}

impl ParentElement for Dialog {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl Styled for Dialog {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

/// Opens a dialog over `window`. `build` runs every frame it is drawn, the fade out included.
pub fn open_dialog(
    window: &mut Window,
    cx: &mut App,
    build: impl Fn(Dialog, &mut Window, &mut App) -> Dialog + 'static,
) {
    let Some(layer) = DialogLayer::of(window, cx) else {
        debug_assert!(
            false,
            "open_dialog: this window wasn't built with ui::window_root"
        );
        tracing::error!("open_dialog: the window has no ui::WindowRoot; no dialog opened");
        return;
    };
    super::tooltip::dialog_opened(window, cx);
    layer.update(cx, |layer, cx| layer.open(Rc::new(build), window, cx));
}

/// Closes the top dialog in `window`.
pub fn close_dialog(window: &mut Window, cx: &mut App) {
    if let Some(layer) = DialogLayer::of(window, cx) {
        layer.update(cx, |layer, cx| layer.close_top(window, cx));
    }
}

/// Whether a dialog is open in `window` (one fading out no longer counts).
pub fn has_active_dialog(window: &mut Window, cx: &mut App) -> bool {
    DialogLayer::of(window, cx).is_some_and(|layer| layer.read(cx).is_open())
}

pub(super) fn heading(title: SharedString) -> gpui::Stateful<Div> {
    div()
        .id(SharedString::from(format!("dialog-title:{title}")))
        .role(gpui::accesskit::Role::Heading)
        .aria_label(title.clone())
        .text_size(BODY)
        .font_weight(FontWeight::SEMIBOLD)
        .child(title)
}

/// A dialog footer's Cancel.
pub fn dialog_cancel(id: &'static str) -> Button {
    Button::new(id)
        .outline()
        .small()
        .label("Cancel")
        .on_click(|_, window, cx| close_dialog(window, cx))
}

/// The footer row: buttons at the end, 8 px apart. Pass it to [`Dialog::footer`], or end a custom dialog body with it.
pub fn dialog_footer() -> Div {
    h_flex().w_full().justify_end().gap_2()
}
