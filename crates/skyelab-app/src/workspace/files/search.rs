//! The Search tab (mod+Shift+F): file contents of the thread's folder, grouped by file with line previews (T3's
//! project content search). The search runs off the UI thread; a newer query cancels the one before it.

use crate::ui;
use crate::ui::ActiveTheme as _;
use crate::ui::SkyeIcon;
use crate::ui::TextField;
use crate::ui::dock::{BasePanel, Panel, PanelEvent};
use crate::ui::{Icon, h_flex, v_flex};
use gpui::{
    App, Context, Entity, EventEmitter, FocusHandle, Focusable, IntoElement, SharedString,
    Subscription, Task, Window, div, prelude::*, px, uniform_list,
};
use gpui_component::input::{InputEvent, InputState};
use skyelab_core::files::{Found, search};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

/// How many matching lines one search lists at most.
pub(in crate::workspace) const MAX_MATCHES: usize = 500;
const ROW: gpui::Pixels = px(28.);

/// A match was clicked: preview `path` of `root` at `line`.
pub(in crate::workspace) struct PreviewAt {
    pub root: PathBuf,
    pub path: PathBuf,
    pub line: u32,
}

enum Row {
    File {
        path: SharedString,
        count: usize,
    },
    Match {
        file: usize,
        line: u32,
        text: SharedString,
    },
}

pub(in crate::workspace) struct SearchPanel {
    focus: FocusHandle,
    pub(in crate::workspace) root: PathBuf,
    pub(in crate::workspace) input: Entity<InputState>,
    pub(in crate::workspace) found: Option<Found>,
    rows: Vec<Row>,
    error: Option<SharedString>,
    searching: bool,
    /// Set to stop the search in flight.
    cancel: Arc<AtomicBool>,
    _search: Option<Task<()>>,
    _input: Subscription,
}

impl SearchPanel {
    pub fn new(root: PathBuf, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let input = cx.new(|cx| InputState::new(window, cx).placeholder("Search in files"));
        let _input = cx.subscribe(&input, |this, _, e: &InputEvent, cx| {
            if matches!(e, InputEvent::Change) {
                this.run(Duration::from_millis(150), cx);
            }
        });
        Self {
            focus: cx.focus_handle(),
            root,
            input,
            found: None,
            rows: Vec::new(),
            error: None,
            searching: false,
            cancel: Arc::default(),
            _search: None,
            _input,
        }
    }

    /// Points the tab at another folder and searches it again.
    pub fn set_root(&mut self, root: PathBuf, cx: &mut Context<Self>) {
        if root != self.root {
            self.root = root;
            // Rows found under the old folder would open the wrong file under the new one.
            self.found = None;
            self.rows.clear();
            self.run(Duration::ZERO, cx);
        }
    }

    pub fn focus_input(&self, window: &mut Window, cx: &mut Context<Self>) {
        self.input.update(cx, |i, cx| i.focus(window, cx));
    }

