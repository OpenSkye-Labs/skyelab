//! The agent path into the browser. Agents may only reach http and https pages, and may not run
//! scripts in a `file://` page: `eval_js` there could read any local file, including other accounts'
//! credentials (docs/architecture.md, "Decisions"). The user's URL bar keeps `file://`.

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
    if !matches!(scheme.to_ascii_lowercase().as_str(), "http" | "https") {
        bail!("agents may only run scripts in http or https pages, not {current_url:?}");
    }
    Ok(())
}

#[cfg(test)]
mod tests;
