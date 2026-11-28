//! The file picker (mod+P): a palette-style fuzzy finder over the thread's folder (T3's ProjectFilePicker). Enter
//! previews the file; mod+Enter opens it in the user's editor.

use crate::palette::score;
use crate::ui;
use crate::ui::ActiveTheme as _;
use crate::ui::SkyeIcon;
use crate::ui::{Icon, h_flex};
use gpui::{
    App, AppContext as _, Context, Entity, FocusHandle, IntoElement, KeyBinding,
    ParentElement as _, Render, SharedString, Styled as _, Task, Window, actions, div, prelude::*,
    px,
};
use gpui_component::command::{Command, CommandItem, CommandState};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;

actions!(skyelab, [OpenPickedInEditor]);

/// How many files the list shows (T3's limit).
pub(in crate::workspace) const MAX_ROWS: usize = 200;
/// How many files a folder lists at most; past this the picker only finds the first ones.
const MAX_FILES: usize = 100_000;
const CONTEXT: &str = "FilePicker";

pub(in crate::workspace) fn bind_keys(cx: &mut App) {
    let key = if cfg!(target_os = "macos") {
        "cmd-enter"
    } else {
        "ctrl-enter"
    };
    cx.bind_keys([
        KeyBinding::new(key, OpenPickedInEditor, Some(CONTEXT)),
        KeyBinding::new(
            super::preview::SAVE_KEY,
            super::preview::SaveFile,
            Some(super::preview::PREVIEW_CONTEXT),
        ),
    ]);
}

/// Runs a pick: the path (relative to the root) and whether it goes to the editor.
pub(in crate::workspace) type OnPick = Rc<dyn Fn(PathBuf, bool, &mut Window, &mut App)>;

pub(in crate::workspace) struct FilePicker {
    state: Entity<CommandState>,
    pub(in crate::workspace) files: Arc<Vec<String>>,
    /// Indexes into `files`, as listed.
    pub(in crate::workspace) shown: Vec<usize>,
    prev: FocusHandle,
    on_pick: OnPick,
    _rank: Option<Task<()>>,
}

/// Indexes of the files `query` fuzzy-matches, best first, at most `limit`. The file name counts more than its folder.
pub(in crate::workspace) fn rank_files(query: &str, files: &[String], limit: usize) -> Vec<usize> {
    let query = query.trim().trim_start_matches(['@', '.', '/']);
    if query.is_empty() {
        // Dotfiles and files under dot folders last: they are seldom what you open.
        let hidden = |i: &usize| files[*i].split('/').any(|c| c.starts_with('.'));
        let (dots, rest): (Vec<usize>, Vec<usize>) = (0..files.len()).partition(hidden);
        return rest.into_iter().chain(dots).take(limit).collect();
    }
    let mut scored: Vec<(i32, usize)> = files
        .iter()
        .enumerate()
        .filter_map(|(i, f)| {
            let name = f.rsplit('/').next().unwrap_or(f);
            let by_name = score(query, name).map(|s| s + 20);
            by_name.max(score(query, f)).map(|s| (s, i))
        })
        .collect();
    // Ties keep the shorter path first.
    scored.sort_by_key(|&(s, i)| (std::cmp::Reverse(s), files[i].len(), i));
    scored.into_iter().take(limit).map(|(_, i)| i).collect()
}

/// Lists `root`'s files off the UI thread and opens the picker over them.
pub(in crate::workspace) fn open(
    root: PathBuf,
    prev: FocusHandle,
    on_pick: OnPick,
    window: &mut Window,
    cx: &mut App,
) -> Entity<FilePicker> {
    let state = cx.new(|cx| CommandState::new(window, cx));
    let picker = cx.new(|_| FilePicker {
        state: state.clone(),
        files: Arc::default(),
        shown: Vec::new(),
        prev,
        on_pick,
        _rank: None,
    });
    let list = cx.background_spawn(async move {
        // A folder git doesn't know (or no git) is walked instead, still skipping ignored files.
        skyelab_git::project_files(&root)
            .map(|mut f| {
                f.truncate(MAX_FILES);
                f
            })
            .unwrap_or_else(|_| skyelab_core::files::walk(&root, MAX_FILES))
    });
    state.update(cx, |s, cx| s.set_loading(true, window, cx));
    let weak = picker.downgrade();
    window
        .spawn(cx, async move |cx| {
            let files = list.await;
            let _ = weak.update_in(cx, |p, window, cx| {
                p.files = Arc::new(files);
                p.state.update(cx, |s, cx| s.set_loading(false, window, cx));
                let query = p.state.read(cx).query(cx).to_string();
                p.rank(query, cx);
            });
        })
        .detach();
    let shown = picker.clone();
    crate::ui::open_dialog(window, cx, move |dialog, _, _| {
        let picker = shown.clone();
        dialog
            .w(px(560.))
            .close_button(false)
            .content(move |content, _, _| content.child(picker.clone()))
    });
    state.update(cx, |state, cx| state.focus(window, cx));
    picker
}

