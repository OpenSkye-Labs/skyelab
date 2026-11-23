//! The agent path into the browser: http and https only (docs/decisions.md).

use anyhow::bail;

use crate::parse_url;

/// [`parse_url`] minus local files: http and https only.
pub fn parse_agent_url(input: &str) -> anyhow::Result<url::Url> {
    let url = parse_url(input)?;
    if !matches!(url.scheme(), "http" | "https") {
        bail!(
            "agents may only open http or https URLs, not {}:",
            url.scheme()
        );
    }
    Ok(url)
}

/// Whether an agent may run a script in the page now on `current_url`. Before the first load wry
/// queues the script and runs it in whatever page commits next, which may be a file the user opens.
pub(crate) fn check_agent_eval(current_url: &str, loaded: bool) -> anyhow::Result<()> {
    if !loaded {
        bail!("page not loaded yet: navigate to an http or https page first");
    }
    let scheme = current_url
        .trim_start()
        .split(':')
        .next()
        .unwrap_or_default();
    // The scheme only: the URL of a local file page is a path the agent must not learn.
    let scheme = scheme.to_ascii_lowercase();
    if !matches!(scheme.as_str(), "http" | "https") {
        bail!("agents may only run scripts in http or https pages, not a {scheme}: page");
    }
    Ok(())
}

#[cfg(test)]
mod tests;
