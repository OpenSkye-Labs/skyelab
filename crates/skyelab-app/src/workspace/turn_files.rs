//! The files a finished turn changed, under its reply (T3's changed-files card in `TimelineAssistantRows`): read from
//! the turn's checkpoint diff, never from a new walk of the tree. Turn divider k is checkpoint turn k, which holds
//! only while the thread has as many dividers as checkpointed turns. A crash or rerun that took a checkpoint
//! without a divider, or a turn that took none (the folder wasn't a repo yet), turns the rows off rather than list
//! another turn's files.

use super::thread::ThreadView;
use super::thread_items::ItemsView;
use super::thread_model::Item;
use crate::ui;
use gpui::{AnyElement, Context, FontWeight, SharedString, Task, div, prelude::*, px};
use gpui_component::{
    ActiveTheme as _, Icon, IconName, Sizable as _,
    button::{Button, ButtonVariants as _},
    h_flex, v_flex,
};
use std::collections::HashMap;
use std::path::PathBuf;

/// One changed file of a turn.
#[derive(Debug, Clone, PartialEq)]
pub struct FileStat {
    /// Absolute: the Changes tab reveals it by its place under the repo root.
    pub path: PathBuf,
    /// Repo-root relative, as shown.
    pub rel: String,
    pub added: usize,
    pub removed: usize,
}

/// A thread view's timeline state beyond its items: turn files, the find bar, a dismissed failure banner.
#[derive(Default)]
pub struct Timeline {
    pub files: TurnFiles,
    pub find: super::thread_find::Find,
    /// The failed row whose banner the user closed.
    pub dismissed_failure: Option<usize>,
}

#[derive(Default)]
pub struct TurnFiles {
    /// By checkpoint turn.
    pub by_turn: HashMap<u32, Vec<FileStat>>,
    /// How many checkpointed turns the thread had at the last load.
    pub turns: usize,
    /// Diffs read, by (from tree, to tree): a pair's diff never changes, so each is read once.
    cache: HashMap<(String, String), Vec<FileStat>>,
    /// How many dividers the thread had when that load landed. A divider newer than it may still wait for its
    /// checkpoint; one counted then without a checkpoint never gets one.
    pub dividers_at_load: usize,
    _task: Option<Task<()>>,
}

/// The checkpoint turn of the divider at row `done`, if the dividers and checkpoints line up: as many of each, or
/// one newer divider whose checkpoint is still being taken (the load before it matched).
pub fn turn_of(items: &[Item], done: usize, turns: usize, dividers_at_load: usize) -> Option<u32> {
    if !matches!(items.get(done)?, Item::Done { .. }) {
        return None;
    }
    let count = |rows: &[Item]| {
        rows.iter()
            .filter(|i| matches!(i, Item::Done { .. }))
            .count()
    };
    let dividers = count(items);
    let pending = dividers == turns + 1 && dividers_at_load == turns;
    (dividers == turns || pending).then(|| count(&items[..=done]) as u32)
}

type Loaded = (
    usize,
    HashMap<u32, Vec<FileStat>>,
    HashMap<(String, String), Vec<FileStat>>,
);

/// Each checkpointed turn's files. Blocking (git): run off the UI thread.
fn load(
    cwd: &std::path::Path,
    key: &str,
    mut cache: HashMap<(String, String), Vec<FileStat>>,
) -> Option<Loaded> {
    let checkpoints = skyelab_git::checkpoints(cwd, key).ok()?;
    let root = skyelab_git::repo_root(cwd).ok()?;
    let tree = |turn: u32| {
        checkpoints
            .iter()
            .find(|c| c.turn == turn)
            .map(|c| c.tree.clone())
    };
    let turns = super::changes_model::turns(&checkpoints);
    let mut by_turn = HashMap::new();
    for &n in &turns {
        let (Some(from), Some(to)) = (tree(n - 1), tree(n)) else {
            continue;
        };
        let pair = (from, to);
        if !cache.contains_key(&pair) {
            let Ok(diffs) = skyelab_git::diff_trees(cwd, &pair.0, &pair.1) else {
                continue;
            };
            cache.insert(pair.clone(), stats(&root, diffs));
        }
        by_turn.insert(n, cache[&pair].clone());
    }
    Some((turns.len(), by_turn, cache))
}

fn stats(root: &std::path::Path, diffs: Vec<skyelab_git::FileDiff>) -> Vec<FileStat> {
    diffs
        .into_iter()
        .filter_map(|d| {
            let rel = d.new_path.clone().or(d.old_path.clone())?;
            let lines = d.hunks.iter().flat_map(|h| &h.lines);
            let count = |f: fn(&skyelab_git::Line) -> bool| lines.clone().filter(|l| f(l)).count();
            Some(FileStat {
                path: root.join(&rel),
                rel: rel.display().to_string(),
                added: count(|l| matches!(l, skyelab_git::Line::Added(_))),
                removed: count(|l| matches!(l, skyelab_git::Line::Removed(_))),
            })
        })
        .collect()
}

