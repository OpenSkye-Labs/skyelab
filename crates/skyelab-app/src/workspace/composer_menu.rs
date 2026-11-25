//! The composer's `@` files and models, `/` commands, `$` skills and `#` threads-and-PRs menus: when one opens, what it lists, and the order.
//! No GPUI here, so it is tested without a window.

use skyelab_core::Provider;
use std::path::Path;

/// The most rows a menu shows.
pub(crate) const MAX_ROWS: usize = 50;
/// Model rows in the `@` menu, as T3 caps them, so they never push the files out.
pub(crate) const MAX_MODEL_ROWS: usize = 8;
/// How many tracked files the `@` menu ranks at most (the file picker's cap).
const MAX_TRACKED: usize = 100_000;
/// How much of a command or skill file is read for its description.
const DESCRIPTION_BYTES: u64 = 4096;
/// The most command or skill files read from one folder.
const MAX_FILES: usize = 200;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MenuKind {
    Files,
    Commands,
    Skills,
    /// This project's threads and their pull requests, picked as context chips.
    Context,
}

/// A token being typed that opens a menu: its kind, where its trigger character sits (a byte offset) and the
/// query after it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Trigger {
    pub kind: MenuKind,
    pub start: usize,
    pub query: String,
}

/// The menu `text` opens with the cursor at byte `cursor`: the word before the cursor starting with `@`, `$` or
/// `#`, or `/` as the first thing in the prompt.
pub(crate) fn trigger(text: &str, cursor: usize) -> Option<Trigger> {
    let before = text.get(..cursor)?;
    let start = before
        .char_indices()
        .rev()
        .find(|(_, c)| c.is_whitespace())
        .map_or(0, |(i, c)| i + c.len_utf8());
    let word = &before[start..];
    let kind = match word.chars().next()? {
        '@' => MenuKind::Files,
        '$' => MenuKind::Skills,
        // `#12` is an issue or PR number in prose, not a pick.
        '#' if word.len() == 1 || !word[1..].bytes().all(|b| b.is_ascii_digit()) => {
            MenuKind::Context
        }
        '/' if before[..start].trim().is_empty() => MenuKind::Commands,
        _ => return None,
    };
    Some(Trigger {
        kind,
        start,
        query: word[1..].to_owned(),
    })
}

/// One row: what it shows, and the text that replaces the typed token.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct MenuItem {
    pub label: String,
    pub detail: String,
    pub insert: String,
    /// A `#` row: the chip picking it adds.
    pub context: Option<ContextPick>,
    /// An `@` model row (T3's model mention): this model id. Picking it inserts a `model:<provider>/<id>` reference
    /// for the agent to read; the thread's own model stays as it is.
    pub model: Option<String>,
}

/// What a `#` row adds: a thread's gist, or the pull request linked to a thread (both by thread id).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ContextPick {
    Thread(i64),
    Pr(i64),
}

/// The rows matching `query`, best first, at most [`MAX_ROWS`]. A match is the query's characters in order
/// (ignoring case); a prefix of the file name beats a substring, which beats a scattered match, and shorter wins ties.
/// Model rows match only by [`model_matches`], come first, and are at most [`MAX_MODEL_ROWS`].
pub(crate) fn rank(items: &[MenuItem], query: &str) -> Vec<usize> {
    let q = query.to_lowercase();
    let mut scored: Vec<(u32, usize, usize)> = items
        .iter()
        .enumerate()
        .filter_map(|(i, item)| match &item.model {
            Some(id) => model_matches(&item.label, id, &q).then_some((0, item.label.len(), i)),
            None => score(&item.label.to_lowercase(), &q).map(|s| (s + 1, item.label.len(), i)),
        })
        .collect();
    scored.sort();
    let mut models = 0;
    scored
        .into_iter()
        .filter(|&(_, _, i)| {
            models += usize::from(items[i].model.is_some());
            items[i].model.is_none() || models <= MAX_MODEL_ROWS
        })
        .take(MAX_ROWS)
        .map(|(_, _, i)| i)
        .collect()
}

