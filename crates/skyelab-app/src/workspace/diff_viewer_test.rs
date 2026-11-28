//! Own UI kit, stage 7a: `DiffViewer`, the whole-change view behind an approval's View all. Every line is there, only
//! the ones in view are built, text can be selected across rows and copied, and the keyboard scrolls it.

use super::changes_model::LineKind;
use super::diff_view::{Line, ROW_H, viewer_rows_built};
use super::diff_viewer::DiffViewer;
use crate::ui::{self, ActiveTheme as _};
use gpui::{
    AppContext as _, ClipboardItem, Entity, Modifiers, MouseButton, Pixels, Point, TestAppContext,
    VisualTestContext, accesskit, point, px,
};
use gpui_base::test_support::snapshots;

const COPY: &str = if cfg!(target_os = "macos") {
    "cmd-c"
} else {
    "ctrl-c"
};
const ALL: &str = if cfg!(target_os = "macos") {
    "cmd-a"
} else {
    "ctrl-a"
};

fn added(n: usize) -> Vec<Line> {
    (1..=n)
        .map(|i| {
            (
                None,
                Some(i as u32),
                LineKind::Added,
                format!("fn line_{i}() {{}}"),
            )
        })
        .collect()
}

fn viewer(
    cx: &mut TestAppContext,
    lines: Vec<Line>,
    breaks: Vec<usize>,
) -> (Entity<DiffViewer>, &mut VisualTestContext) {
    cx.update(|cx| {
        gpui_component::init(cx);
        ui::install_theme(cx).expect("the bundled themes parse");
        ui::bind_keys(cx);
        super::diff_viewer::bind_keys(cx);
    });
    COLUMNS.with(|c| *c.borrow_mut() = lines.iter().map(|l| l.3.chars().count()).collect());
    let v = cx.update(|cx| cx.new(|cx| DiffViewer::new("/r/src/big.rs", lines, breaks, cx)));
    let content = v.clone();
    let (_, vcx) =
        cx.add_window_view(move |window, cx| ui::window_root(content.clone().into(), window, cx));
    vcx.run_until_parked();
    (v, vcx)
}

/// The code of line `ix` as drawn (no number, no sign), when it is in view.
fn code(vcx: &mut VisualTestContext, ix: usize) -> Option<gpui::Bounds<Pixels>> {
    vcx.debug_bounds(Box::leak(format!("diff-viewer-code-{ix}").into_boxed_str()))
}

fn drag(vcx: &mut VisualTestContext, from: Point<Pixels>, to: Point<Pixels>) {
    vcx.simulate_mouse_down(from, MouseButton::Left, Modifiers::none());
    vcx.simulate_mouse_move(to, MouseButton::Left, Modifiers::none());
    vcx.simulate_mouse_up(to, MouseButton::Left, Modifiers::none());
    vcx.run_until_parked();
}

fn copied(vcx: &mut VisualTestContext) -> Option<String> {
    vcx.update(|_, cx| cx.write_to_clipboard(ClipboardItem::new_string("nothing copied".into())));
    vcx.simulate_keystrokes(COPY);
    vcx.update(|_, cx| cx.read_from_clipboard().and_then(|c| c.text()))
}

fn focus_by_click(vcx: &mut VisualTestContext) {
    let row = code(vcx, 0).expect("the first line is drawn");
    vcx.simulate_click(row.center(), Modifiers::none());
    vcx.run_until_parked();
}

