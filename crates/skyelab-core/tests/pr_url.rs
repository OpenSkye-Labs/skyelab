//! Acceptance: "Link pull request…" stores only a GitHub pull request URL, in one canonical form.

use skyelab_core::github_pr_url;

#[test]
fn github_pull_request_urls_are_accepted_and_normalized() {
    let canon = Some("https://github.com/o-1/r.s_x/pull/42".to_owned());
    for ok in [
        "https://github.com/o-1/r.s_x/pull/42",
        "  https://github.com/o-1/r.s_x/pull/42/  ",
        "https://github.com/o-1/r.s_x/pull/42/files",
        "https://github.com/o-1/r.s_x/pull/42#issuecomment-1",
        "https://github.com/o-1/r.s_x/pull/42?w=1",
        "https://www.github.com/o-1/r.s_x/pull/42",
        "HTTPS://GitHub.com/o-1/r.s_x/pull/42",
    ] {
        assert_eq!(github_pr_url(ok), canon, "{ok}");
    }
}

#[test]
fn anything_else_is_refused() {
    for bad in [
        "",
        "github.com/o/r/pull/1",
        "http://github.com/o/r/pull/1",
        "https://github.com/o/r/issues/1",
        "https://github.com/o/r/pull/",
        "https://github.com/o/r/pull/1x",
        "https://github.com/o/r/pull/0",
        "https://github.com/o/pull/1",
        "https://gitlab.com/o/r/pull/1",
        "https://github.com.evil.example/o/r/pull/1",
        "https://user@github.com/o/r/pull/1",
        "https://github.com:8443/o/r/pull/1",
        "https://github.com/o/r/pull/1 extra",
        "https://github.com/o w/r/pull/1",
        "https://github.com/../r/pull/1",
        "https://github.com/o/r/pull/99999999999999999999999",
        "javascript:alert(1)//https://github.com/o/r/pull/1",
    ] {
        assert_eq!(github_pr_url(bad), None, "{bad}");
    }
}
