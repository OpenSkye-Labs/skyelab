//! The Preview tab: one file of the thread's folder. Text shows in gpui-component's code editor (line numbers, syntect
//! highlighting) at the asked line, read-only until Edit is on (`preview_edit`); images draw as images; binary and huge
//! files get a note.
//! mod+F finds in the text with the thread's find bar; the editor's own matcher counts and highlights the matches.

use crate::ui;
use crate::ui::Button;
use crate::ui::SkyeIcon;
use crate::workspace::thread_find::{
    CloseFind, Find, FindInThread, FindNext, FindPrevious, find_bar, open_bar,
};
use anyhow::Context as _;
use gpui::{
    App, Context, Entity, EventEmitter, FocusHandle, Focusable, IntoElement, ObjectFit,
    SharedString, Task, Window, div, img, prelude::*,
};
use gpui_component::{
    ActiveTheme as _,
    dock::{BasePanel, Panel, PanelEvent},
    h_flex,
    input::{Editor, EditorState, Position},
    v_flex,
};
use skyelab_core::files::Preview;
use std::path::{Path, PathBuf};
use std::sync::Arc;

#[path = "preview_edit.rs"]
mod edit;
pub(in crate::workspace) use edit::{SAVE_KEY, SaveFile};

#[cfg(all(test, unix))] // the workspace fixture runs the fake agent, a shell script
#[path = "../preview_edit_test.rs"]
mod edit_test;
#[cfg(all(test, unix))] // the workspace fixture runs the fake agent, a shell script
#[path = "../preview_find_test.rs"]
mod find_test;
#[cfg(test)]
#[path = "../preview_image_cache_test.rs"]
mod image_cache_test;
#[cfg(test)]
#[path = "../preview_image_test.rs"]
mod image_test;

/// The key context of a focused preview: mod+F there opens its find bar.
pub(in crate::workspace) const PREVIEW_CONTEXT: &str = "FilePreview";

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
    /// The find bar, while open. `matches` holds each match's byte offset; the editor keeps the current one.
    find: Find,
    edit: edit::Edit,
    _load: Option<Task<()>>,
}