/// The selection as really painted: for each of the first `lines` lines in view, the columns its selection-colored
/// quad covers, over the row's own tint. Read from the frame's quads, not from what the viewer says.
fn selected(vcx: &mut VisualTestContext, lines: usize) -> Vec<(usize, std::ops::Range<usize>)> {
    vcx.update(|window, _| window.refresh());
    vcx.run_until_parked();
    let (quads, scale, color) = vcx.update(|window, cx| {
        (
            window.painted_quads(),
            window.scale_factor(),
            cx.theme().selection,
        )
    });
    assert!(color.a > 0.);
    let mut out = Vec::new();
    for ix in 0..lines {
        let text = vcx.debug_bounds(Box::leak(format!("diff-viewer-text-{ix}").into_boxed_str()));
        let Some(text) = text else { continue };
        let (left, right) = (
            f32::from(text.left()) * scale,
            f32::from(text.right()) * scale,
        );
        let middle = f32::from(text.center().y) * scale;
        let over: Vec<_> = quads
            .iter()
            .filter(|q| {
                let (x, y, w, h) = (
                    q.bounds.origin.x.0,
                    q.bounds.origin.y.0,
                    q.bounds.size.width.0,
                    q.bounds.size.height.0,
                );
                y <= middle
                    && middle < y + h
                    && x < right
                    && x + w > left
                    && !q.background.is_transparent()
            })
            .collect();
        let Some(mark) = over.iter().find(|q| q.background.as_solid() == Some(color)) else {
            continue;
        };
        let under = over
            .iter()
            .filter(|q| q.background.as_solid() != Some(color));
        assert!(
            under.clone().all(|q| q.order < mark.order),
            "line {ix}: the selection is painted under the row"
        );
        // The test text system gives every character the same width; code here is ASCII but for one `é`.
        let chars = vcx.update(|_, _| COLUMNS.with(|c| c.borrow()[ix]));
        let column = |x: f32| ((x - left) / (right - left) * chars as f32).round() as usize;
        let (from, to) = (
            mark.bounds.origin.x.0,
            mark.bounds.origin.x.0 + mark.bounds.size.width.0,
        );
        out.push((ix, column(from)..column(to)));
    }
    out
}

thread_local! {
    /// How many characters each line under test draws, for [`selected`].
    static COLUMNS: std::cell::RefCell<Vec<usize>> = const { std::cell::RefCell::new(Vec::new()) };
}

fn click_times(vcx: &mut VisualTestContext, at: Point<Pixels>, times: usize) {
    for click_count in 1..=times {
        vcx.simulate_event(gpui::MouseDownEvent {
            button: MouseButton::Left,
            position: at,
            modifiers: Modifiers::none(),
            click_count,
            first_mouse: false,
        });
        vcx.simulate_event(gpui::MouseUpEvent {
            button: MouseButton::Left,
            position: at,
            modifiers: Modifiers::none(),
            click_count,
        });
    }
    vcx.run_until_parked();
}

#[gpui::test]
fn every_line_is_there_and_only_the_ones_in_view_are_built(cx: &mut TestAppContext) {
    let before = viewer_rows_built();
    let (_, vcx) = viewer(cx, added(2_000), Vec::new());
    let built = viewer_rows_built() - before;
    assert!(
        (1..400).contains(&built),
        "built {built} rows for the first frames"
    );
    assert!(code(vcx, 0).is_some() && code(vcx, 1_999).is_none());
    focus_by_click(vcx);
    vcx.simulate_keystrokes("end");
    vcx.run_until_parked();
    assert!(code(vcx, 1_999).is_some(), "End shows the last line");
}

/// The first and last line whose code is fully inside the viewer.
fn in_view(vcx: &mut VisualTestContext, v: &Entity<DiffViewer>) -> (usize, usize) {
    let view = v.read_with(vcx, |v, _| v.list().viewport_bounds());
    let drawn: Vec<usize> = (0..2_000)
        .filter(|ix| {
            code(vcx, *ix).is_some_and(|b| b.top() >= view.top() && b.bottom() <= view.bottom())
        })
        .collect();
    (
        *drawn.first().expect("rows in view"),
        *drawn.last().expect("rows in view"),
    )
}

