//! Hooks the agent runs from the account's own config (Codex's `hook/started` / `hook/completed`): while one runs,
//! the working row names it, so a slow or stuck hook isn't a bare "Working".

use super::ThreadView;
use crate::ui;
use gpui::Context;

/// After this long a running hook gets a hint under the working row.
pub(super) const SLOW_HOOK_MS: i64 = 30_000;

/// A hook running now: its run id, what it is, and since when (ms since the epoch).
pub(in crate::workspace) struct Hook {
    id: String,
    name: String,
    since_ms: i64,
}

/// The working row for hook `name` that has run `ran_ms`, and the hint once it is slow.
pub(super) fn hook_line(name: &str, ran_ms: i64) -> (String, Option<&'static str>) {
    let line = format!("Running hook {name}… {}", ui::elapsed(ran_ms));
    let hint = (ran_ms >= SLOW_HOOK_MS).then_some(
        "This hook comes from the account's own Codex hooks.json and is taking a while. Interrupt (⌘.) stops the turn.",
    );
    (line, hint)
}

impl ThreadView {
    /// A hook started or finished.
    pub(in crate::workspace) fn on_hook(
        &mut self,
        id: String,
        name: String,
        running: bool,
        cx: &mut Context<Self>,
    ) {
        self.hooks.retain(|h| h.id != id);
        if running {
            self.hooks.push(Hook {
                id,
                name,
                since_ms: ui::now_ms(),
            });
        }
        cx.notify();
    }

    /// The working row's text while a hook runs (the newest one), with its hint.
    pub(super) fn running_hook(&self) -> Option<(String, Option<&'static str>)> {
        let h = self.hooks.last()?;
        Some(hook_line(&h.name, ui::now_ms() - h.since_ms))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_hook_line_names_the_hook_and_hints_once_slow() {
        let (line, hint) = hook_line("sessionStart (Reporting status to Orca)", 12_000);
        assert_eq!(
            line,
            "Running hook sessionStart (Reporting status to Orca)… 12s"
        );
        assert!(hint.is_none());
        let (_, hint) = hook_line("sessionStart", SLOW_HOOK_MS);
        assert!(hint.is_some_and(|h| h.contains("Interrupt")));
    }
}
