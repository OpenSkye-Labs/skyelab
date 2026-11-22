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
        parse_url("localhost:5173").expect("host:port").as_str(),
        "https://localhost:5173/"
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
