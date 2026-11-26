use super::{CONTEXT, Row, prepared, rows};
use crate::workspace::changes_model::LineKind::{self, Added as A, Context as C, Removed as R};

fn kinds(spec: &str) -> Vec<LineKind> {
    spec.chars()
        .map(|c| match c {
            '+' => A,
            '-' => R,
            _ => C,
        })
        .collect()
}

#[test]
fn a_long_unchanged_run_folds_keeping_context_beside_each_change() {
    // 10 unchanged, a change, 10 unchanged, a change, 10 unchanged.
    let k = kinds(&format!(
        "{}-{}+{}",
        ".".repeat(10),
        ".".repeat(10),
        ".".repeat(10)
    ));
    let r = rows(&k, |_| false);
    assert_eq!(
        r[0],
        Row::Gap {
            start: 0,
            len: 10 - CONTEXT
        }
    );
    assert_eq!(r[1..=CONTEXT], [7, 8, 9].map(Row::Line));
    assert_eq!(r[CONTEXT + 1], Row::Line(10));
    // Between the changes: CONTEXT after the first, a gap, CONTEXT before the second.
    let mid = Row::Gap {
        start: 11 + CONTEXT,
        len: 10 - 2 * CONTEXT,
    };
    assert!(r.contains(&mid));
    assert_eq!(
        r.last(),
        Some(&Row::Gap {
            start: 22 + CONTEXT,
            len: 10 - CONTEXT
        })
    );
    // Every line is either drawn or inside exactly one gap.
    let drawn: usize = r
        .iter()
        .map(|r| match r {
            Row::Line(_) => 1,
            Row::Gap { len, .. } => *len,
        })
        .sum();
    assert_eq!(drawn, k.len());
}

#[test]
fn an_open_run_shows_whole_and_short_runs_never_fold() {
    let k = kinds(&format!("{}+", ".".repeat(10)));
    let open = rows(&k, |start| start == 0);
    assert_eq!(open, (0..11).map(Row::Line).collect::<Vec<_>>());
    // Hiding one line saves nothing, so a run one longer than its context shows whole.
    let k = kinds(&format!("+{}+", ".".repeat(2 * CONTEXT + 1)));
    assert!(
        rows(&k, |_| false)
            .iter()
            .all(|r| matches!(r, Row::Line(_)))
    );
    // A file with no change at all (a Write of nothing new) shows every line.
    assert_eq!(rows(&kinds("...."), |_| false).len(), 4);
    // A Write: all added, nothing to fold.
    assert_eq!(
        rows(&kinds("+++"), |_| false),
        (0..3).map(Row::Line).collect::<Vec<_>>()
    );
}

#[test]
fn a_prepared_diff_counts_and_colors_each_side_by_its_own_text() {
    let lines = vec![
        (Some(1), Some(1), C, "def f():".to_owned()),
        (Some(2), None, R, "    return 1".to_owned()),
        (None, Some(2), A, "    return \"two\"".to_owned()),
    ];
    let p = prepared("test:prepared", "calc.py", || lines);
    assert_eq!((p.added, p.removed), (1, 1));
    let named = |ix: usize, text: &str| {
        let at = p.lines[ix].3.find(text).expect("text in line");
        p.spans[ix]
            .iter()
            .find(|(r, _)| r.contains(&at))
            .map(|s| s.1)
    };
    assert_eq!(named(0, "def"), Some("keyword"));
    assert_eq!(named(2, "\"two\""), Some("string"));
    assert_eq!(named(1, "1"), Some("number"));
    // Cached: a second call under the key never rebuilds.
    let again = prepared("test:prepared", "calc.py", || panic!("cached"));
    assert!(std::rc::Rc::ptr_eq(&p, &again));
}