impl ThreadView {
    /// Reads the turns' files again (after a turn's checkpoint, or when the thread opens). A newer load replaces
    /// an older one.
    pub(super) fn load_turn_files(&mut self, cx: &mut Context<Self>) {
        let Some(t) = &self.thread else { return };
        let (cwd, key) = (t.cwd.clone(), super::thread_checkpoints::key(t));
        let cache = self.timeline.files.cache.clone();
        let task = cx.background_spawn(async move { load(&cwd, &key, cache) });
        self.timeline.files._task = Some(cx.spawn(async move |this, cx| {
            let Some((turns, by_turn, cache)) = task.await else {
                return;
            };
            let _ = this.update(cx, |this, cx| {
                let f = &mut this.timeline.files;
                (f.turns, f.by_turn, f.cache) = (turns, by_turn, cache);
                f.dividers_at_load = this
                    .items
                    .iter()
                    .filter(|i| matches!(i, Item::Done { .. }))
                    .count();
                // The dividers grow a row: measure them again.
                for ix in 0..this.items.len() {
                    if matches!(this.items[ix], Item::Done { .. }) {
                        this.lists.splice(ix..ix + 1, 1);
                    }
                }
                cx.notify();
            });
        }));
    }
}

fn stat_label(added: usize, removed: usize, cx: &gpui::App) -> impl IntoElement {
    let theme = cx.theme();
    h_flex()
        .gap_1()
        .flex_shrink_0()
        .font_family(theme.mono_font_family.clone())
        .text_size(ui::META)
        .child(
            div()
                .text_color(ui::ink(theme, theme.success))
                .child(format!("+{added}")),
        )
        .child(
            div()
                .text_color(ui::ink(theme, theme.danger))
                .child(format!("−{removed}")),
        )
}

/// The card under turn divider `done`: "N changed files +a −r" and Open turn N diff; opened, one row per file.
pub(super) fn render<V: ItemsView>(
    this: &V,
    done: usize,
    cx: &mut Context<V>,
) -> Option<AnyElement> {
    let (turn, files) = this.turn_files(done)?;
    if files.is_empty() {
        return None;
    }
    let key = format!("files:{done}");
    let open = this.is_expanded(&key);
    let (added, removed) = files
        .iter()
        .fold((0, 0), |(a, r), f| (a + f.added, r + f.removed));
    let count = match files.len() {
        1 => "1 changed file".to_owned(),
        n => format!("{n} changed files"),
    };
    let first = files[0].path.clone();
    let theme = cx.theme();
    let toggle = key.clone();
    let header = h_flex()
        .id(SharedString::from(key))
        .debug_selector(|| "turn-files".into())
        .gap_3()
        .px_3()
        .py_1p5()
        .cursor_pointer()
        .text_size(ui::META)
        .font_weight(FontWeight::MEDIUM)
        .on_click(cx.listener(move |this, _, _, cx| this.toggle_tool(&toggle, cx)))
        .child(
            Icon::new(if open {
                IconName::ChevronDown
            } else {
                IconName::ChevronRight
            })
            .size_3()
            .text_color(theme.muted_foreground),
        )
        .child(count)
        .child(stat_label(added, removed, cx))
        .child(div().flex_1())
        .child(
            Button::new(SharedString::from(format!("open-turn-diff-{done}")))
                .ghost()
                .xsmall()
                .label(format!("Open turn {turn} diff"))
                .on_click(cx.listener(move |this, _, _, cx| {
                    cx.stop_propagation();
                    this.open_turn_file(turn, &first, cx)
                })),
        );
    let theme = cx.theme();
    let rows = files.iter().enumerate().map(|(i, f)| {
        let path = f.path.clone();
        h_flex()
            .id(SharedString::from(format!("turn-file-{done}-{i}")))
            .debug_selector(|| "turn-file".into())
            .h(px(28.))
            .gap_2()
            .px_2()
            .rounded(ui::RADIUS_MD)
            .cursor_pointer()
            .hover(|s| s.bg(theme.accent))
            .on_click(cx.listener(move |this, _, _, cx| this.open_turn_file(turn, &path, cx)))
            .child(
                Icon::new(IconName::File)
                    .size_3p5()
                    .text_color(theme.muted_foreground),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .font_family(theme.mono_font_family.clone())
                    .text_size(ui::META)
                    .child(f.rel.clone()),
            )
            .child(stat_label(f.added, f.removed, cx))
    });
    let rows: Vec<_> = rows.collect();
    let theme = cx.theme();
    Some(
        v_flex()
            .mt_2()
            .rounded(ui::RADIUS)
            .bg(theme.secondary)
            .child(header)
            .when(open, |el| el.child(v_flex().px_1().pb_1().children(rows)))
            .into_any_element(),
    )
}
