use super::parse_url;

#[test]
fn accepts_http_https_file() {
    for u in [
        "http://localhost:3000/a?b=1",
        "https://example.com/",
        "file:///tmp/index.html",
    ] {
        assert_eq!(
            parse_url(u).map(|u| u.to_string()).ok().as_deref(),
            Some(u),
            "{u}"
        );
    }
}

#[test]
fn bare_host_gets_https() {
    assert_eq!(
        parse_url("  example.com  ").expect("bare host").as_str(),
        "https://example.com/"
    );
    assert_eq!(
        parse_url("example.com:8443").expect("host:port").as_str(),
        "https://example.com:8443/"
    );
}

#[test]
fn rejects_other_schemes() {
    for u in [
        "javascript:alert(1)",
        "JavaScript:alert(1)",
        " javascript:alert(1)",
        "data:text/html,<b>x</b>",
        "ftp://example.com",
        "about:blank",
        "vscode://open",
        "",
        "   ",
    ] {
        assert!(parse_url(u).is_err(), "{u:?} should be rejected");
    }
}

#[test]
fn bare_host_with_colon_or_url_in_query() {
    assert_eq!(
        parse_url("example.com/search?q=a:b")
            .expect("colon in query")
            .as_str(),
        "https://example.com/search?q=a:b"
    );
    let url = parse_url("localhost:5173/?next=http://x").expect("url in query");
    assert_eq!(url.host_str(), Some("localhost"));
}

#[test]
fn loopback_defaults_to_http() {
    for (input, want) in [
        ("localhost:5173", "http://localhost:5173/"),
        ("127.0.0.1:3000/app", "http://127.0.0.1:3000/app"),
        ("[::1]:3000", "http://[::1]:3000/"),
    ] {
        assert_eq!(parse_url(input).expect(input).as_str(), want);
    }
    // An explicit scheme is kept.
    assert_eq!(
        parse_url("https://localhost:5173")
            .expect("explicit")
            .as_str(),
        "https://localhost:5173/"
    );
}

#[test]
fn absolute_path_is_a_file_url() {
    assert_eq!(
        parse_url("/tmp/index.html").expect("abs path").as_str(),
        "file:///tmp/index.html"
    );
}

#[test]
fn rejects_file_urls_with_a_remote_host() {
    // On Windows this is a UNC path: opening it leaks the user's NTLM hash.
    assert!(parse_url("file://attacker.example/share/x").is_err());
    assert!(parse_url("file://localhost/tmp/x").is_ok());
}
