use super::text_edit::{Buffer, Intent, MAX_INSERT, clip, one_line};

fn buffer(text: &str) -> Buffer {
    let mut b = Buffer::default();
    b.set_text(text);
    b
}

#[test]
fn graphemes_are_one_step() {
    let b = buffer("a👍🏽e\u{301}x");
    assert_eq!(b.next_grapheme(1), 9);
    assert_eq!(b.prev_grapheme(9), 1);
    assert_eq!(b.next_grapheme(9), 12);
    assert_eq!(b.prev_grapheme(0), 0);
    assert_eq!(b.next_grapheme(b.text().len()), b.text().len());
    assert_eq!(b.snap(5), 1, "inside the emoji snaps to its start");
    assert_eq!(b.snap(999), b.text().len());
}

#[test]
fn words_skip_the_space_before_them() {
    let b = buffer("foo  bar-baz");
    assert_eq!(b.prev_word(5), 0);
    assert_eq!(b.prev_word(8), 5);
    assert_eq!(b.next_word(3), 8);
    assert_eq!(b.next_word(12), 12);
    assert_eq!(b.word_at(6), 5..8);
    assert_eq!(b.word_at(4), 3..5, "a run of spaces is a word of its own");
    assert_eq!(b.word_at(8), 8..9, "punctuation is one character");
    assert_eq!(b.word_at(12), 9..12, "past the end takes the last word");
    assert_eq!(buffer("").word_at(0), 0..0);
    assert_eq!(
        buffer("你好").word_at(0),
        0..3,
        "no spaces, so one ideograph"
    );
    assert_eq!(buffer("καλή 한국어").word_at(0), 0..8);
    assert_eq!(buffer("καλή 한국어").word_at(12), 9..18);
}

#[test]
fn utf16_offsets_convert_both_ways() {
    let b = buffer("😀a你b");
    for (bytes, utf16) in [(0, 0), (4, 2), (5, 3), (8, 4), (9, 5)] {
        assert_eq!(b.to_utf16(bytes), utf16);
        assert_eq!(b.bytes_of_utf16(&(utf16..utf16)), bytes..bytes);
    }
    assert_eq!(
        b.bytes_of_utf16(&(1..3)),
        0..5,
        "half a surrogate pair widens"
    );
    assert_eq!(b.bytes_of_utf16(&(0..1)), 0..4);
    assert_eq!(b.bytes_of_utf16(&(1..1)), 0..0, "a caret stays a caret");
    assert_eq!(b.bytes_of_utf16(&(90..99)), 9..9, "past the end clamps");
    let reversed = std::ops::Range { start: 3, end: 1 };
    assert_eq!(b.bytes_of_utf16(&reversed), 0..5);
}

#[test]
fn runs_of_one_kind_are_one_undo_step_until_the_caret_moves() {
    let mut b = buffer("");
    for c in ["a", "b", "c"] {
        b.replace(b.range(), c, Intent::Typing);
    }
    b.move_to(1);
    b.replace(b.range(), "x", Intent::Typing);
    assert_eq!(b.text(), "axbc");
    assert!(b.undo());
    assert_eq!((b.text(), b.head()), ("abc", 1));
    assert!(b.undo());
    assert_eq!(b.text(), "");
    assert!(!b.undo());
    assert!(b.redo() && b.redo());
    assert_eq!((b.text(), b.head()), ("axbc", 2));
    assert!(!b.redo());

    b.move_to(4);
    for at in [4, 3] {
        b.replace(at - 1..at, "", Intent::Backspace);
    }
    assert_eq!(b.text(), "ax");
    b.undo();
    assert_eq!(
        (b.text(), b.head()),
        ("axbc", 4),
        "two backspaces, one step"
    );

    b.move_to(0);
    b.replace(0..1, "", Intent::DeleteForward);
    b.replace(0..1, "", Intent::DeleteForward);
    b.undo();
    assert_eq!((b.text(), b.head()), ("axbc", 0));
}

#[test]
fn an_edit_after_an_undo_drops_the_redo() {
    let mut b = buffer("");
    b.replace(0..0, "a", Intent::Atomic);
    b.undo();
    b.replace(0..0, "b", Intent::Atomic);
    assert!(!b.redo());
    assert_eq!(b.text(), "b");
}

#[test]
fn a_composition_is_one_step_and_puts_back_what_it_replaced() {
    let mut b = buffer("hello");
    b.select(0, 5);
    b.compose(b.range(), "n", None);
    b.compose(b.marked().expect("marked"), "ni", None);
    b.compose(b.marked().expect("marked"), "你", None);
    assert_eq!((b.text(), b.marked()), ("你", Some(0..3)));
    b.unmark();
    b.replace(b.range(), "x", Intent::Typing);
    b.undo();
    assert_eq!(b.text(), "你", "typing after it is its own step");
    b.undo();
    assert_eq!((b.text(), b.range()), ("hello", 0..5));
}

#[test]
fn an_emptied_composition_leaves_no_mark() {
    let mut b = buffer("a");
    b.compose(1..1, "k", None);
    b.compose(b.marked().expect("marked"), "", None);
    assert_eq!((b.text(), b.marked()), ("a", None));
}

#[test]
fn inserted_text_is_one_line_and_at_most_the_cap() {
    assert_eq!(one_line("a\nb\tc\r"), "abc");
    assert_eq!(clip("short"), "short");
    assert_eq!(clip(&"€".repeat(30_000)).len(), MAX_INSERT - 1);
    // The cap falls inside the last thumb-with-skin-tone (8 bytes): it goes whole.
    let thumbs = format!("abc{}", "👍🏽".repeat(MAX_INSERT / 8 + 1));
    let cut = clip(&thumbs);
    assert_eq!(cut.len(), 3 + (MAX_INSERT - 3) / 8 * 8);
    assert!(cut.ends_with("👍🏽"));
    // One endless cluster can't be kept whole: it is cut between characters.
    let cluster = format!("e{}", "\u{301}".repeat(MAX_INSERT));
    assert_eq!(clip(&cluster).len(), MAX_INSERT - 1);
}
