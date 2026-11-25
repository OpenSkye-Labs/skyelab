//! The command palette (T3's CommandPalette): one key to find and run any action, switch project, open a thread or
//! focus a tab. Entries come from the workspace; this file groups and ranks them and draws them, T3's way, in
//! gpui-component's `Command` (its virtual list, arrow keys and hover) inside a dialog.

use crate::ui::{Kbd, SkyeIcon};
use gpui::{App, KeyBinding, NoAction, SharedString, Task, Window, actions};
use std::cmp::Reverse;
use std::ops::Range;
use std::rc::Rc;

actions!(skyelab, [OpenCommandPalette]);

#[cfg(test)]
pub const OPEN_PALETTE_KEY: &str = if cfg!(target_os = "macos") {
    "cmd-k"
} else {
    "ctrl-k"
};

/// T3's root placeholder.
const PLACEHOLDER: &str = "Search commands, projects, and threads…";
/// How many threads an empty query lists, most recently used first (T3's `RECENT_THREAD_LIMIT`).
const RECENT_LIMIT: usize = 12;

/// What an entry does when chosen.
#[derive(Clone, Debug, PartialEq)]
pub enum Kind {
    /// A registered action, by its full name (`skyelab::OpenSettings`).
    Action(&'static str),
    Project,
    Thread,
    Tab,
}

/// The section a kind lists under.
fn group_of(kind: &Kind) -> &'static str {
    match kind {
        Kind::Action("skyelab::RunProjectScript") => "Scripts",
        Kind::Action(_) => "Actions",
        Kind::Project => "Projects",
        Kind::Thread => "Threads",
        Kind::Tab => "Tabs",
    }
}

/// Runs with focus already back on the element that had it before the palette opened.
pub type Run = Rc<dyn Fn(&mut Window, &mut App)>;

#[derive(Clone)]
pub struct Entry {
    /// What the search matches, and what tests find an entry by.
    pub label: SharedString,
    pub kind: Kind,
    pub binding: Option<Kbd>,
    /// A toggle's current state, shown as a ticked box; `None` for anything else.
    pub checked: Option<bool>,
    pub run: Run,
    /// What the row shows when it differs from `label` ("demo" for "Switch to project: demo").
    pub title: Option<SharedString>,
    /// A muted second line (the project of another project's thread).
    pub subtitle: Option<SharedString>,
    /// The row's icon; `None` picks one from the kind.
    pub icon: Option<SkyeIcon>,
    /// Bytes of the shown title drawn bold (T3's "New thread in **project**").
    pub bold: Option<Range<usize>>,
    /// Choosing the row opens these as a page with a Back row, instead of running.
    pub submenu: Option<Rc<[Entry]>>,
    /// When a thread was last used (ms since the epoch), for Recent threads.
    pub recency: i64,
    /// The section it lists under when not its kind's: a submenu item found by a query lists under its submenu.
    pub group: Option<SharedString>,
    /// Choosing the row (or Tab on it) puts this in the search field instead of running: a folder to browse into.
    pub browse: Option<SharedString>,
    /// Choosing the row opens this page (with its own search) over the shown one.
    pub page: Option<Rc<view::PageSpec>>,
}

impl Entry {
    pub fn new(label: impl Into<SharedString>, kind: Kind, run: Run) -> Self {
        Self {
            label: label.into(),
            kind,
            binding: None,
            checked: None,
            run,
            title: None,
            subtitle: None,
            icon: None,
            bold: None,
            submenu: None,
            recency: 0,
            group: None,
            browse: None,
            page: None,
        }
    }

    pub fn group(mut self, group: impl Into<SharedString>) -> Self {
        self.group = Some(group.into());
        self
    }

    pub fn page(mut self, spec: view::PageSpec) -> Self {
        self.page = Some(Rc::new(spec));
        self
    }

    pub fn browse(mut self, query: impl Into<SharedString>) -> Self {
        self.browse = Some(query.into());
        self
    }

