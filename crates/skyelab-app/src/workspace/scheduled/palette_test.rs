//! Acceptance 9: "Schedule this prompt…" is offered from a composer and opens Settings → Scheduled tasks with the
//! composer's text and project in a new task.

use super::super::{NewThread, SelectProject};
use super::test_util::{open, rig};
use crate::settings::Section;
use gpui::TestAppContext;

#[gpui::test]
fn schedule_this_prompt_prefills_a_new_task(cx: &mut TestAppContext) {
    let rig = rig(true);
    let (ws, cx) = open(&rig, cx);
    cx.dispatch_action(SelectProject { id: rig.project });
    cx.run_until_parked();
    cx.dispatch_action(NewThread);
    cx.run_until_parked();
    let thread = ws
        .read_with(cx, |ws, _| ws.open_threads.last().cloned())
        .expect("a new thread tab");
    thread.update_in(cx, |t, window, cx| t.focus_prompt(window, cx));
    cx.simulate_input("triage new issues");
    cx.run_until_parked();

    // Listed in the palette while the composer has focus.
    let labels: Vec<String> = ws.update_in(cx, |ws, window, cx| {
        let focus = window.focused(cx).expect("the composer is focused");
        ws.palette_entries(&focus, window, cx)
            .into_iter()
            .map(|e| e.label.to_string())
            .collect()
    });
    assert!(
        labels.iter().any(|l| l == "Schedule this prompt…"),
        "{labels:?}"
    );

    cx.dispatch_action(super::compose::SchedulePrompt);
    cx.run_until_parked();
    let shown = ws.read_with(cx, |ws, cx| {
        ws.settings_open
            && ws.settings.as_ref().map(|s| s.read(cx).section) == Some(Section::Scheduled)
    });
    assert!(shown, "Settings → Scheduled tasks is shown");
    let page = ws
        .read_with(cx, |ws, cx| {
            ws.settings
                .as_ref()
                .and_then(|s| s.read(cx).scheduled.clone())
        })
        .expect("the page");
    let (prompt, project) = page.read_with(cx, |p, cx| {
        let e = p.editor.as_ref().expect("the editor is open");
        (e.prompt.read(cx).value().to_string(), e.draft.project)
    });
    assert_eq!(prompt, "triage new issues");
    assert_eq!(project, Some(rig.project));
}