impl FilePicker {
    /// Ranks the files for `query` off the UI thread; a newer query drops the older ranking.
    fn rank(&mut self, query: String, cx: &mut Context<Self>) {
        let files = self.files.clone();
        let ranked = cx.background_spawn(async move { rank_files(&query, &files, MAX_ROWS) });
        self._rank = Some(cx.spawn(async move |p, cx| {
            let shown = ranked.await;
            let _ = p.update(cx, |p, cx| {
                p.shown = shown;
                cx.notify();
            });
        }));
    }

    /// Picks row `row` (of what's listed): closes the dialog, puts focus back, then runs the pick.
    pub(in crate::workspace) fn pick(
        &mut self,
        row: usize,
        editor: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(path) = self.shown.get(row).and_then(|&i| self.files.get(i)) else {
            return;
        };
        let (path, on_pick, prev) = (PathBuf::from(path), self.on_pick.clone(), self.prev.clone());
        crate::ui::close_dialog(window, cx);
        window.focus(&prev, cx);
        on_pick(path, editor, window, cx);
    }

    fn pick_in_editor(
        &mut self,
        _: &OpenPickedInEditor,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(ix) = self.state.read(cx).selected_index() {
            self.pick(ix.row, true, window, cx);
        }
    }
}

fn row(path: &str, cx: &App) -> gpui::Div {
    let theme = cx.theme();
    let (dir, name) = path.rsplit_once('/').unwrap_or(("", path));
    h_flex()
        .w_full()
        .min_w_0()
        .gap_2()
        .child(
            Icon::new(SkyeIcon::File)
                .size_3p5()
                .text_color(theme.muted_foreground),
        )
        .child(
            div()
                .flex_none()
                .font_weight(gpui::FontWeight::MEDIUM)
                .child(SharedString::from(name.to_owned())),
        )
        .child(
            div()
                .min_w_0()
                .truncate()
                .text_size(ui::META)
                .text_color(theme.muted_foreground)
                .child(SharedString::from(dir.to_owned())),
        )
}

impl Render for FilePicker {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let _render = crate::debug_renders::count("FilePicker");
        let items = self
            .shown
            .iter()
            .filter_map(|&i| self.files.get(i))
            .map(|path| {
                let path = path.clone();
                CommandItem::new()
                    .label(path.clone())
                    .child(move |_, cx| row(&path, cx))
            });
        let this = cx.entity().downgrade();
        let on_query = this.clone();
        div()
            .key_context(CONTEXT)
            .on_action(cx.listener(Self::pick_in_editor))
            .child(
                Command::new(&self.state)
                    .filterable(false)
                    .bordered(false)
                    .placeholder("Search files")
                    .items(items)
                    // The palette's key hints, so both pickers look alike.
                    .footer(|_, _, cx| crate::palette::footer(false, false, cx))
                    .empty(|state, _, _| {
                        let text = if state.is_loading() {
                            "Listing files…"
                        } else {
                            "No matching files."
                        };
                        div().py_6().text_center().text_size(ui::BODY).child(text)
                    })
                    .on_query(move |query, _, cx| {
                        let query = query.to_owned();
                        _ = on_query.update(cx, |p, cx| p.rank(query, cx));
                    })
                    .on_confirm(move |ix, window, cx| {
                        _ = this.update(cx, |p, cx| p.pick(ix.row, false, window, cx));
                    }),
            )
    }
}
