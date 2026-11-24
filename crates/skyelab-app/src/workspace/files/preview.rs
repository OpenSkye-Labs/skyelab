//! The Preview tab: one file of the thread's folder, read-only. Text shows in gpui-component's code editor (line
//! numbers, syntect highlighting) at the asked line; images draw as images; binary and huge files get a note.

use crate::ui;
use anyhow::Context as _;
use gpui::{
    App, Context, Entity, EventEmitter, FocusHandle, Focusable, IntoElement, ObjectFit,
    SharedString, Task, Window, div, img, prelude::*,
};
use gpui_component::{
    ActiveTheme as _, IconName, Sizable as _,
    button::{Button, ButtonVariants as _},
    dock::{BasePanel, Panel, PanelEvent},
    h_flex,
    input::{Editor, EditorState, Position},
    v_flex,
};
use skyelab_core::files::Preview;
use std::path::{Path, PathBuf};
use std::sync::Arc;

#[cfg(test)]
#[path = "../preview_image_cache_test.rs"]
mod image_cache_test;
#[cfg(test)]
#[path = "../preview_image_test.rs"]
mod image_test;

/// The preview's header asked to open its file in the user's editor.
pub(in crate::workspace) struct OpenInEditor {
    pub root: PathBuf,
    pub path: PathBuf,
}

enum Body {
    Loading,
    Text,
    /// Its bytes as read: GPUI keys an image by its content, so an edited file draws anew (a path is cached for good).
    Image(Arc<gpui::Image>),
    Note(SharedString),
}

pub(in crate::workspace) struct PreviewPanel {
    focus: FocusHandle,
    pub(in crate::workspace) root: PathBuf,
    /// As asked: relative to `root`, or absolute.
    pub(in crate::workspace) path: PathBuf,
    body: Body,
    pub(in crate::workspace) editor: Entity<EditorState>,
    _load: Option<Task<()>>,
}

impl PreviewPanel {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        Self {
            focus: cx.focus_handle(),
            root: PathBuf::new(),
            path: PathBuf::new(),
            body: Body::Loading,
            editor: cx.new(|cx| EditorState::new(window, cx)),
            _load: None,
        }
    }

    /// Shows `path` of `root`, scrolled to `line` (1-based) when it is text. A newer call drops the older read.
    pub fn show(
        &mut self,
        root: PathBuf,
        path: PathBuf,
        line: Option<u32>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // GPUI keeps every decoded image until told: the one this replaces goes.
        if let Body::Image(old) = std::mem::replace(&mut self.body, Body::Loading) {
            old.remove_asset(cx);
        }
        self.root = root.clone();
        self.path = path.clone();
        let read = cx.background_spawn(async move {
            let preview = skyelab_core::files::read_preview(&root, &path)?;
            let image = match &preview {
                Preview::Image(full) => Some(image(full)?),
                _ => None,
            };
            anyhow::Ok((preview, image))
        });
        self._load = Some(cx.spawn_in(window, async move |this, cx| {
            let read = read.await;
            let _ = this.update_in(cx, |this, window, cx| this.loaded(read, line, window, cx));
        }));
        cx.notify();
    }

    fn loaded(
        &mut self,
        read: anyhow::Result<(Preview, Option<Arc<gpui::Image>>)>,
        line: Option<u32>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.body = match read {
            Ok((_, Some(image))) => Body::Image(image),
            Ok((Preview::Text(text), _)) => {
                let lang = language(&self.path);
                let row = line.map_or(0, |l| l.saturating_sub(1));
                self.editor.update(cx, |e, cx| {
                    e.set_highlighter(lang, cx);
                    e.set_value(text, window, cx);
                    // Moves the caret there and scrolls it into view (and focuses the preview).
                    e.set_cursor_position(Position::new(row, 0), window, cx);
                });
                Body::Text
            }
            Ok((Preview::Binary | Preview::Image(_), _)) => {
                Body::Note("This is a binary file, so it has no text preview.".into())
            }
            Ok((Preview::TooLarge(n), _)) => Body::Note(
                format!(
                    "This file is too large to preview ({:.1} MB).",
                    n as f64 / 1e6
                )
                .into(),
            ),
            Err(e) => Body::Note(format!("{e:#}").into()),
        };
        cx.notify();
    }

    fn name(&self) -> SharedString {
        self.path.file_name().map_or_else(
            || "Preview".into(),
            |n| n.to_string_lossy().into_owned().into(),
        )
    }
}