#[gpui::test]
fn the_wheel_and_the_scrollbar_reach_the_whole_change_after_a_resize_too(cx: &mut TestAppContext) {
    let (v, vcx) = viewer(cx, added(2_000), Vec::new());
    for size in [None, Some(gpui::size(px(700.), px(500.)))] {
        if let Some(size) = size {
            vcx.simulate_resize(size);
            vcx.run_until_parked();
            v.update(vcx, |v, cx| {
                v.list().scroll_to(gpui::ListOffset::default());
                cx.notify()
            });
            vcx.run_until_parked();
        }
        let max = v.read_with(vcx, |v, _| v.list().max_offset_for_scrollbar().y);
        assert!(
            max >= px(1_900. * ROW_H),
            "the scrollbar covers {max:?} ({size:?})"
        );
        let at = code(vcx, 3).expect("line 4").center();
        vcx.simulate_event(gpui::ScrollWheelEvent {
            position: at,
            delta: gpui::ScrollDelta::Pixels(point(px(0.), px(-100. * ROW_H))),
            ..Default::default()
        });
        vcx.run_until_parked();
        let (first, _) = in_view(vcx, &v);
        assert!(
            (95..=105).contains(&first),
            "one 100-row wheel event shows line {first} first ({size:?})"
        );
    }
}

#[gpui::test]
fn the_keyboard_scrolls_it(cx: &mut TestAppContext) {
    let (v, vcx) = viewer(cx, added(2_000), Vec::new());
    focus_by_click(vcx);
    let top = |vcx: &mut VisualTestContext, ix| code(vcx, ix).map(|b| b.top());
    let start = top(vcx, 5).expect("line 6 is in view");
    vcx.simulate_keystrokes("down");
    vcx.run_until_parked();
    let after = top(vcx, 5).expect("still in view");
    assert!(after < start, "Down scrolls: {start:?} -> {after:?}");
    vcx.simulate_keystrokes("up");
    vcx.run_until_parked();
    assert_eq!(top(vcx, 5), Some(start), "Up scrolls back");
    let (first, last) = in_view(vcx, &v);
    vcx.simulate_keystrokes("pagedown");
    vcx.run_until_parked();
    let (next_first, next_last) = in_view(vcx, &v);
    assert!(
        next_first <= last,
        "Page Down skips no line: {last} then {next_first}"
    );
    assert!(
        next_first >= first + (last - first) / 2,
        "and moves most of a page: {next_first}"
    );
    assert!(next_last < 1_999, "not to the end");
    vcx.simulate_keystrokes("pageup");
    vcx.run_until_parked();
    assert_eq!(top(vcx, 5), Some(start), "Page Up comes back");
    vcx.simulate_keystrokes("end home");
    vcx.run_until_parked();
    assert_eq!(top(vcx, 5), Some(start), "Home shows the first line");
}

#[gpui::test]
fn a_drag_copies_the_code_alone_even_after_it_scrolled_away(cx: &mut TestAppContext) {
    let lines = vec![
        (
            Some(1),
            Some(1),
            LineKind::Context,
            "fn main() {".to_owned(),
        ),
        (Some(2), None, LineKind::Removed, "    old();".to_owned()),
        (None, Some(2), LineKind::Added, "    new(\"é\");".to_owned()),
        (Some(3), Some(3), LineKind::Context, "}".to_owned()),
    ]
    .into_iter()
    .chain((4..2_000).map(|i| (Some(i), Some(i), LineKind::Context, format!("// {i}"))))
    .collect();
    let (_, vcx) = viewer(cx, lines, Vec::new());
    let (from, to) = (code(vcx, 0).expect("line 1"), code(vcx, 3).expect("line 4"));
    drag(
        vcx,
        point(from.left(), from.center().y),
        point(to.right() - px(2.), to.center().y),
    );
    let want = "fn main() {\n    old();\n    new(\"é\");\n}";
    assert_eq!(
        copied(vcx).as_deref(),
        Some(want),
        "as drawn, no numbers or signs"
    );
    assert_eq!(
        selected(vcx, 8),
        [(0, 0..11), (1, 0..10), (2, 0..13), (3, 0..1)],
        "the selected code is painted, and nothing else"
    );
    vcx.simulate_keystrokes("end");
    vcx.run_until_parked();
    assert!(code(vcx, 0).is_none(), "the selected rows scrolled away");
    assert_eq!(
        copied(vcx).as_deref(),
        Some(want),
        "the selection outlives its rows"
    );
}

