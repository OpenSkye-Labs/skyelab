//! "Schedule this prompt…": from a composer, the Bots page's editor with its text and project.

use super::super::Workspace;
use super::super::center::Page;
use super::super::thread::ThreadView;
use gpui::{Context, Window, actions};

actions!(skyelab, [SchedulePrompt]);

/// Opens the Bots page with a new task holding this prompt and project.
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
        self.show(Page::Bots, window, cx);
        let page = self.bots_page(window, cx).read(cx).list.clone();
        let prompt = a.prompt.clone();
        page.update(cx, |p, cx| {
            p.new_task(Some(prompt), Some(a.project), window, cx)
        });
    }
}
