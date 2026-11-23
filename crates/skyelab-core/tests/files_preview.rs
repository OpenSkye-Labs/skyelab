//! What the preview shows for each kind of file.

use skyelab_core::files::{MAX_TEXT_BYTES, Preview, read_preview};
use std::path::Path;

#[test]
fn preview_classifies_text_image_binary_large() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    std::fs::write(root.join("a.rs"), "fn main() {}\n").expect("write");
    std::fs::write(root.join("logo.PNG"), [0x89, b'P', b'N', b'G', 0]).expect("write");
    std::fs::write(root.join("blob.bin"), [1, 2, 0, 3]).expect("write");
    std::fs::write(root.join("latin1.txt"), [b'c', b'a', b'f', 0xe9]).expect("write");
    let big = vec![b'x'; usize::try_from(MAX_TEXT_BYTES).expect("fits") + 1];
    std::fs::write(root.join("big.log"), big).expect("write");
    std::fs::create_dir(root.join("dir")).expect("mkdir");

    assert_eq!(
        read_preview(root, Path::new("a.rs")).expect("text"),
        Preview::Text("fn main() {}\n".into())
    );
    assert!(matches!(
        read_preview(root, Path::new("logo.PNG")).expect("image"),
        Preview::Image(p) if p.ends_with("logo.PNG")
    ));
    assert_eq!(
        read_preview(root, Path::new("blob.bin")).expect("binary"),
        Preview::Binary
    );
    assert_eq!(
        read_preview(root, Path::new("latin1.txt")).expect("not utf-8"),
        Preview::Binary
    );
    assert_eq!(
        read_preview(root, Path::new("big.log")).expect("large"),
        Preview::TooLarge(MAX_TEXT_BYTES + 1)
    );
    assert!(read_preview(root, Path::new("dir")).is_err());
}