#[gpui::test]
fn select_all_a_double_and_a_triple_click_select_lines_a_word_and_a_line(cx: &mut TestAppContext) {
    let (_, vcx) = viewer(cx, added(2_000), Vec::new());
    focus_by_click(vcx);
    vcx.simulate_keystrokes(ALL);
    let all = copied(vcx).expect("copied");
    assert_eq!(all.lines().count(), 2_000);
    assert_eq!(all.lines().next(), Some("fn line_1() {}"));
    assert_eq!(all.lines().last(), Some("fn line_2000() {}"));
    let want: Vec<_> = (0..8).map(|ix| (ix, 0..14)).collect();
    assert_eq!(selected(vcx, 8), want);

    // "fn line_3() {}": the middle of `line_3` is about 6 of its 14 columns in.
    let text = vcx
        .debug_bounds("diff-viewer-text-2")
        .expect("the drawn text's own box");
    let at = point(text.left() + text.size.width * (6. / 14.), text.center().y);
    click_times(vcx, at, 2);
    assert_eq!(copied(vcx).as_deref(), Some("line_3"));
    assert_eq!(selected(vcx, 8), [(2, 3..9)]);
    click_times(vcx, at, 3);
    assert_eq!(copied(vcx).as_deref(), Some("fn line_3() {}"));
    assert_eq!(selected(vcx, 8), [(2, 0..14)]);
}

#[gpui::test]
fn tabs_draw_to_four_column_stops_and_copy_as_tabs(cx: &mut TestAppContext) {
    let lines = vec![(None, Some(1), LineKind::Added, "\tfoo\tbar".to_owned())];
    let (v, vcx) = viewer(cx, lines, Vec::new());
    assert_eq!(v.read_with(vcx, |v, _| v.drawn_text(0)), "    foo bar");
    focus_by_click(vcx);
    vcx.simulate_keystrokes(ALL);
    assert_eq!(copied(vcx).as_deref(), Some("\tfoo\tbar"));
    // From the middle of "foo" (column 5) to the end.
    let text = vcx
        .debug_bounds("diff-viewer-text-0")
        .expect("the drawn text's own box");
    let x = |col: f32| text.left() + text.size.width * (col / 11.);
    drag(
        vcx,
        point(x(5.), text.center().y),
        point(x(10.9), text.center().y),
    );
    let part = copied(vcx).expect("copied");
    assert!(part.ends_with("o\tbar") && !part.contains(' '), "{part:?}");
}

#[gpui::test]
fn a_long_line_wraps_inside_the_viewer(cx: &mut TestAppContext) {
    let long = "wrap me ".repeat(200) + "END";
    let lines = vec![
        (None, Some(1), LineKind::Added, "short".to_owned()),
        (None, Some(2), LineKind::Added, long),
        (None, Some(3), LineKind::Added, "after".to_owned()),
    ];
    let (_, vcx) = viewer(cx, lines, Vec::new());
    let window = vcx.update(|window, _| window.viewport_size().width);
    let (short, long, after) = (
        code(vcx, 0).expect("short"),
        code(vcx, 1).expect("long"),
        code(vcx, 2).expect("after"),
    );
    assert_eq!(short.size.height, px(ROW_H));
    assert!(long.size.height >= px(3. * ROW_H), "{long:?}");
    assert!(
        long.right() <= window,
        "{long:?} past the window's {window:?}"
    );
    assert!(
        after.top() >= long.bottom(),
        "the next row starts under the wrapped one"
    );
}

#[gpui::test]
fn a_long_line_without_a_space_wraps_too(cx: &mut TestAppContext) {
    let lines = vec![(None, Some(1), LineKind::Added, "x".repeat(1_600))];
    let (_, vcx) = viewer(cx, lines, Vec::new());
    let long = code(vcx, 0).expect("long");
    assert!(long.size.height >= px(3. * ROW_H), "{long:?}");
}

#[gpui::test]
fn each_multi_edit_after_the_first_starts_under_a_divider(cx: &mut TestAppContext) {
    let (_, vcx) = viewer(cx, added(5), vec![3]);
    let divider = vcx.debug_bounds("diff-viewer-break-3").expect("a divider");
    let (above, below) = (code(vcx, 2).expect("line 3"), code(vcx, 3).expect("line 4"));
    assert!(divider.top() >= above.bottom() && divider.bottom() <= below.top());
    for none in [
        "diff-viewer-break-0",
        "diff-viewer-break-1",
        "diff-viewer-break-2",
        "diff-viewer-break-4",
    ] {
        assert!(vcx.debug_bounds(none).is_none(), "{none}");
    }
}

