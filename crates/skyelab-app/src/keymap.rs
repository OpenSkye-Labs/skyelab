//! The user's keybindings file over the built-in bindings, Zed's way: on every load the keymap is cleared, the
//! defaults go back in, then the user's rules go in last. GPUI lets the later of two bindings at the same context
//! depth win, so a rule overrides the default it shares a key with. The file is re-read when it changes.

use gpui::{
    Action, App, AsyncApp, Global, KeyBinding, KeyBindingContextPredicate, KeyBindingMetaIndex,
    NoAction, Task,
};
use skyelab_core::keymap::{Problem, Rule, UNBIND};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, SystemTime};

/// Marks the bindings that came from the file, so Settings can tell them apart. GPUI ranks a smaller meta as the
/// stronger source: a file `none` (`NoAction`) hides only bindings whose meta is not smaller, so defaults get a larger one.
pub const CUSTOM: KeyBindingMetaIndex = KeyBindingMetaIndex(0);
const DEFAULT: KeyBindingMetaIndex = KeyBindingMetaIndex(1);
/// How often the file is checked for changes.
pub const POLL: Duration = Duration::from_secs(1);

pub struct Keymap {
    pub path: Option<PathBuf>,
    /// Every binding registered before the file was first read: each crate's and gpui-component's.
    defaults: Vec<KeyBinding>,
    /// What was wrong with the file at its last load.
    pub problems: Vec<Problem>,
    /// Bumped on every load, so each load's problems are shown once.
    pub generation: usize,
    _watch: Option<Task<()>>,
}

impl Global for Keymap {}

/// Call once, after every `init` that binds keys. Reads `path` now and again whenever it changes.
pub fn init(path: Option<PathBuf>, cx: &mut App) {
    let defaults = cx
        .key_bindings()
        .borrow()
        .bindings()
        .map(|b| {
            let mut b = b.clone();
            if b.meta().is_none() {
                b.set_meta(DEFAULT);
            }
            b
        })
        .collect();
    cx.set_global(Keymap {
        path: path.clone(),
        defaults,
        problems: Vec::new(),
        generation: 0,
        _watch: None,
    });
    let Some(path) = path else { return };
    let (stamp, text) = read(&path, None);
    load_read(text.unwrap_or(Ok(String::new())), cx);
    let watch = cx.spawn(async move |cx| watch(path, stamp, cx).await);
    cx.global_mut::<Keymap>()._watch = Some(watch);
}

type Stamp = Option<(SystemTime, u64)>;

/// The file at its last read: its text, or why it couldn't be read.
type Read = Result<String, String>;

/// Files over this are refused unread: a keymap is a few KB.
const MAX_FILE: u64 = 1024 * 1024;

/// The file's stamp, and what it holds when the stamp differs from `last` (a missing file reads as empty). Only a
/// regular file is opened: a FIFO there would block the read for good.
fn read(path: &Path, last: Option<Stamp>) -> (Stamp, Option<Read>) {
    let meta = std::fs::metadata(path);
    let stamp = meta
        .as_ref()
        .ok()
        .and_then(|m| Some((m.modified().ok()?, m.len())));
    if last == Some(stamp) {
        return (stamp, None);
    }
    let text = match meta {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
        Err(e) => Err(e.to_string()),
        Ok(m) if !m.is_file() => Err("it isn't a regular file".to_owned()),
        Ok(m) if m.len() > MAX_FILE => Err("it is over 1 MB".to_owned()),
        Ok(_) => std::fs::read_to_string(path).map_err(|e| e.to_string()),
    };
    (stamp, Some(text))
}

/// [`load`] from what [`read`] found; a file that can't be read keeps only the defaults and says why.
fn load_read(read: Read, cx: &mut App) {
    match read {
        Ok(text) => load(Some(&text), cx),
        Err(why) => {
            load(None, cx);
            cx.global_mut::<Keymap>().problems.push(Problem {
                line: 1,
                reason: format!("can't read the file ({why}), so only the built-in keys apply"),
            });
        }
    }
}