    pub fn title(mut self, title: impl Into<SharedString>) -> Self {
        self.title = Some(title.into());
        self
    }

    pub fn subtitle(mut self, subtitle: impl Into<SharedString>) -> Self {
        self.subtitle = Some(subtitle.into());
        self
    }

    pub fn icon(mut self, icon: SkyeIcon) -> Self {
        self.icon = Some(icon);
        self
    }

    pub fn bold(mut self, bytes: Range<usize>) -> Self {
        self.bold = Some(bytes);
        self
    }

    pub fn submenu(mut self, items: Vec<Entry>) -> Self {
        self.submenu = Some(items.into());
        self
    }

    pub fn recency(mut self, ms: i64) -> Self {
        self.recency = ms;
        self
    }

    pub fn binding(mut self, binding: Option<Kbd>) -> Self {
        self.binding = binding;
        self
    }

    pub fn checked(mut self, checked: Option<bool>) -> Self {
        self.checked = checked;
        self
    }

    pub(crate) fn shown(&self) -> &SharedString {
        self.title.as_ref().unwrap_or(&self.label)
    }
}

/// `entries` with every submenu's items after them, so a query finds those too (T3's nested search).
pub(crate) fn flatten(mut entries: Vec<Entry>) -> Vec<Entry> {
    let nested: Vec<Entry> = entries
        .iter()
        .filter_map(|e| Some((e.shown().clone(), e.submenu.as_deref()?)))
        .flat_map(|(parent, items)| {
            items.iter().map(move |item| Entry {
                group: Some(parent.clone()),
                ..item.clone()
            })
        })
        .collect();
    entries.extend(nested);
    entries
}

/// The palette keys for one OS. `KeyBinding::new` can't resolve `secondary-`, so the caller picks.
pub fn bindings(mac: bool) -> Vec<KeyBinding> {
    let (k, p) = if mac {
        ("cmd-k", "cmd-shift-p")
    } else {
        ("ctrl-k", "ctrl-shift-p")
    };
    let mut keys = vec![
        KeyBinding::new(k, OpenCommandPalette, Some("Workspace")),
        KeyBinding::new(p, OpenCommandPalette, Some("Workspace")),
    ];
    // ctrl-k is readline's kill-line: a focused shell keeps it, ctrl-shift-p still opens the palette there.
    if !mac {
        keys.push(KeyBinding::new("ctrl-k", NoAction, Some("Terminal")));
    }
    keys
}

pub fn init(cx: &mut App) {
    cx.bind_keys(bindings(cfg!(target_os = "macos")));
    view::bind_keys(cx);
}

/// One char per char, so text indexes stay aligned (`İ` lowercases to two).
fn lower(c: char) -> char {
    c.to_lowercase().next().unwrap_or(c)
}

fn is_word_start(text: &[char], i: usize) -> bool {
    i == 0
        || !text[i - 1].is_alphanumeric()
        || (text[i - 1].is_lowercase() && text[i].is_uppercase())
}

/// The best fuzzy match of `query` in `text`: its score and the char indexes it used. See [`score`].
fn best_match(query: &str, text: &str) -> Option<(i32, Vec<usize>)> {
    let query: Vec<char> = query
        .chars()
        .filter(|c| !c.is_whitespace())
        .map(lower)
        .collect();
    let Some((&first, rest)) = query.split_first() else {
        return Some((0, Vec::new()));
    };
    let text: Vec<char> = text.chars().collect();
    let lower: Vec<char> = text.iter().copied().map(lower).collect();
    (0..text.len())
        .filter(|&i| lower[i] == first)
        .filter_map(|start| {
            let mut score = if start == 0 { 3 } else { 0 };
            if is_word_start(&text, start) {
                score += 10;
            }
            let mut at = vec![start];
            for &c in rest {
                let prev = *at.last()?;
                let i = (prev + 1..text.len()).find(|&i| lower[i] == c)?;
                let gap = (i - prev - 1) as i32;
                score += if gap == 0 { 8 } else { -gap.min(5) };
                if gap > 0 && is_word_start(&text, i) {
                    score += 6;
                }
                at.push(i);
            }
            Some((score, at))
        })
        .max_by_key(|(score, _)| *score)
}

