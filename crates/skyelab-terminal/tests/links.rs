use skyelab_terminal::{LinkKind, find_links, path_target};

fn found(line: &str) -> Vec<(String, LinkKind)> {
    let chars: Vec<char> = line.chars().collect();
    find_links(line)
        .into_iter()
        .map(|l| (chars[l.start..l.end].iter().collect(), l.kind))
        .collect()
}

#[test]
fn detects_urls_and_trims_punctuation() {
    assert_eq!(
        found("see https://example.com/a?b=1, or (http://x.test/y)."),
        [
            ("https://example.com/a?b=1".to_string(), LinkKind::Url),
            ("http://x.test/y".to_string(), LinkKind::Url),
        ]
    );
}

#[test]
fn rejects_other_schemes() {
    assert!(found("file:///etc/passwd ssh://host/x javascript:alert(1) ftp://a/b").is_empty());
}

#[test]
fn detects_paths_with_line_suffix() {
    assert_eq!(
        found("error at src/main.rs:12:5: bad, see ./README.md and Cargo.toml."),
        [
            ("src/main.rs:12:5".to_string(), LinkKind::Path),
            ("./README.md".to_string(), LinkKind::Path),
            ("Cargo.toml".to_string(), LinkKind::Path),
        ]
    );
    assert_eq!(path_target("src/main.rs:12:5"), "src/main.rs");
    assert_eq!(path_target("a/b.rs"), "a/b.rs");
}

#[test]
fn plain_words_and_numbers_are_not_links() {
    assert!(found("hello world 1/2 3.14 ...").is_empty());
}
