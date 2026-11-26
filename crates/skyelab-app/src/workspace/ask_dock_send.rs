//! Next or Submit in place of Send while a question is docked (T3's labels), live once the step has an answer.

use crate::ui::Button;
use crate::workspace::thread::ThreadView;
use gpui::{AnyElement, Context, InteractiveElement as _, IntoElement as _};

impl ThreadView {
    /// Next or Submit in place of Send while a question is docked.
    pub(in crate::workspace) fn render_dock_send(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let (step, n) = self.dock_step()?;
        let label = next_label(step, n);
        // Live once the step has an answer: text in the composer (not yet in the draft), a pick or a file.
        let answered = !self.prompt.read(cx).value().trim().is_empty()
            || self.dock().is_some_and(|a| self.step_answered(&a.id, step));
        Some(
            Button::new("dock-next")
                .debug_selector(|| "dock-next".into())
                .primary()
                .small()
                .label(label)
                .disabled(!answered)
                .tooltip(format!("{label} ({})", crate::ui::keys("⌘↩")))
                .on_click(cx.listener(|this, _, window, cx| {
                    // A pick just moved the question on: this click was aimed at the button before it changed.
                    let just = this
                        .composer
                        .dock_advanced
                        .is_some_and(|at| at.elapsed() < ADVANCE_GUARD);
                    if !just {
                        this.advance_question(window, cx)
                    }
                }))
                .into_any_element(),
        )
    }
}

/// How long after a pick moved the question on the Next/Submit button ignores clicks.
const ADVANCE_GUARD: std::time::Duration = std::time::Duration::from_millis(500);

/// T3's labels: "Next question" until the last, then "Submit answer" (one question) or "Submit answers". They differ
/// in width as well as word, so a button that changed under the pointer reads as changed.
pub(in crate::workspace) fn next_label(step: usize, n: usize) -> &'static str {
    if step + 1 < n {
        "Next question"
    } else if n > 1 {
        "Submit answers"
    } else {
        "Submit answer"
    }
}