/// Lower is better; `None` when `label` doesn't hold `q`'s characters in order.
fn score(label: &str, q: &str) -> Option<u32> {
    if q.is_empty() {
        return Some(3);
    }
    let name = label.rsplit('/').next().unwrap_or(label);
    if name.starts_with(q) {
        return Some(0);
    }
    if label.starts_with(q) {
        return Some(1);
    }
    if label.contains(q) {
        return Some(2);
    }
    let mut chars = label.chars();
    q.chars().all(|c| chars.any(|l| l == c)).then_some(4)
}

/// Whether `@q` names model `label` (`id`): at least two characters, and every word of `q` (split at anything but
/// letters, digits and `.`) starts a word of the label or id. Fuzzy hits would push files out of the menu.
fn model_matches(label: &str, id: &str, q: &str) -> bool {
    let split = |s: &str| -> Vec<String> {
        s.to_lowercase()
            .split(|c: char| !c.is_alphanumeric() && c != '.')
            .filter(|w| !w.is_empty())
            .map(str::to_owned)
            .collect()
    };
    let words = split(&format!("{label} {id}"));
    let wanted = split(q);
    wanted.iter().map(String::len).sum::<usize>() >= 2
        && wanted
            .iter()
            .all(|w| words.iter().any(|x| x.starts_with(w.as_str())))
}

/// The `@` rows for the thread's models (id, label). Picking one inserts `model:<provider>/<id>` (T3's mention,
/// with the provider's config id), text for the agent to read.
pub(crate) fn model_items(models: &[(String, String)], provider: Provider) -> Vec<MenuItem> {
    let key = provider.label().to_lowercase();
    models
        .iter()
        .map(|(id, label)| MenuItem {
            label: label.clone(),
            detail: format!("{} model", provider.label()),
            insert: format!("model:{key}/{id} "),
            model: Some(id.clone()),
            ..Default::default()
        })
        .collect()
}

/// The `@` menu's rows: `models` (from [`model_items`]), then the files git tracks in `cwd`.
pub(crate) fn at_items(mut models: Vec<MenuItem>, cwd: &Path) -> Vec<MenuItem> {
    let files = skyelab_git::tracked_files(cwd).map(|f| file_items(&f));
    models.extend(files.unwrap_or_default());
    models
}

/// The `@` rows for tracked files (from `skyelab_git::tracked_files`).
pub(crate) fn file_items(files: &[String]) -> Vec<MenuItem> {
    files
        .iter()
        .take(MAX_TRACKED)
        .map(|f| MenuItem {
            label: f.clone(),
            detail: String::new(),
            insert: format!("@{f} "),
            ..Default::default()
        })
        .collect()
}

/// Commands the CLI runs itself in `-p` (stream-json) mode. Codex's app-server takes none.
const CLAUDE_BUILTINS: &[(&str, &str)] = &[
    ("compact", "Summarize the conversation to free up context"),
    ("context", "Show what fills the context window"),
    ("cost", "Show this session's cost and duration"),
    ("init", "Write a CLAUDE.md for this project"),
    ("review", "Review a pull request"),
    (
        "security-review",
        "Review the pending changes for security issues",
    ),
    (
        "pr-comments",
        "Show the comments on this branch's pull request",
    ),
];

/// SkyeLab's own commands, which any thread but a side chat takes (`side_chat::side_question`).
const SKYELAB_COMMANDS: &[(&str, &str)] = &[
    (
        "side",
        "Ask in a side chat, without interrupting this thread",
    ),
    ("btw", "Same as /side"),
];

/// The `/` rows: SkyeLab's own (with `side_commands`, so not in a side chat), the CLI's built-ins, then the account's
/// own `commands/` (from its config dir, never the parent's), then the project's `.claude/commands/`. Codex has no
/// slash commands of its own.
pub(crate) fn command_items(
    provider: Provider,
    account_dir: &Path,
    project: &Path,
    side_commands: bool,
) -> Vec<MenuItem> {
    let ours = SKYELAB_COMMANDS
        .iter()
        .filter(|_| side_commands)
        .map(|(name, about)| MenuItem {
            label: format!("/{name}"),
            detail: (*about).to_owned(),
            insert: format!("/{name} "),
            ..Default::default()
        });
    if matches!(provider, Provider::Codex | Provider::Antigravity) {
        return ours.collect();
    }
    let builtins = CLAUDE_BUILTINS.iter().map(|(name, about)| MenuItem {
        label: format!("/{name}"),
        detail: (*about).to_owned(),
        insert: format!("/{name} "),
        ..Default::default()
    });
    let account = markdown_commands(&account_dir.join("commands"), "", Links::Follow);
    // The repo's folder is untrusted: a link there could point the menu at any file.
    let project = markdown_commands(
        &project.join(".claude").join("commands"),
        "project",
        Links::Skip,
    );
    ours.chain(builtins).chain(account).chain(project).collect()
}

