use super::{PageTheme, blocked_hosts, csp, prepare};

fn hosts(list: &[&str]) -> Vec<String> {
    list.iter().map(|h| (*h).to_owned()).collect()
}

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
            let out = prepare(&page, &theme(false), &[]);
            assert!(
                out.starts_with(&format!("<!doctype html>{META}")),
                "{lead:?}: {out}"
            );
            assert!(out.ends_with(&page));
        }
    }
}

#[test]
fn the_policy_blocks_requests_and_frames_and_allows_only_listed_hosts() {
    let defaults = csp(&hosts(&skyelab_core::page_hosts::DEFAULT_PAGE_HOSTS));
    for rule in [
        "default-src 'none'",
        "connect-src 'none'",
        "frame-src 'none'",
        "form-action 'none'",
        "base-uri 'none'",
        "object-src 'none'",
        "worker-src 'none'",
        "img-src data: blob:;",
        "media-src data: blob:;",
        "script-src 'unsafe-inline' 'unsafe-eval' https://cdnjs.cloudflare.com https://cdn.jsdelivr.net https://unpkg.com;",
        "style-src 'unsafe-inline' https://cdnjs.cloudflare.com https://cdn.jsdelivr.net https://unpkg.com;",
        "font-src data: https://cdnjs.cloudflare.com https://cdn.jsdelivr.net https://unpkg.com;",
    ] {
        assert!(defaults.contains(rule), "{rule}: {defaults}");
    }
    // Never any https host: `https:` alone appears nowhere.
    assert!(!defaults.contains("https:;") && !defaults.contains("https: "));
    assert!(!defaults.contains("http:") && !defaults.contains("file:") && !defaults.contains('*'));
    assert!(
        !defaults.contains('"'),
        "it sits in a double-quoted attribute"
    );
}

#[test]
fn the_policy_follows_a_custom_list_and_drops_what_the_validator_refuses() {
    let policy = csp(&hosts(&[
        "*.example.com",
        "FONTS.gstatic.com",
        "localhost",
        "*",
        "evil.com; script-src *",
        "\"x.com\"",
        "10.0.0.1",
    ]));
    assert!(policy.contains(
        "script-src 'unsafe-inline' 'unsafe-eval' https://*.example.com https://fonts.gstatic.com;"
    ));
    assert!(policy.contains("font-src data: https://*.example.com https://fonts.gstatic.com;"));
    assert!(
        policy.contains("img-src data: blob:;"),
        "images never follow the list"
    );
    for bad in ["localhost", "evil", "x.com", "10.0.0.1", "* "] {
        assert!(!policy.contains(bad), "{bad}: {policy}");
    }
    let empty = csp(&[]);
    assert!(empty.contains("script-src 'unsafe-inline' 'unsafe-eval';"));
    assert!(empty.contains("font-src data:;"));
    assert!(
        prepare("<p>", &theme(false), &hosts(&["cdn.example.com"]))
            .contains("https://cdn.example.com")
    );
}

#[test]
fn blocked_hosts_keeps_only_valid_new_https_hosts() {
    let allowed = hosts(&["unpkg.com"]);
    let reported = "https://cdn.evil.example/a.js https://unpkg.com/x https://cdn.evil.example/b.css \
        http://plain.example/a.js https://127.0.0.1/a.js https://localhost/a.js https://cdn.example:8443/a.js \
        inline eval data https://user@fonts.example/f.woff2 not-a-url \"<b>x</b>\" https://*.example.com/";
    assert_eq!(
        blocked_hosts(reported, &allowed),
        ["cdn.evil.example", "fonts.example"]
    );
    let many: String = (0..12).map(|i| format!("https://h{i}.example/ ")).collect();
    assert_eq!(blocked_hosts(&many, &[]).len(), 5);
}

#[test]
fn the_theme_sets_the_scheme_and_variables_and_cannot_break_out() {
    let out = prepare("<p>", &theme(true), &[]);
    assert!(out.contains(":root{color-scheme:dark;--background:#101010;"));
    assert!(prepare("<p>", &theme(false), &[]).contains("color-scheme:light;"));
    // The font value lost the characters that could end the rule or the style tag.
    assert!(out.contains("--font-sans:\"Inter\"/stylescriptx()/script;"));
    assert_eq!(out.matches("</style>").count(), 1);
    assert!(out.contains("var(--background)"), "the base sheet");
}