async fn watch(path: PathBuf, mut stamp: Stamp, cx: &mut AsyncApp) {
    // shortcut: polls a stat each second; switch to an fs watcher if more files need watching.
    loop {
        cx.background_executor().timer(POLL).await;
        let p = path.clone();
        let (now, text) = cx
            .background_executor()
            .spawn(async move { read(&p, Some(stamp)) })
            .await;
        stamp = now;
        if let Some(text) = text {
            cx.update(|cx| load_read(text, cx));
        }
    }
}

/// Re-binds from `text` (the file's contents; `None` keeps only the defaults) and records its problems.
pub fn load(text: Option<&str>, cx: &mut App) {
    let (rules, mut problems) = skyelab_core::keymap::parse(text.unwrap_or_default());
    problems.extend(apply(&rules, cx));
    let keymap = cx.global_mut::<Keymap>();
    keymap.problems = problems;
    keymap.generation += 1;
}

/// The defaults, then `rules`. Returns the rules GPUI refused.
fn apply(rules: &[Rule], cx: &mut App) -> Vec<Problem> {
    let mut problems = Vec::new();
    let mut custom = Vec::new();
    for rule in rules {
        match binding(rule, cx) {
            Ok(b) => custom.push(b.with_meta(CUSTOM)),
            Err(reason) => problems.push(Problem {
                line: rule.line,
                reason,
            }),
        }
    }
    let defaults = cx.global::<Keymap>().defaults.clone();
    cx.clear_key_bindings();
    cx.bind_keys(defaults);
    cx.bind_keys(custom);
    problems
}

fn binding(rule: &Rule, cx: &App) -> Result<KeyBinding, String> {
    let action: Box<dyn Action> = if rule.action == UNBIND {
        Box::new(NoAction)
    } else {
        cx.build_action(&rule.action, None).map_err(|e| match e {
            gpui::ActionBuildError::NotFound { .. } => format!("unknown action `{}`", rule.action),
            gpui::ActionBuildError::BuildError { .. } => {
                format!("`{}` needs arguments, so it can't be bound", rule.action)
            }
        })?
    };
    let context = KeyBindingContextPredicate::parse(&rule.context)
        .map_err(|e| format!("bad context `{}`: {e}", rule.context))?;
    // GPUI takes any name, so a typo would bind a key that never fires: names come from the built-in bindings.
    let known = known_contexts(&cx.global::<Keymap>().defaults);
    if let Some(name) = names(&context).into_iter().find(|n| !known.contains(n)) {
        let mut all: Vec<&str> = known.iter().map(|n| n.as_ref()).collect();
        all.sort_unstable();
        return Err(format!(
            "unknown context `{name}` (known: {})",
            all.join(", ")
        ));
    }
    // `+` is a key of its own (cmd-+), never a join (cmd+k).
    if rule
        .key
        .split('-')
        .any(|part| part.len() > 1 && part.contains('+'))
    {
        return Err(format!(
            "bad key `{}`: join keys with `-`, e.g. cmd-shift-k",
            rule.key
        ));
    }
    KeyBinding::load(&rule.key, action, Some(Rc::new(context)), false, None, cx)
        .map_err(|_| format!("bad key `{}`", rule.key))
}

/// The context names a predicate tests for (`a > b && !c` names a, b and c).
fn names(p: &KeyBindingContextPredicate) -> Vec<gpui::SharedString> {
    use KeyBindingContextPredicate as P;
    match p {
        P::Identifier(n) | P::Equal(n, _) | P::NotEqual(n, _) => vec![n.clone()],
        P::Not(a) => names(a),
        P::Descendant(a, b) | P::And(a, b) | P::Or(a, b) => {
            let mut all = names(a);
            all.extend(names(b));
            all
        }
    }
}

/// Every context name the built-in bindings use, which is every context a view sets that has keys.
fn known_contexts(defaults: &[KeyBinding]) -> std::collections::HashSet<gpui::SharedString> {
    defaults
        .iter()
        .filter_map(KeyBinding::predicate)
        .flat_map(|p| names(&p))
        .collect()
}

#[cfg(test)]
#[path = "keymap_tests.rs"]
mod tests;
