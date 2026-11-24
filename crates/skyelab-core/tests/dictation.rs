use skyelab_core::dictation::{Dictation, Edit, Phase, Refusal};

fn edit(text: &str, cursor: usize) -> Option<Edit> {
    Some(Edit {
        text: text.into(),
        cursor,
    })
}

#[test]
fn partials_replace_the_dictated_span_at_the_cursor() {
    let mut d = Dictation::default();
    // Cursor after "fix " in "fix the bug".
    let s = d.start("fix the bug", 4..4).expect("idle starts");
    assert_eq!(d.phase(), &Phase::Listening);
    assert_eq!(d.heard(s, "only", false), edit("fix only the bug", 8));
    // A later partial revises the whole span, not appends to it.
    assert_eq!(
        d.heard(s, "only really", false),
        edit("fix only really the bug", 15)
    );
    assert_eq!(d.heard(s, "only", true), edit("fix only the bug", 8));
    assert_eq!(d.phase(), &Phase::Idle, "a final result ends it");
}

#[test]
fn the_selection_at_start_is_replaced_and_spaces_are_added() {
    let mut d = Dictation::default();
    // "the bug" selected in "fix the bug."
    let s = d.start("fix the bug.", 4..11).expect("starts");
    assert_eq!(d.heard(s, "it", false), edit("fix it.", 6));
    // Touching words get a space between them; an empty draft gets none.
    let mut d = Dictation::default();
    let s = d.start("hello", 5..5).expect("starts");
    assert_eq!(d.heard(s, "world", false), edit("hello world", 11));
    let mut d = Dictation::default();
    let s = d.start("tail", 0..0).expect("starts");
    assert_eq!(d.heard(s, "head", false), edit("head tail", 4));
    let mut d = Dictation::default();
    let s = d.start("", 0..0).expect("starts");
    assert_eq!(d.heard(s, "hi", false), edit("hi", 2));
}

#[test]
fn a_selection_off_char_boundaries_is_clamped() {
    let mut d = Dictation::default();
    // "é" is two bytes: 1 is inside it, 99 past the end.
    let s = d.start("é", 1..99).expect("starts");
    assert_eq!(d.heard(s, "a", false), edit("a", 1));
}

#[test]
fn cancel_restores_the_draft_and_its_cursor() {
    let mut d = Dictation::default();
    let s = d.start("keep me", 4..7).expect("starts");
    d.heard(s, "words", false);
    assert_eq!(d.cancel(), edit("keep me", 7));
    assert_eq!(d.phase(), &Phase::Idle);
    assert_eq!(d.cancel(), None, "nothing to cancel when idle");
}

#[test]
fn events_from_an_earlier_session_are_dropped() {
    let mut d = Dictation::default();
    let old = d.start("a", 1..1).expect("starts");
    d.cancel();
    let new = d.start("b", 1..1).expect("starts again");
    assert_ne!(old, new);
    assert_eq!(d.heard(old, "late", true), None);
    assert!(!d.failed(old, "late".into()));
    assert_eq!(d.phase(), &Phase::Listening);
    assert_eq!(d.start("b", 0..0), None, "one dictation at a time");
}

#[test]
fn stop_waits_for_the_final_and_an_error_after_words_keeps_them() {
    let mut d = Dictation::default();
    let s = d.start("", 0..0).expect("starts");
    d.heard(s, "some words", false);
    assert!(d.stop());
    assert_eq!(d.phase(), &Phase::Finishing);
    assert!(!d.stop(), "already stopping");
    // The recognizer ends with an error after it heard words: the words stay, no error shows.
    assert!(d.failed(s, "No speech detected".into()));
    assert_eq!(d.phase(), &Phase::Idle);
}

#[test]
fn a_failure_shows_until_dismissed() {
    let mut d = Dictation::default();
    let s = d.start("draft", 5..5).expect("starts");
    assert!(d.failed(s, Refusal::MicDenied.to_string()));
    let Phase::Failed(why) = d.phase() else {
        panic!("failed: {:?}", d.phase())
    };
    assert!(why.contains("Microphone"), "{why}");
    // A new start clears it.
    assert!(d.start("draft", 5..5).is_some());
    d.cancel();
    d.failed(0, "x".into());
    d.dismiss();
    assert_eq!(d.phase(), &Phase::Idle);
}

#[test]
fn ending_for_a_send_keeps_the_text_and_drops_the_rest() {
    let mut d = Dictation::default();
    let s = d.start("", 0..0).expect("starts");
    d.heard(s, "send this", false);
    assert!(d.end());
    assert_eq!(d.phase(), &Phase::Idle);
    assert_eq!(
        d.heard(s, "send this now", true),
        None,
        "the final after a send is dropped"
    );
    assert!(!d.end(), "nothing running");
}

#[test]
fn the_mode_records_whether_audio_leaves_the_mac() {
    let mut d = Dictation::default();
    let s = d.start("", 0..0).expect("starts");
    assert!(!d.server(), "on-device until told otherwise");
    d.started(s, false);
    assert!(d.server());
    d.cancel();
    assert!(!d.server());
}

#[test]
fn refusals_say_what_to_do() {
    for (r, words) in [
        (Refusal::MicDenied, "Privacy & Security → Microphone"),
        (
            Refusal::SpeechDenied,
            "Privacy & Security → Speech Recognition",
        ),
        (Refusal::Restricted, "restricted"),
        (Refusal::NoRecognizer, "language"),
        (Refusal::NotPackaged, "SkyeLab.app"),
    ] {
        let text = r.to_string();
        assert!(text.contains(words), "{r:?}: {text}");
    }
}

#[test]
fn no_spaces_around_cjk_or_inside_brackets() {
    let mut d = Dictation::default();
    let s = d.start("修复。", 6..6).expect("starts");
    assert_eq!(d.heard(s, "错误", false), edit("修复错误。", 12));
    let mut d = Dictation::default();
    let s = d.start("hello 世界", 6..6).expect("starts");
    assert_eq!(d.heard(s, "你好", false), edit("hello 你好世界", 12));
    let mut d = Dictation::default();
    let s = d.start("call ()", 6..6).expect("starts");
    assert_eq!(d.heard(s, "x", false), edit("call (x)", 7));
}

#[test]
fn shown_is_what_dictation_last_put_in_the_prompt() {
    let mut d = Dictation::default();
    let s = d.start("draft", 5..5).expect("starts");
    assert_eq!(d.shown(), "draft");
    d.heard(s, "more", false);
    assert_eq!(d.shown(), "draft more");
}
