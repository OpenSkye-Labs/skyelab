//! Which fences become diagrams: only a closed `mermaid` fence; a streamed one still open stays code.

use super::mermaid::{Block, block};
use gpui_component::text::markdown_ast::{Code, Node};

fn code(lang: Option<&str>, value: &str) -> Node {
    Node::Code(Code {
        value: value.into(),
        position: None,
        lang: lang.map(Into::into),
        meta: None,
    })
}

#[test]
fn fence_closed_mermaid_is_a_diagram() {
    let raw = "```mermaid\nflowchart LR\n  A-->B\n```";
    assert_eq!(
        block(&code(Some("mermaid"), "flowchart LR\n  A-->B"), raw),
        Some(Block {
            source: "flowchart LR\n  A-->B".into(),
            closed: true
        })
    );
}

#[test]
fn fence_language_is_case_insensitive_and_tilde_fences_count() {
    let b = block(
        &code(Some("Mermaid"), "graph TD"),
        "~~~Mermaid\ngraph TD\n~~~\n",
    )
    .expect("mermaid");
    assert!(b.closed);
}

#[test]
fn fence_still_streaming_is_not_closed() {
    let raw = "```mermaid\nflowchart LR\n  A-->";
    let b = block(&code(Some("mermaid"), "flowchart LR\n  A-->"), raw).expect("mermaid");
    assert!(!b.closed);
    // Only the opening line so far.
    assert!(
        !block(&code(Some("mermaid"), ""), "```mermaid")
            .expect("mermaid")
            .closed
    );
}

#[test]
fn fence_closed_by_a_shorter_or_other_fence_is_not_closed() {
    let raw = "````mermaid\ngraph TD\n```";
    assert!(
        !block(&code(Some("mermaid"), "graph TD\n```"), raw)
            .expect("mermaid")
            .closed
    );
    let raw = "```mermaid\ngraph TD\n~~~";
    assert!(
        !block(&code(Some("mermaid"), "graph TD\n~~~"), raw)
            .expect("mermaid")
            .closed
    );
}

#[test]
fn fence_in_a_quote_or_list_still_closes() {
    let raw = "> ```mermaid\n> graph TD\n> ```";
    assert!(
        block(&code(Some("mermaid"), "graph TD"), raw)
            .expect("mermaid")
            .closed
    );
    let raw = "   ```mermaid\n   graph TD\n   ```";
    assert!(
        block(&code(Some("mermaid"), "graph TD"), raw)
            .expect("mermaid")
            .closed
    );
}

#[test]
fn fence_of_another_language_or_none_is_left_alone() {
    assert_eq!(
        block(
            &code(Some("rust"), "fn main() {}"),
            "```rust\nfn main() {}\n```"
        ),
        None
    );
    assert_eq!(block(&code(None, "graph TD"), "```\ngraph TD\n```"), None);
    assert_eq!(
        block(
            &Node::Text(gpui_component::text::markdown_ast::Text {
                value: "mermaid".into(),
                position: None
            }),
            "mermaid"
        ),
        None
    );
}