impl PreviewPanel {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        // Not its own search panel (a library default): mod+F goes on to the thread's find bar.
        let editor = cx.new(|cx| EditorState::new(window, cx).searchable(false));
        let mut edit = edit::Edit::default();
        edit.sub = Some(cx.subscribe(&editor, |this, _, e, cx| this.editor_event(e, cx)));
        Self {
            focus: cx.focus_handle(),
            root: PathBuf::new(),
            path: PathBuf::new(),
            body: Body::Loading,
            editor,
            find: Find::default(),
            edit,
            _load: None,
        }
    }

    /// Shows `path` of `root`, scrolled to `line` (1-based) when it is text. A newer call drops the older read.
    /// Unsaved edits of the file shown are asked about first (Save / Discard / Cancel).
    pub fn show(
        &mut self,
        root: PathBuf,
        path: PathBuf,
        line: Option<u32>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.edit.dirty {
            return self.leave(
                Box::new(move |this, window, cx| this.show(root, path, line, window, cx)),
                window,
                cx,
            );
        }
        // GPUI keeps every decoded image until told: the one this replaces goes.
        if let Body::Image(old) = std::mem::replace(&mut self.body, Body::Loading) {
            old.remove_asset(cx);
        }
        if self.root != root || self.edit.watch.is_none() {
            self.edit.watch = super::super::folder_watch::watch(&root, cx, Self::disk_changed);
        }
        if self.path != path {
            self.edit.on = false; // another file opens read-only
        }
        self.edit.base = None;
        self.root = root.clone();
        self.path = path.clone();
        self.end_find(cx);
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
                let lang = crate::ui::syntax::language(&self.path);
                let row = line.map_or(0, |l| l.saturating_sub(1));
                self.editor.update(cx, |e, cx| {
                    e.set_highlighter(lang, cx);
                    e.set_value(text.clone(), window, cx);
                    // Moves the caret there and scrolls it into view (and focuses the preview).
                    e.set_cursor_position(Position::new(row, 0), window, cx);
                });
                self.text_loaded(text, cx);
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

    /// mod+F: the find bar over the text (none for an image or a note), its query selected.
    fn find_in_file(&mut self, _: &FindInThread, window: &mut Window, cx: &mut Context<Self>) {
        if !matches!(self.body, Body::Text) {
            return;
        }
        open_bar(
            &mut self.find,
            "Find in file",
            window,
            cx,
            Self::find_changed,
            Self::find_step,
        );
    }

    /// A new query: the editor counts and highlights it, and shows the match nearest the view.
    fn find_changed(&mut self, cx: &mut Context<Self>) {
        let Some(input) = &self.find.input else {
            return;
        };
        let query = input.read(cx).value().to_string();
        if query == self.find.query {
            return; // Enter reports a change too, with the same text: it keeps the place
        }
        self.editor.update(cx, |e, cx| {
            // shortcut: the editor folds ASCII case only (the thread's find folds all of Unicode); match it here if
            // non-ASCII code needs it.
            e.set_search_query(query.trim(), true, cx);
            // Back and forth lands on the match the query started at, and scrolls to it.
            if e.previous_search_match(cx).is_some() {
                e.next_search_match(cx);
            }
        });
        self.find.query = query;
        self.sync_find(cx);
    }

    /// The next (or previous) match, wrapping around.
    fn find_step(&mut self, forward: bool, cx: &mut Context<Self>) {
        if self.find.input.is_none() {
            return; // no bar, no search to step through
        }
        self.editor.update(cx, |e, cx| match forward {
            true => e.next_search_match(cx),
            false => e.previous_search_match(cx),
        });
        self.sync_find(cx);
    }

    /// The bar's count from the editor's matcher.
    fn sync_find(&mut self, cx: &mut Context<Self>) {
        let m = &self.editor.read(cx).search_session().matcher;
        self.find.matches = m.matched_ranges().iter().map(|r| r.start).collect();
        self.find.active = m.current_match_index();
        cx.notify();
    }

    /// Closes the bar and the editor's highlights.
    fn end_find(&mut self, cx: &mut Context<Self>) {
        if self.find.input.take().is_some() {
            self.find = Find::default();
            self.editor.update(cx, |e, cx| e.close_search(cx));
        }
    }

    fn close_find(&mut self, _: &CloseFind, window: &mut Window, cx: &mut Context<Self>) {
        self.end_find(cx);
        self.editor.update(cx, |e, cx| e.focus(window, cx));
        cx.notify();
    }

    fn find_next(&mut self, _: &FindNext, _: &mut Window, cx: &mut Context<Self>) {
        self.find_step(true, cx);
    }

    fn find_previous(&mut self, _: &FindPrevious, _: &mut Window, cx: &mut Context<Self>) {
        self.find_step(false, cx);
    }

    /// The name, with a dot while edits are unsaved.
    fn title_text(&self) -> SharedString {
        match self.edit.dirty {
            true => format!("{} •", self.name()).into(),
            false => self.name(),
        }
    }

    pub(in crate::workspace) fn name(&self) -> SharedString {
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

impl EventEmitter<PanelEvent> for PreviewPanel {}
impl EventEmitter<OpenInEditor> for PreviewPanel {}

impl BasePanel for PreviewPanel {
    fn panel_name(&self) -> &'static str {
        "FilePreview"
    }
    /// Not while edits are unsaved: no ×, and the tab's Close asks first (`close_panel`).
    fn closable(&self, _: &App) -> bool {
        !self.edit.dirty
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
    fn title(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        crate::workspace::tab_context_menu::tab_title(
            cx.entity_id().into(),
            Some(self.title_text()),
            self.title_text(),
        )
    }
}

impl Focusable for PreviewPanel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for PreviewPanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let _render = crate::debug_renders::count("PreviewPanel");
        let find = find_bar(
            &self.find,
            cx,
            |this, forward, cx| this.find_step(forward, cx),
            |this, window, cx| this.close_find(&CloseFind, window, cx),
        );
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
                    .child(self.title_text()),
            )
            .child(
                div()
                    .min_w_0()
                    .flex_1()
                    .truncate()
                    .text_color(theme.muted_foreground)
                    .child(dir),
            )
            .when(self.can_edit(), |row| {
                row.child(
                    Button::new("preview-edit")
                        .ghost()
                        .xsmall()
                        .icon(SkyeIcon::PenLine)
                        .selected(self.edit.on)
                        .tooltip(if self.edit.on { "Stop editing" } else { "Edit" })
                        .on_click(cx.listener(|this, _, window, cx| this.toggle_edit(window, cx))),
                )
            })
            .child(
                Button::new("preview-open-editor")
                    .ghost()
                    .xsmall()
                    .icon(SkyeIcon::ExternalLink)
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
                    .role(gpui::accesskit::Role::Button)
                    .aria_label(format!("Open image {name}"))
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
                .readonly(!self.edit.on)
                .bordered(false)
                .appearance(false)
                .h_full()
                .into_any_element(),
        };
        v_flex()
            .id("file-preview")
            .key_context(PREVIEW_CONTEXT)
            .track_focus(&self.focus)
            .on_action(cx.listener(Self::find_in_file))
            .on_action(cx.listener(Self::find_next))
            .on_action(cx.listener(Self::find_previous))
            .on_action(cx.listener(Self::close_find))
            .on_action(cx.listener(Self::save_file))
            .on_action(cx.listener(Self::escape))
            .on_action(cx.listener(Self::close_panel))
            .size_full()
            .bg(theme.background)
            .child(header)
            .children(self.banner(cx))
            .children(find)
            .child(div().flex_1().min_h_0().child(body))
    }
}
