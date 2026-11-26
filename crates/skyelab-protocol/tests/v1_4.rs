//! Protocol 1.4: a thread's pin and reply preview, and answering an AskUserQuestion. Each addition is optional on the
//! wire, so a 1.3 peer's messages still parse.

use serde_json::json;
use skyelab_protocol::{Call, Request, Thread, image_label};

#[test]
fn thread_pin_and_preview_round_trip() {
    let thread = Thread {
        id: 3,
        project_id: 1,
        title: "t".into(),
        account_id: "work".into(),
        account_label: None,
        status: "exited".into(),
        pinned: true,
        preview: Some("Done. **Tests pass.**".into()),
        settled_at: None,
        snoozed_until: None,
        created_at: 1,
        updated_at: 2,
        row: Default::default(),
    };
    let wire = serde_json::to_value(&thread).expect("serializes");
    assert_eq!(wire["pinned"], true);
    assert_eq!(wire["preview"], "Done. **Tests pass.**");
    assert_eq!(
        serde_json::from_value::<Thread>(wire).expect("parses"),
        thread
    );
    // Unpinned and without a reply: neither field is sent, so a 1.3 client sees what it always did.
    let plain = Thread {
        pinned: false,
        preview: None,
        ..thread
    };
    let wire = serde_json::to_value(&plain).expect("serializes");
    assert!(wire.get("pinned").is_none() && wire.get("preview").is_none());
}

#[test]
fn a_1_3_thread_has_no_pin_or_preview() {
    let old = json!({"id": 3, "project_id": 1, "title": "t", "account_id": "work", "status": "idle",
                     "created_at": 1, "updated_at": 2});
    let thread: Thread = serde_json::from_value(old).expect("a 1.3 thread parses");
    assert!(!thread.pinned);
    assert_eq!(thread.preview, None);
}

#[test]
fn round_trip_answer_questions() {
    let req = Request {
        id: 7,
        call: Call::AnswerQuestions {
            thread_id: 3,
            request_id: "ask-1".into(),
            answers: vec![vec!["Rust".into()], vec!["Tests".into(), "Docs".into()]],
        },
    };
    let text = serde_json::to_string(&req).expect("serializes");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&text).expect("json"),
        json!({"id": 7, "method": "answer_questions", "params": {"thread_id": 3, "request_id": "ask-1",
               "answers": [["Rust"], ["Tests", "Docs"]]}})
    );
    assert_eq!(Request::parse(&text).expect("parses"), req);
}

#[test]
fn image_labels_say_whether_the_bytes_were_kept() {
    let dropped = json!({"type": "image", "source": {"type": "base64", "media_type": "image/png",
                         "omitted_bytes": 210_039, "sha256": "ab"}});
    assert_eq!(image_label(&dropped), "Image (205 KB) not kept");
    let live = json!({"type": "image", "source": {"type": "base64", "data": "A".repeat(4000)}});
    assert_eq!(image_label(&live), "Image (3 KB)");
    assert_eq!(image_label(&json!({"type": "image"})), "Image");
}
