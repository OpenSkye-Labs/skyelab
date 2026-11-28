//! Stage 5b acceptance: our dock engine redraws no more than gpui-base's did. The same steps run on gpui-base's engine
//! under gpui-component's skin and on ours, and each fixture panel counts its renders (`debug_renders`). Panels are
//! drawn as cached views, so a step that doesn't change a panel or its bounds must not render it again.

use super::dock_engine_parity_test::{Side, Spec, draw};
use crate::debug_renders::take_test_count;
use gpui::{Modifiers, TestAppContext};

const SPEC: Spec = &[
    (Side::Right, &[("A", true), ("B", false)], 300., true),
    (Side::Bottom, &[("Bottom", false)], 200., true),
];
const PANELS: [&str; 4] = ["Center", "A", "B", "Bottom"];

#[derive(Clone, Copy, Debug)]
enum Step {
    Notify(&'static str),
    Select(&'static str),
    Click(&'static str),
    Toggle(Side),
    Zoom(&'static str),
}

const STEPS: [Step; 10] = [
    Step::Notify("A"),
    Step::Select("B"),
    Step::Notify("B"),
    Step::Click("dock-tab-1"),
    Step::Toggle(Side::Bottom),
    Step::Toggle(Side::Bottom),
    Step::Click("dock-tab-0"),
    Step::Zoom("A"),
    Step::Zoom("A"),
    Step::Notify("Bottom"),
];

fn counts() -> Vec<u64> {
    PANELS.iter().map(|p| take_test_count(p)).collect()
}

/// Each step's renders per panel, in `PANELS` order.
fn run(reference: bool, cx: &mut TestAppContext) -> Vec<Vec<u64>> {
    let mut d = draw(reference, SPEC, cx);
    d.vcx.run_until_parked();
    counts();
    let mut out = Vec::new();
    for step in STEPS {
        match step {
            Step::Notify(name) => {
                let p = d.panel(name);
                p.update(d.vcx, |_, cx| cx.notify());
            }
            Step::Click(selector) => {
                let at = d
                    .vcx
                    .debug_bounds(selector)
                    .expect("the tab is drawn")
                    .center();
                d.vcx.simulate_click(at, Modifiers::none());
            }
            Step::Select(name) => d.select(name),
            Step::Toggle(side) => d.toggle(side),
            Step::Zoom(name) => d.toggle_zoom(name),
        }
        d.vcx.run_until_parked();
        out.push(counts());
    }
    out
}

#[gpui::test]
fn our_engine_renders_no_panel_more_than_gpui_base_did(cx: &mut TestAppContext) {
    super::super::button_test::setup(cx);
    let theirs = run(true, cx);
    let ours = run(false, cx);
    for (i, step) in STEPS.iter().enumerate() {
        for (j, panel) in PANELS.iter().enumerate() {
            assert!(
                ours[i][j] <= theirs[i][j],
                "{step:?}: {panel} rendered {} times under ours, {} under gpui-base",
                ours[i][j],
                theirs[i][j]
            );
        }
    }
    // A notify of a right-dock panel and a tab select there leave every other panel's cache alone. (A clicked tab
    // moves the focus, and a focus move redraws every cached view, on either engine.)
    assert_eq!(ours[0], [0, 1, 0, 0], "{:?}: only A", STEPS[0]);
    assert_eq!(ours[1], [0, 0, 1, 0], "{:?}: only B", STEPS[1]);
    assert_eq!(ours[2], [0, 0, 1, 0], "{:?}: only B", STEPS[2]);
    assert_eq!(ours[9], [0, 0, 0, 1], "{:?}: only Bottom", STEPS[9]);
}