/// How well `query` fuzzy-matches `text`, higher is better; `None` when its letters aren't all there in order.
/// Case and spaces in the query are ignored. Word starts and consecutive runs score; gaps cost.
pub fn score(query: &str, text: &str) -> Option<i32> {
    best_match(query, text).map(|(score, _)| score)
}

/// Indexes of the labels that match `query`, best first.
pub fn rank<'a>(query: &str, labels: impl IntoIterator<Item = &'a str>) -> Vec<usize> {
    let mut scored: Vec<(i32, usize)> = labels
        .into_iter()
        .enumerate()
        .filter_map(|(i, label)| score(query, label).map(|s| (s, i)))
        .collect();
    scored.sort_by_key(|&(s, _)| Reverse(s));
    scored.into_iter().map(|(_, i)| i).collect()
}

/// The byte ranges of `text` a row draws bold: the letters `query` matched, and `bold`.
pub fn match_ranges(query: &str, text: &str, bold: Option<Range<usize>>) -> Vec<Range<usize>> {
    let matched = best_match(query, text)
        .map(|(_, at)| at)
        .unwrap_or_default();
    let mut out: Vec<Range<usize>> = Vec::new();
    for (n, (i, c)) in text.char_indices().enumerate() {
        let strong = matched.contains(&n) || bold.as_ref().is_some_and(|b| b.contains(&i));
        if !strong {
            continue;
        }
        let end = i + c.len_utf8();
        match out.last_mut() {
            Some(last) if last.end == i => last.end = end,
            _ => out.push(i..end),
        }
    }
    out
}

/// A titled section of the list: indexes into the entries.
#[derive(Debug, PartialEq)]
pub struct Group {
    pub label: SharedString,
    pub rows: Vec<usize>,
}

fn push_to(groups: &mut Vec<Group>, entry: &Entry, row: usize) {
    let label = match &entry.group {
        Some(label) => label.clone(),
        None => group_of(&entry.kind).into(),
    };
    match groups.iter_mut().find(|g| g.label == label) {
        Some(group) => group.rows.push(row),
        None => groups.push(Group {
            label,
            rows: vec![row],
        }),
    }
}

/// T3's sections. `entries[..nested]` are the top-level ones, `[nested..found]` their submenus' items (listed only
/// for a query), `[found..]` the message search's threads. An empty query lists Actions, then Recent threads (newest
/// first, at most [`RECENT_LIMIT`]), Projects, Scripts and Tabs. A query ranks everything with [`score`]; each section
/// keeps that order and sections come in the order of their best row; found threads follow in Threads.
pub fn arrange(query: &str, entries: &[Entry], nested: usize, found: usize) -> Vec<Group> {
    let found = found.min(entries.len());
    let mut groups = Vec::new();
    if query.trim().is_empty() {
        let top = &entries[..nested.min(found)];
        let mut recent: Vec<usize> = (0..top.len())
            .filter(|&i| top[i].kind == Kind::Thread)
            .collect();
        recent.sort_by_key(|&i| Reverse(top[i].recency));
        recent.truncate(RECENT_LIMIT);
        for name in ["Actions", "Threads", "Projects", "Scripts", "Tabs"] {
            let rows: Vec<usize> = match name {
                "Threads" => std::mem::take(&mut recent),
                _ => (0..top.len())
                    .filter(|&i| group_of(&top[i].kind) == name)
                    .collect(),
            };
            if !rows.is_empty() {
                let label = if name == "Threads" {
                    "Recent threads"
                } else {
                    name
                };
                groups.push(Group {
                    label: label.into(),
                    rows,
                });
            }
        }
        return groups;
    }
    let ranked = rank(query, entries[..found].iter().map(|e| e.label.as_ref()));
    for i in ranked.into_iter().chain(found..entries.len()) {
        push_to(&mut groups, &entries[i], i);
    }
    groups
}