/// The image file `full` (already checked by `read_preview`: inside the folder, at most its size cap), read once. Its
/// format comes from its bytes, as GPUI's path loader did, and is one of `read_preview`'s rasters: never SVG (it can
/// load files from its hrefs).
pub(in crate::workspace) fn image(full: &Path) -> anyhow::Result<Arc<gpui::Image>> {
    // The cap again: the file may have grown since it was checked.
    let mut bytes = Vec::new();
    std::io::Read::read_to_end(
        &mut std::io::Read::take(
            std::fs::File::open(full).context("Can't read this file.")?,
            skyelab_core::files::MAX_IMAGE_BYTES + 1,
        ),
        &mut bytes,
    )
    .context("Can't read this file.")?;
    if bytes.len() as u64 > skyelab_core::files::MAX_IMAGE_BYTES {
        anyhow::bail!("This file is too large to preview.");
    }
    use gpui::ImageFormat as F;
    let format = match bytes.as_slice() {
        [0x89, b'P', b'N', b'G', ..] => F::Png,
        [0xFF, 0xD8, 0xFF, ..] => F::Jpeg,
        [b'G', b'I', b'F', b'8', ..] => F::Gif,
        [
            b'R',
            b'I',
            b'F',
            b'F',
            _,
            _,
            _,
            _,
            b'W',
            b'E',
            b'B',
            b'P',
            ..,
        ] => F::Webp,
        [b'B', b'M', ..] => F::Bmp,
        _ => anyhow::bail!("This file isn't an image this preview draws."),
    };
    Ok(Arc::new(gpui::Image::from_bytes(format, bytes)))
}

/// The highlighter's language for a file: its extension, or its name for `Makefile`, `Dockerfile` and the like.
fn language(path: &Path) -> String {
    path.extension()
        .or_else(|| path.file_name())
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default()
}

impl EventEmitter<PanelEvent> for PreviewPanel {}
impl EventEmitter<OpenInEditor> for PreviewPanel {}

impl BasePanel for PreviewPanel {
    fn panel_name(&self) -> &'static str {
        "FilePreview"
    }
    fn zoomable(&self, _: &App) -> bool {
        false
    }
    fn on_removed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.path.as_os_str().is_empty() {
            let (root, path) = (self.root.clone(), self.path.clone());
            crate::workspace::reopen::remember(
                window,
                crate::workspace::reopen::Closed::Preview { root, path },
                cx,
            );
        }
    }
}

impl Panel for PreviewPanel {
    fn title(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        self.name()
    }
    fn zoom_control(&self, _: &App) -> Option<gpui_component::dock::PanelControl> {
        None
    }
}

impl Focusable for PreviewPanel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for PreviewPanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        crate::debug_renders::count("PreviewPanel");
        let theme = cx.theme();
        let shown = self.path.strip_prefix(&self.root).unwrap_or(&self.path);
        let dir = shown
            .parent()
            .map(|d| d.to_string_lossy().into_owned())
            .unwrap_or_default();
        let header = h_flex()
            .h(ui::ROW_H)
            .flex_none()
            .px_3()
            .gap_2()
            .border_b_1()
            .border_color(theme.border)
            .text_size(ui::META)
            .child(
                div()
                    .font_weight(gpui::FontWeight::MEDIUM)
                    .child(self.name()),
            )
            .child(
                div()
                    .min_w_0()
                    .flex_1()
                    .truncate()
                    .text_color(theme.muted_foreground)
                    .child(dir),
            )
            .child(
                Button::new("preview-open-editor")
                    .ghost()
                    .xsmall()
                    .icon(IconName::ExternalLink)
                    .tooltip("Open in editor")
                    .on_click(cx.listener(|this, _, _, cx| {
                        cx.emit(OpenInEditor {
                            root: this.root.clone(),
                            path: this.path.clone(),
                        })
                    })),
            );
        let note = |text: SharedString| {
            div()
                .p_4()
                .text_size(ui::BODY)
                .text_color(theme.muted_foreground)
                .child(text)
        };
        let body = match &self.body {
            Body::Loading => note("Loading…".into()).into_any_element(),
            Body::Note(text) => note(text.clone()).into_any_element(),
            Body::Image(image) => {
                let (shown, name) = (image.clone(), self.name());
                div()
                    .id("preview-image")
                    .size_full()
                    .p_4()
                    .cursor_pointer()
                    .child(
                        img(image.clone())
                            .size_full()
                            .object_fit(ObjectFit::Contain),
                    )
                    .on_click(move |_, window, cx| {
                        crate::workspace::image_viewer::open(
                            shown.clone(),
                            name.clone(),
                            window,
                            cx,
                        )
                    })
                    .into_any_element()
            }
            Body::Text => Editor::new(&self.editor)
                .readonly(true)
                .bordered(false)
                .appearance(false)
                .h_full()
                .into_any_element(),
        };
        v_flex()
            .id("file-preview")
            .track_focus(&self.focus)
            .size_full()
            .bg(theme.background)
            .child(header)
            .child(div().flex_1().min_h_0().child(body))
    }
}
