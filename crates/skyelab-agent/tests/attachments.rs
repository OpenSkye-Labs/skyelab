//! Acceptance: attached files reach the agent as Claude content blocks or Codex input items, within the caps.
#![cfg(unix)]

#[path = "common/composer.rs"]
mod composer;

use composer::{bound, codex_fake, codex_requests, launch, lines_of, recorder};
use serde_json::Value;
use skyelab_agent::{Attachment, AttachmentKind, MAX_ATTACHMENT_BYTES, start_with_bin};
use skyelab_core::Provider;

const PNG: &[u8] = &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 1, 2, 3];

fn files() -> Vec<Attachment> {
    vec![
        Attachment::new("shot.png", PNG.to_vec()).expect("png"),
        Attachment::new("notes.md", b"# hi".to_vec()).expect("text"),
    ]
}

#[test]
fn attachments_become_claude_content_blocks() {
    let f = bound(Provider::Claude);
    let bin = recorder(&f);
    let files = files();
    let mut s = start_with_bin(&bin, &f.config, &launch(&f, None, None, &files)).expect("start");
    let pdf = Attachment::new("a.pdf", b"%PDF-1.7 x".to_vec()).expect("pdf");
    s.send_message("look", std::slice::from_ref(&pdf))
        .expect("send");
    let lines = lines_of(&f, "stdin.txt", 2);
    let first: Value = serde_json::from_str(&lines[0]).expect("json");
    let content = &first["message"]["content"];
    assert_eq!(content[0]["type"], "image");
    assert_eq!(content[0]["source"]["media_type"], "image/png");
    assert_eq!(content[0]["source"]["data"], "iVBORw0KGgoBAgM=");
    assert_eq!(content[1]["type"], "document");
    assert_eq!(content[1]["source"]["type"], "text");
    assert_eq!(content[1]["source"]["data"], "# hi");
    assert_eq!(content[1]["title"], "notes.md");
    assert_eq!(
        content[2],
        serde_json::json!({ "type": "text", "text": "hi" })
    );
    let second: Value = serde_json::from_str(&lines[1]).expect("json");
    let c = &second["message"]["content"];
    assert_eq!(c[0]["type"], "document");
    assert_eq!(c[0]["source"]["media_type"], "application/pdf");
    assert_eq!(c[1]["text"], "look");
}

#[test]
fn a_plain_prompt_stays_a_string() {
    let f = bound(Provider::Claude);
    let bin = recorder(&f);
    let _s = start_with_bin(&bin, &f.config, &launch(&f, None, None, &[])).expect("start");
    let lines = lines_of(&f, "stdin.txt", 1);
    let v: Value = serde_json::from_str(&lines[0]).expect("json");
    assert_eq!(v["message"]["content"], "hi");
}

#[test]
fn codex_attachments_become_input_items() {
    let f = bound(Provider::Codex);
    let fake = codex_fake();
    let files = files();
    let s = start_with_bin(&fake, &f.config, &launch(&f, None, None, &files)).expect("start");
    let turn = codex_requests(&s, "turn/start", 1);
    let input = &turn[0]["input"];
    assert_eq!(
        input[0],
        serde_json::json!({ "type": "text", "text": "hi" })
    );
    assert_eq!(input[1]["type"], "image");
    assert_eq!(input[1]["url"], "data:image/png;base64,iVBORw0KGgoBAgM=");
    assert_eq!(input[2]["type"], "text");
    assert!(input[2]["text"].as_str().expect("text").contains("# hi"));
}

#[test]
fn caps_and_kinds_are_checked() {
    let big = vec![b'a'; MAX_ATTACHMENT_BYTES + 1];
    assert!(Attachment::new("big.txt", big).is_err());
    assert!(Attachment::new("bin.dat", vec![0xFF, 0xFE, 0x00, 0x81]).is_err());
    // The kind comes from the bytes, not the name.
    let fake_png = Attachment::new("x.png", b"just text".to_vec()).expect("text");
    assert_eq!(fake_png.kind, AttachmentKind::Text);

    let f = bound(Provider::Claude);
    let bin = recorder(&f);
    let many: Vec<Attachment> = (0..11)
        .map(|i| Attachment::new(format!("{i}.txt"), b"x".to_vec()).expect("text"))
        .collect();
    assert!(start_with_bin(&bin, &f.config, &launch(&f, None, None, &many)).is_err());
}
