//! Tells the user once per load what is wrong with their keybindings file. The keys still work: bad lines are skipped.

use super::Workspace;
use crate::keymap::Keymap;
use gpui::{Context, Window};

impl Workspace {
    pub(super) fn watch_keymap(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if cx.try_global::<Keymap>().is_none() {
            return; // tests that don't load a keymap
        }
        // Each load replaces the global; the first load happened before this window, and Root isn't up yet.
        self._keymap = Some(cx.observe_global_in::<Keymap>(window, Self::keymap_problems));
        cx.defer_in(window, Self::keymap_problems);
    }

    fn keymap_problems(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let keymap = cx.global::<Keymap>();
        if keymap.problems.is_empty() {
            return;
        }
        let file = keymap
            .path
            .as_ref()
            .and_then(|p| p.file_name())
            .map_or("keybindings.toml".into(), |n| n.to_string_lossy());
        let lines: Vec<String> = keymap.problems.iter().map(|p| p.to_string()).collect();
        let text = format!("{file}: skipped what it couldn't use.\n{}", lines.join("\n"));
        self.say(text, window, cx);
    }
}