#[gpui::test]
fn it_is_a_named_document_whose_rows_say_what_they_are(cx: &mut TestAppContext) {
    let lines = vec![
        (Some(7), Some(7), LineKind::Context, "same".to_owned()),
        (Some(8), None, LineKind::Removed, "gone".to_owned()),
        (None, Some(8), LineKind::Added, "came".to_owned()),
    ];
    let (_, vcx) = viewer(cx, lines, Vec::new());
    let tree = vcx.update(|window, _| snapshots(window));
    let documents: Vec<_> = tree
        .iter()
        .filter(|s| s.role() == Some(accesskit::Role::Document))
        .map(|s| s.label().unwrap_or_default().to_owned())
        .collect();
    assert_eq!(documents, ["Diff for /r/src/big.rs"]);
    let row = |text: &str| {
        tree.iter()
            .find(|s| s.label() == Some(text))
            .and_then(|s| s.description())
            .map(str::to_owned)
    };
    assert_eq!(row("same").as_deref(), Some("Line 7"));
    assert_eq!(row("gone").as_deref(), Some("Removed line 8"));
    assert_eq!(row("came").as_deref(), Some("Added line 8"));
}

#[gpui::test]
fn syntax_colors_come_from_the_background_and_skip_huge_lines(cx: &mut TestAppContext) {
    let huge = format!("fn b() {{ /* {} */ }}", "x".repeat(2_000));
    let lines = vec![
        (None, Some(1), LineKind::Added, "fn a() {}".to_owned()),
        (None, Some(2), LineKind::Added, huge),
        (None, Some(3), LineKind::Added, "fn c() {}".to_owned()),
    ];
    cx.update(|cx| {
        gpui_component::init(cx);
        ui::install_theme(cx).expect("the bundled themes parse");
    });
    let before = super::diff_viewer::bytes_parsed();
    let v = cx.update(|cx| cx.new(|cx| DiffViewer::new("/r/a.rs", lines, Vec::new(), cx)));
    let parsed = || super::diff_viewer::bytes_parsed() - before;
    assert_eq!(parsed(), 0, "nothing is parsed on the UI thread");
    // Background jobs only: the parse must be one of them.
    while cx.dispatcher.tick(true) {}
    assert_eq!(parsed(), 18, "the two short lines, never the huge one");

    let content = v.clone();
    let (_, vcx) =
        cx.add_window_view(move |window, cx| ui::window_root(content.clone().into(), window, cx));
    vcx.run_until_parked();
    let runs = v.read_with(vcx, |v, _| v.painted_syntax_runs());
    assert!(
        runs[0] > 0 && runs[1] == 0 && runs[2] > 0,
        "colors drawn per line: {runs:?}"
    );
}

#[gpui::test]
fn colors_that_arrive_after_the_first_frame_are_drawn(cx: &mut TestAppContext) {
    let (v, vcx) = viewer(cx, added(3), Vec::new());
    // `viewer` ran until parked with the window open: nothing but the parse's own notify redrew it.
    let runs = v.read_with(vcx, |v, _| v.painted_syntax_runs());
    assert!(runs.iter().all(|n| *n > 0), "{runs:?}");
}

#[gpui::test]
fn an_open_comment_in_one_edit_does_not_color_the_next(cx: &mut TestAppContext) {
    let lines = vec![
        (None, Some(1), LineKind::Added, "/* open".to_owned()),
        (None, Some(1), LineKind::Added, "fn x() {}".to_owned()),
    ];
    let (v, vcx) = viewer(cx, lines, vec![1]);
    assert_eq!(v.read_with(vcx, |v, _| v.span_names(0)), ["comment"]);
    assert_eq!(v.read_with(vcx, |v, _| v.span_names(1))[0], "keyword");
}
