//! The large image dialog (T3's `chat/ExpandedImageDialog.tsx`): an image the thread still holds, on the dimmed
//! backdrop, fitted to the window, with zoom steps over that fit. Esc or a click on the backdrop closes it
//! (gpui-component's `Dialog`). Sources: a user message's image attachments sent this session (kept in memory
//! only, never stored) and the Preview tab's image file.

use crate::ui;
use crate::ui::Button;
use crate::ui::SkyeIcon;
use gpui::{
    AnyElement, App, AppContext as _, Context, Image, ImageFormat, ImageSource, IntoElement,
    ObjectFit, SharedString, Window, div, img, prelude::*, px,
};
use gpui_component::{ActiveTheme as _, Icon, WindowExt as _, h_flex, v_flex};
use skyelab_agent::{Attachment, AttachmentKind};
use std::sync::Arc;

/// Zoom steps over the fitted size; step 0 is Fit.
pub(in crate::workspace) const ZOOMS: [f32; 6] = [1.0, 1.5, 2.0, 3.0, 4.0, 6.0];

/// A sent message's images, named, for its thumbnails.
pub(in crate::workspace) type Sent = Vec<(SharedString, Arc<Image>)>;

/// The image attachments of a message, as the viewer shows them. Other kinds are left out.
pub(in crate::workspace) fn images_of(attachments: &[Attachment]) -> Sent {
    attachments
        .iter()
        .filter_map(|a| match a.kind {
            AttachmentKind::Image(mime) => {
                let format = ImageFormat::from_mime_type(mime)?;
                let image = Image::from_bytes(format, a.data.clone());
                Some((a.name.clone().into(), Arc::new(image)))
            }
            _ => None,
        })
        .collect()
}

/// The next zoom step from `ix` toward `dir` (−1 out, +1 in), held at the ends.
pub(in crate::workspace) fn zoom_step(ix: usize, dir: i32) -> usize {
    ix.saturating_add_signed(dir as isize).min(ZOOMS.len() - 1)
}

pub(in crate::workspace) struct ImageViewer {
    /// Bytes, never a path: GPUI caches a path's image for the session, so an edited file would show stale.
    shown: Arc<Image>,
    name: SharedString,
    zoom: usize,
}

impl ImageViewer {
    fn zoom(&mut self, dir: i32, cx: &mut Context<Self>) {
        self.zoom = if dir == 0 {
            0
        } else {
            zoom_step(self.zoom, dir)
        };
        cx.notify();
    }
}

/// Opens `shown` in the dialog over the window.
pub(in crate::workspace) fn open(
    shown: Arc<Image>,
    name: SharedString,
    window: &mut Window,
    cx: &mut App,
) {
    let viewer = cx.new(|_| ImageViewer {
        shown,
        name,
        zoom: 0,
    });
    crate::ui::open_dialog(window, cx, move |dialog, window, _| {
        let viewer = viewer.clone();
        dialog
            .w(window.viewport_size().width * 0.92)
            .margin_top(px(24.))
            .p_2()
            .close_button(false)
            .content(move |content, _, _| content.child(viewer.clone()))
    });
}

impl Render for ImageViewer {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let _render = crate::debug_renders::count("ImageViewer");
        let theme = cx.theme();
        let viewport = window.viewport_size();
        let (w, h) = (viewport.width * 0.92 - px(16.), viewport.height * 0.8);
        let z = ZOOMS[self.zoom];
        let label = format!("{}%", (z * 100.) as u32);
        let button = |id: &'static str, icon: Icon, tip: &'static str| {
            Button::new(id).ghost().xsmall().icon(icon).tooltip(tip)
        };
        v_flex()
            .id("image-viewer")
            .gap_2()
            .child(
                h_flex()
                    .h(px(28.))
                    .px_1()
                    .gap_1()
                    .text_size(ui::META)
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .font_weight(gpui::FontWeight::MEDIUM)
                            .child(self.name.clone()),
                    )
                    .child(
                        button("image-zoom-out", Icon::new(SkyeIcon::Minus), "Zoom out")
                            .disabled(self.zoom == 0)
                            .on_click(cx.listener(|this, _, _, cx| this.zoom(-1, cx))),
                    )
                    .child(
                        div()
                            .w(px(40.))
                            .text_center()
                            .text_color(theme.muted_foreground)
                            .child(label),
                    )
                    .child(
                        button("image-zoom-in", Icon::new(SkyeIcon::Plus), "Zoom in")
                            .disabled(self.zoom == ZOOMS.len() - 1)
                            .on_click(cx.listener(|this, _, _, cx| this.zoom(1, cx))),
                    )
                    .child(
                        Button::new("image-fit")
                            .ghost()
                            .xsmall()
                            .label("Fit")
                            .on_click(cx.listener(|this, _, _, cx| this.zoom(0, cx))),
                    )
                    .child(
                        button("image-close", Icon::new(SkyeIcon::X), "Close (Esc)")
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    ),
            )
            .child(
                div()
                    .id("image-viewer-body")
                    .w(w)
                    .h(h)
                    .rounded(ui::RADIUS)
                    .bg(theme.muted)
                    // Centered while it fits; zoomed, it starts at the top left so every edge scrolls into view.
                    .map(|el| match self.zoom {
                        0 => el.flex().items_center().justify_center(),
                        _ => el.overflow_scroll(),
                    })
                    // img() takes its height from its width and aspect ratio: max_h keeps it to the box.
                    .child(
                        img(self.shown.clone())
                            .flex_none()
                            .w(w * z)
                            .h(h * z)
                            .max_h(h * z)
                            .object_fit(ObjectFit::Contain),
                    ),
            )
    }
}

/// A message's image thumbnails; a click opens the image in the viewer.
pub(in crate::workspace) fn thumbnails(id: u64, images: &Sent) -> AnyElement {
    h_flex()
        .flex_wrap()
        .justify_end()
        .gap_1p5()
        .children(images.iter().enumerate().map(|(i, (name, image))| {
            let (name, image) = (name.clone(), image.clone());
            div()
                .id(SharedString::from(format!("sent-image-{id}-{i}")))
                .size(px(72.))
                .rounded(ui::RADIUS)
                .overflow_hidden()
                .cursor_pointer()
                .child(
                    img(ImageSource::from(image.clone()))
                        .size_full()
                        .object_fit(ObjectFit::Cover),
                )
                .on_click(move |_, window, cx| open(image.clone(), name.clone(), window, cx))
        }))
        .into_any_element()
}

impl super::thread::ThreadView {
    /// Keeps a sent message's images for its thumbnails.
    // shortcut: keyed by the message's shown text, so two identical messages show the later one's images; key by
    // row if that ever matters.
    pub(in crate::workspace) fn keep_images(&mut self, shown: &str, attachments: &[Attachment]) {
        let images = images_of(attachments);
        if !images.is_empty() {
            self.sent_images.insert(shown.to_owned(), images);
        }
    }
}