/// `name.md` files in `dir`, and `ns/name.md` one folder down (Claude names those `ns:name`).
fn markdown_commands(dir: &Path, source: &str, links: Links) -> Vec<MenuItem> {
    let mut out = Vec::new();
    for (prefix, file) in entries(dir, links).into_iter().flat_map(|e| {
        if e.is_dir() {
            let ns = e
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            entries(&e, links)
                .into_iter()
                .map(|f| (format!("{ns}:"), f))
                .collect()
        } else {
            vec![(String::new(), e)]
        }
    }) {
        if out.len() >= MAX_FILES {
            break;
        }
        // Regular files only: a repo's FIFO named `x.md` would block the read forever.
        if file.extension().is_none_or(|x| x != "md") || !file.is_file() {
            continue;
        }
        let Some(stem) = file.file_stem().map(|s| s.to_string_lossy().into_owned()) else {
            continue;
        };
        let name = format!("{prefix}{stem}");
        let about = description(&file);
        out.push(MenuItem {
            label: format!("/{name}"),
            detail: match (about.is_empty(), source.is_empty()) {
                (true, _) => source.to_owned(),
                (false, true) => about,
                (false, false) => format!("{about} ({source})"),
            },
            insert: format!("/{name} "),
            ..Default::default()
        });
    }
    out
}

/// The `$` rows: the account's skills, `skills/<name>/SKILL.md` in its config dir. Claude runs a skill as
/// `/name`, Codex as `$name`.
pub(crate) fn skill_items(provider: Provider, account_dir: &Path) -> Vec<MenuItem> {
    entries(&account_dir.join("skills"), Links::Follow)
        .into_iter()
        .filter(|d| d.join("SKILL.md").is_file())
        .take(MAX_FILES)
        .filter_map(|d| {
            let name = d.file_name()?.to_string_lossy().into_owned();
            let insert = match provider {
                Provider::Claude | Provider::Antigravity => format!("/{name} "),
                Provider::Codex => format!("${name} "),
            };
            Some(MenuItem {
                label: format!("${name}"),
                detail: description(&d.join("SKILL.md")),
                insert,
                ..Default::default()
            })
        })
        .collect()
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Links {
    Follow,
    Skip,
}

/// `dir`'s entries (at most a thousand), sorted; none when it can't be read. With [`Links::Skip`], symlinks are left out.
fn entries(dir: &Path, links: Links) -> Vec<std::path::PathBuf> {
    let mut v: Vec<_> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .filter(|e| links == Links::Follow || e.file_type().is_ok_and(|t| !t.is_symlink()))
        .take(1000)
        .map(|e| e.path())
        .collect();
    v.sort();
    v
}

/// The `description:` from a markdown file's front matter, or its first text line, from its first bytes only.
fn description(file: &Path) -> String {
    use std::io::Read as _;
    let mut bytes = Vec::new();
    if file.is_file() {
        let _ = std::fs::File::open(file)
            .and_then(|f| f.take(DESCRIPTION_BYTES).read_to_end(&mut bytes));
    }
    let head = String::from_utf8_lossy(&bytes);
    let mut lines = head.lines();
    if head.starts_with("---") {
        lines.next();
        for l in lines.by_ref() {
            if l.trim() == "---" {
                break;
            }
            if let Some(d) = l.strip_prefix("description:") {
                return d.trim().trim_matches('"').to_owned();
            }
        }
    }
    lines
        .map(|l| l.trim().trim_start_matches('#').trim())
        .find(|l| !l.is_empty())
        .unwrap_or_default()
        .chars()
        .take(120)
        .collect()
}