const HUMAN_NAME_OVERRIDES: &[(&str, &str)] = &[
    ("skyelab::ToggleEnvironments", "Switch Machine"),
    ("skyelab::Stop", "Stop Agent"),
    ("skyelab::SchedulePrompt", "Schedule this prompt…"),
    ("skyelab::Interrupt", "Interrupt Agent"),
    ("skyelab::RemoteStop", "Stop Remote Agent"),
    ("skyelab::SendRemotePrompt", "Send Prompt to Remote"),
    ("skyelab::ToggleSharing", "Toggle Sharing on Tailnet"),
    ("skyelab::CheckForUpdates", "Check for Updates"),
    ("skyelab::OpenRemote", "Open Remote Access"),
    ("skyelab::BindFolder", "Bind Folder to Account"),
    ("skyelab::ContinueInCodex", "Continue in Codex"),
    ("skyelab::ContinueInClaude", "Continue in Claude"),
    ("skyelab::ContinueInAntigravity", "Continue in Antigravity"),
    ("skyelab::ToggleWorktree", "Toggle Thread Worktree"),
    ("skyelab::OpenInEditor", "Open in Editor"),
    ("skyelab::OpenFilePicker", "Go to File"),
    ("skyelab::SearchFiles", "Search in Files"),
    ("skyelab::JumpToThread1", "Jump to Thread 1"),
    ("skyelab::JumpToThread2", "Jump to Thread 2"),
    ("skyelab::JumpToThread3", "Jump to Thread 3"),
    ("skyelab::JumpToThread4", "Jump to Thread 4"),
    ("skyelab::JumpToThread5", "Jump to Thread 5"),
    ("skyelab::JumpToThread6", "Jump to Thread 6"),
    ("skyelab::JumpToThread7", "Jump to Thread 7"),
    ("skyelab::JumpToThread8", "Jump to Thread 8"),
    ("skyelab::JumpToThread9", "Jump to Thread 9"),
];

const HIDDEN: &[&str] = &[
    "skyelab::OpenCommandPalette",
    "skyelab::CompleteFolder",
    "skyelab::OnboardingNext",
    "skyelab::OnboardingBack",
    "skyelab::ConnectTyped",
    "skyelab::ConfirmDiscardWorktree",
    "skyelab::CancelDiscardWorktree",
    "skyelab::CancelEdit",
    "skyelab::OpenPickedInEditor",
];

/// `skyelab::OpenSettings` → "Open Settings": the table, else the CamelCase name split into words.
pub fn humanize(name: &str) -> String {
    if let Some((_, human)) = HUMAN_NAME_OVERRIDES.iter().find(|(n, _)| *n == name) {
        return (*human).to_owned();
    }
    let short = name.rsplit("::").next().unwrap_or(name);
    let mut out = String::new();
    let mut prev_lower = false;
    for c in short.chars() {
        if c.is_uppercase() && prev_lower {
            out.push(' ');
        }
        prev_lower = c.is_lowercase();
        out.push(c);
    }
    out
}

pub fn listed(name: &str) -> bool {
    name.starts_with("skyelab::") && !HIDDEN.contains(&name)
}

/// More entries for a query, found off the UI thread (the store's thread search over message text). They list after
/// the ranked entries, minus any label already there.
pub type Search = Rc<dyn Fn(String, &mut App) -> Task<Vec<Entry>>>;

#[path = "palette_view.rs"]
mod view;
pub use view::{PageSpec, open, open_page, open_spec};
// The Add project tests' hook (unix only, as they are).
#[cfg(all(test, unix))]
pub use view::{Shown, shown};

#[cfg(test)]
#[path = "palette_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "palette_arrange_test.rs"]
mod arrange_tests;
