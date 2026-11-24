use super::{CSP, PageTheme, prepare};

fn theme(dark: bool) -> PageTheme {
    PageTheme {
        dark,
        vars: vec![
            ("--background", "#101010".into()),
            (
                "--font-sans",
                "\"Inter\";}</style><script>x()</script>".into(),
            ),
        ],
    }
}

const META: &str = "<meta http-equiv=\"Content-Security-Policy\"";

#[test]
fn our_doctype_and_policy_come_before_anything_of_the_page() {
    let leads = [
        "",
        "  ",
        "\u{0B}",
        "\u{A0}",
        "\u{2028}",
        "\u{3000}",
        "\u{FEFF}",
        "<!-- x -->",
    ];
    for lead in leads {
        for page in [
            "<!DOCTYPE html><html><head><script>1</script>",
            "<script>1</script>",
            "",
            "<!doc",
        ] {
            let page = format!("{lead}{page}");
            let out = prepare(&page, &theme(false));
            assert!(
                out.starts_with(&format!("<!doctype html>{META}")),
                "{lead:?}: {out}"
            );
            assert!(out.ends_with(&page));
        }
    }
}

#[test]
fn the_policy_blocks_requests_and_frames_and_allows_only_https_resources() {
    for rule in [
        "default-src 'none'",
        "connect-src 'none'",
        "frame-src 'none'",
        "form-action 'none'",
        "base-uri 'none'",
        "object-src 'none'",
        "worker-src 'none'",
    ] {
        assert!(CSP.contains(rule), "{rule}");
    }
    assert!(!CSP.contains("http:") && !CSP.contains("file:") && !CSP.contains('*'));
    assert!(!CSP.contains('"'), "it sits in a double-quoted attribute");
}

#[test]
fn the_theme_sets_the_scheme_and_variables_and_cannot_break_out() {
    let out = prepare("<p>", &theme(true));
    assert!(out.contains(":root{color-scheme:dark;--background:#101010;"));
    assert!(prepare("<p>", &theme(false)).contains("color-scheme:light;"));
    // The font value lost the characters that could end the rule or the style tag.
    assert!(out.contains("--font-sans:\"Inter\"/stylescriptx()/script;"));
    assert_eq!(out.matches("</style>").count(), 1);
    assert!(out.contains("var(--background)"), "the base sheet");
}
