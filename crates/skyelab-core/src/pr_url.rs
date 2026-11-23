//! Which pasted links "Link pull request…" stores on a thread.

/// A GitHub pull request link as `https://github.com/<owner>/<repo>/pull/<n>`, or `None` for anything else. A
/// trailing tab (`/files`), query or fragment is dropped; no credentials, port, other host or whitespace is accepted.
pub fn github_pr_url(text: &str) -> Option<String> {
    let text = text.trim();
    if text.chars().any(char::is_whitespace) {
        return None;
    }
    let (scheme, rest) = text.split_once("://")?;
    let (host, path) = rest.split_once('/')?;
    if !scheme.eq_ignore_ascii_case("https")
        || !(host.eq_ignore_ascii_case("github.com") || host.eq_ignore_ascii_case("www.github.com"))
    {
        return None;
    }
    let mut parts = path.split(['?', '#']).next()?.split('/');
    let (owner, repo, pull, number) = (parts.next()?, parts.next()?, parts.next()?, parts.next()?);
    let name = |s: &str| {
        !s.is_empty()
            && !s.starts_with('.')
            && s.bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
    };
    let number: u32 = number
        .bytes()
        .all(|b| b.is_ascii_digit())
        .then(|| number.parse().ok())??;
    (name(owner) && name(repo) && pull == "pull" && number > 0)
        .then(|| format!("https://github.com/{owner}/{repo}/pull/{number}"))
}
