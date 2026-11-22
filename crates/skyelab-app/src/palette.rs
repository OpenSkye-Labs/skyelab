//! The command palette: one key to find and run any action, switch project, open a thread or focus a tab.

use gpui::{App, KeyBinding, SharedString, Window, actions};
use gpui_component::kbd::Kbd;
use std::rc::Rc;

actions!(skyelab, [OpenCommandPalette]);

pub const OPEN_PALETTE_KEY: &str = if cfg!(target_os = "macos") {
    "cmd-k"
} else {
    "ctrl-k"
};

/// What an entry does when chosen.
#[derive(Clone, Debug, PartialEq)]
pub enum Kind {
    /// A registered action, by its full name (`skyelab::OpenSettings`).
    Action(&'static str),
    Project,
    Thread,
    Tab,
}

#[derive(Clone)]
pub struct Entry {
    pub label: SharedString,
    pub kind: Kind,
    pub binding: Option<Kbd>,
    /// Runs with focus already back on the element that had it before the palette opened.
    pub run: Rc<dyn Fn(&mut Window, &mut App)>,
}

pub fn bindings(_mac: bool) -> Vec<KeyBinding> {
    todo!()
}

pub fn init(_cx: &mut App) {}

pub fn score(_query: &str, _text: &str) -> Option<i32> {
    todo!()
}

pub fn rank<'a>(_query: &str, _labels: impl IntoIterator<Item = &'a str>) -> Vec<usize> {
    todo!()
}

pub fn humanize(_name: &str) -> String {
    todo!()
}

pub fn listed(_name: &str) -> bool {
    todo!()
}

#[cfg(test)]
#[path = "palette_tests.rs"]
mod tests;
