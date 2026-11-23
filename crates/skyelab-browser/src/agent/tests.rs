use super::{check_agent_eval, parse_agent_url};

#[test]
fn agent_urls_are_http_or_https_only() {
    for u in [
        "file:///etc/hosts",
        "FILE:///etc/hosts",
        "/etc/hosts",
        "file://localhost/tmp/x",
        "javascript:alert(1)",
        "about:blank",
    ] {
        assert!(parse_agent_url(u).is_err(), "{u:?} must be refused");
    }
    for (u, want) in [
        ("https://example.com/", "https://example.com/"),
        ("http://localhost:3000/a", "http://localhost:3000/a"),
        ("localhost:5173", "http://localhost:5173/"),
        ("example.com", "https://example.com/"),
    ] {
        assert_eq!(parse_agent_url(u).expect(u).as_str(), want);
    }
}

#[test]
fn agent_eval_refuses_file_pages() {
    for u in ["file:///Users/me/index.html", "FILE:///x", " file:///x"] {
        assert!(check_agent_eval(u, true).is_err(), "{u:?} must be refused");
    }
    assert!(check_agent_eval("https://example.com/", true).is_ok());
    assert!(check_agent_eval("http://localhost:3000/", true).is_ok());
}

#[test]
fn agent_eval_refuses_before_the_first_load() {
    let err = check_agent_eval("https://example.com/", false).expect_err("not loaded");
    assert!(format!("{err:#}").contains("not loaded"), "{err:#}");
}

#[test]
fn agent_refusal_never_names_the_local_path() {
    let err = check_agent_eval("file:///Users/me/.ssh/config", true).expect_err("file page");
    let err = format!("{err:#}");
    assert!(!err.contains("/Users/me"), "{err}");
    assert!(err.contains("file:"), "{err}");
}
