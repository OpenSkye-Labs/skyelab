//! Which URLs the preview may load.

use anyhow::{Context as _, bail};

/// Parses user or agent input into a URL the preview may load: http, https or file only.
/// A bare host (`example.com`, `localhost:5173`) gets `https://`. Everything else, including
/// `javascript:` and `data:`, is rejected.
pub fn parse_url(input: &str) -> anyhow::Result<url::Url> {
    let input = input.trim();
    if input.is_empty() {
        bail!("enter a URL");
    }
    // No scheme, or `host:port`: the text after the first `:` is a port, not a scheme body.
    let bare = !input.contains("://")
        && input
            .split_once(':')
            .is_none_or(|(_, rest)| rest.starts_with(|c: char| c.is_ascii_digit()));
    let url = if bare {
        url::Url::parse(&format!("https://{input}"))
    } else {
        url::Url::parse(input)
    }
    .with_context(|| format!("not a valid URL: {input}"))?;
    match url.scheme() {
        "http" | "https" | "file" => Ok(url),
        other => bail!("{other}: URLs are not allowed, use http, https or file"),
    }
}

#[cfg(test)]
mod tests;
