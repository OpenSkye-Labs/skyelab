//! "Schedule this prompt…": from a composer, the Scheduled tasks editor with its text and project.

use super::super::Workspace;
use super::super::thread::ThreadView;
use crate::settings::Section;
use gpui::{Context, Window, actions};

actions!(skyelab, [SchedulePrompt]);

/// Opens Settings → Scheduled tasks with a new task holding this prompt and project.
#[derive(Clone, PartialEq, gpui::Action)]
#[action(namespace = skyelab, no_json)]
pub(crate) struct ScheduleText {
    pub prompt: String,
    pub project: i64,
}

impl ThreadView {
    pub(in crate::workspace) fn schedule_prompt(
        &mut self,
        _: &SchedulePrompt,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let action = ScheduleText {
            prompt: self.prompt.read(cx).value().to_string(),
            project: self.project_id,
        };
        crate::workspace::dispatch(Box::new(action), window, cx);
    }
}

impl Workspace {
    pub(in crate::workspace) fn schedule_text(
        &mut self,
        a: &ScheduleText,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.is_remote() {
            return; // tasks run on this machine; another machine's composer is not this one's project
        }
        self.open_settings_at(Section::Scheduled, window, cx);
        let page = self
            .settings
            .as_ref()
            .and_then(|s| s.read(cx).scheduled.clone());
        if let Some(page) = page {
            let prompt = a.prompt.clone();
            page.update(cx, |p, cx| {
                p.new_task(Some(prompt), Some(a.project), window, cx)
            });
        }
    }
}