    /// Searches for the input's text after `wait` (typing settles), cancelling the search before it.
    fn run(&mut self, wait: Duration, cx: &mut Context<Self>) {
        self.cancel.store(true, Ordering::Relaxed);
        let cancel = Arc::new(AtomicBool::new(false));
        self.cancel = cancel.clone();
        let query = self.input.read(cx).value().to_string();
        let root = self.root.clone();
        self.searching = !query.is_empty();
        self.error = None;
        if query.is_empty() {
            self.found = None;
            self.rows.clear();
            self._search = None;
            cx.notify();
            return;
        }
        let timer = cx.background_executor().timer(wait);
        self._search = Some(cx.spawn(async move |this, cx| {
            timer.await;
            let found = cx
                .background_spawn(async move { search(&root, &query, MAX_MATCHES, &cancel) })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.searching = false;
                match found {
                    Ok(found) => this.show(found),
                    Err(e) => this.error = Some(format!("{e:#}").into()),
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    fn show(&mut self, found: Found) {
        self.rows.clear();
        for (file, f) in found.files.iter().enumerate() {
            self.rows.push(Row::File {
                path: f.path.clone().into(),
                count: f.lines.len(),
            });
            self.rows.extend(f.lines.iter().map(|l| Row::Match {
                file,
                line: l.line,
                text: l.text.clone().into(),
            }));
        }
        self.found = Some(found);
    }

    pub(in crate::workspace) fn open_match(
        &mut self,
        file: usize,
        line: u32,
        cx: &mut Context<Self>,
    ) {
        let Some(f) = self.found.as_ref().and_then(|f| f.files.get(file)) else {
            return;
        };
        cx.emit(PreviewAt {
            root: self.root.clone(),
            path: PathBuf::from(&f.path),
            line,
        });
    }

    fn status(&self) -> Option<SharedString> {
        if let Some(e) = &self.error {
            return Some(e.clone());
        }
        if self.searching {
            return Some("Searching…".into());
        }
        let found = self.found.as_ref()?;
        let lines: usize = found.files.iter().map(|f| f.lines.len()).sum();
        let plus = if found.truncated { "+" } else { "" };
        Some(match (lines, found.files.len()) {
            (0, _) => "No results found.".into(),
            (1, _) => "1 result in 1 file".into(),
            (n, 1) => format!("{n}{plus} results in 1 file").into(),
            (n, f) => format!("{n}{plus} results in {f} files").into(),
        })
    }

    fn render_rows(
        &mut self,
        range: std::ops::Range<usize>,
        cx: &mut Context<Self>,
    ) -> Vec<gpui::AnyElement> {
        let theme = cx.theme();
        range
            .filter_map(|ix| self.rows.get(ix).map(|r| (ix, r)))
            .map(|(ix, row)| match row {
                Row::File { path, count } => {
                    let (dir, name) = path.rsplit_once('/').unwrap_or(("", path));
                    h_flex()
                        .h(ROW)
                        .px_3()
                        .gap_2()
                        .text_size(ui::META)
                        .child(
                            Icon::new(SkyeIcon::File)
                                .size_3p5()
                                .text_color(theme.muted_foreground),
                        )
                        .child(
                            div()
                                .font_weight(gpui::FontWeight::MEDIUM)
                                .child(SharedString::from(name.to_owned())),
                        )
                        .child(
                            div()
                                .min_w_0()
                                .flex_1()
                                .truncate()
                                .text_color(theme.muted_foreground)
                                .child(SharedString::from(dir.to_owned())),
                        )
                        .child(
                            div()
                                .px_1p5()
                                .rounded_full()
                                .bg(theme.muted)
                                .text_size(ui::MICRO)
                                .text_color(theme.muted_foreground)
                                .child(count.to_string()),
                        )
                        .into_any_element()
                }
                &Row::Match {
                    file,
                    line,
                    ref text,
                } => h_flex()
                    .id(("search-match", ix))
                    .role(gpui::accesskit::Role::Button)
                    .aria_label(format!("Line {line}: {text}"))
                    .h(ROW)
                    .px_3()
                    .gap_3()
                    .font_family(theme.mono_font_family.clone())
                    .text_size(ui::META)
                    .cursor_pointer()
                    .hover(|el| el.bg(theme.accent))
                    .on_click(cx.listener(move |this, _, _, cx| this.open_match(file, line, cx)))
                    .child(
                        div()
                            .w(px(40.))
                            .flex_none()
                            .text_right()
                            .text_color(theme.muted_foreground)
                            .child(line.to_string()),
                    )
                    .child(div().min_w_0().flex_1().truncate().child(text.clone()))
                    .into_any_element(),
            })
            .collect()
    }
}

impl Drop for SearchPanel {
    // Dropping the task doesn't stop a walk already running on a background thread.
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

impl EventEmitter<PanelEvent> for SearchPanel {}
impl EventEmitter<PreviewAt> for SearchPanel {}

impl BasePanel for SearchPanel {
    fn panel_name(&self) -> &'static str {
        "FileSearch"
    }
    fn zoomable(&self, _: &App) -> bool {
        false
    }
}

impl Panel for SearchPanel {
    fn title(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        crate::workspace::tab_context_menu::tab_title(
            cx.entity_id().into(),
            Some("Search".into()),
            "Search",
        )
    }
    fn zoom_control(&self, _: &App) -> Option<crate::ui::dock::PanelControl> {
        None
    }
}

impl Focusable for SearchPanel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for SearchPanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let _render = crate::debug_renders::count("SearchPanel");
        let theme = cx.theme();
        let status = self.status();
        let empty = self.rows.is_empty();
        let list = uniform_list(
            "search-results",
            self.rows.len(),
            cx.processor(|this, range, _, cx| this.render_rows(range, cx)),
        )
        .flex_1()
        .min_h_0()
        .py_1();
        v_flex()
            .id("file-search")
            .track_focus(&self.focus)
            .size_full()
            .bg(theme.background)
            .child(
                div().p_2().child(
                    TextField::new(&self.input)
                        .small()
                        .cleanable(true)
                        .accessibility_label("Search files")
                        .prefix(
                            Icon::new(SkyeIcon::Search)
                                .size_3p5()
                                .text_color(theme.muted_foreground),
                        ),
                ),
            )
            .children(status.map(|s| {
                h_flex()
                    .h(px(36.))
                    .flex_none()
                    .px_3()
                    .border_b_1()
                    .border_color(theme.border)
                    .text_size(ui::META)
                    .text_color(if self.error.is_some() {
                        theme.danger
                    } else {
                        theme.muted_foreground
                    })
                    .child(s)
            }))
            .map(|el| {
                if empty {
                    el.child(
                        div()
                            .flex_1()
                            .flex()
                            .items_center()
                            .justify_center()
                            .px_6()
                            .text_size(ui::BODY)
                            .text_color(theme.muted_foreground)
                            .when(self.found.is_none() && !self.searching, |el| {
                                el.child("Type to search across this thread's folder.")
                            }),
                    )
                } else {
                    el.child(list)
                }
            })
    }
}
