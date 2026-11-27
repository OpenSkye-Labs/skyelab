//! Criterion 17: a caller's title is stored and drawn without control or bidi-override characters.

use super::remote_delegation_harness::*;
use gpui::TestAppContext;

#[gpui::test]
fn the_parent_title_is_cleaned_before_it_is_stored_or_drawn(cx: &mut TestAppContext) {
    let (b, cx) = b_side(
        Setup {
            link_to: Some("work"),
            ..Setup::default()
        },
        cx,
    );
    let mut from = caller();
    from.title = "fix\u{202E}it\u{0000}\nnow".into();
    let id = started(&b, &from, start_op(b.project("work-a"), "r1"), cx);
    let origin = b.store.lock().expect("lock").delegated_from(id).expect("read").expect("origin");
    assert_eq!(origin.parent_title, "fixitnow");
    let view = b.ws.read_with(cx, |ws, cx| ws.open_view(id, cx)).expect("its tab");
    let label = view.read_with(cx, |v, _| v.delegated_label()).expect("a label");
    assert_eq!(label.to_string(), "Delegated by fixitnow on phone");
}
