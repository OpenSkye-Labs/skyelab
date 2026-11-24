use anyhow::{Context as _, anyhow, bail};

/// Parses the user's URL bar input into a URL the preview may load: http, https or local file only.
/// Agents go through [`crate::parse_agent_url`] instead.
/// A bare host gets `https://` (`example.com`), or `http://` for loopback (`localhost:5173`).
/// An absolute path becomes a `file://` URL. Everything else, including `javascript:`, `data:`
/// and `file://` on a remote host, is rejected.
pub fn parse_url(input: &str) -> anyhow::Result<url::Url> {
    let input = input.trim();
    if input.is_empty() {
        bail!("enter a URL");
    }
    // `/x` on unix, `C:\x` on Windows (where a rooted `/x` has no drive and fails). A UNC path gets a host.
    if input.starts_with('/') || std::path::Path::new(input).is_absolute() {
        let url =
            url::Url::from_file_path(input).map_err(|()| anyhow!("not a file path: {input}"))?;
        if let Some(host) = url.host_str().filter(|h| !h.is_empty()) {
            bail!("file URLs must be local, not on {host}");
        }
        return Ok(url);
    }
    let bare = is_bare_host(input);
    let url = if bare {
        url::Url::parse(&format!("https://{input}"))
    } else {
        url::Url::parse(input)
    };
    let mut url = url.with_context(|| format!("not a valid URL: {input}"))?;
    if bare && matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "[::1]")) {
        // Local dev servers serve plain http.
        url.set_scheme("http")
            .map_err(|()| anyhow!("can't use http for {input}"))?;
    }
    match url.scheme() {
        "http" | "https" => Ok(url),
        // A remote host is a UNC path on Windows; opening it leaks the user's NTLM hash.
        "file" if matches!(url.host_str(), None | Some("" | "localhost")) => Ok(url),
        "file" => bail!(
            "file URLs must be local, not on {}",
            url.host_str().unwrap_or_default()
        ),
        other => bail!("{other}: URLs are not allowed, use http, https or file"),
    }
}

/// `host`, `host:port` or `[v6]:port`, judged on the text before any path, query or fragment.
fn is_bare_host(input: &str) -> bool {
    let head = input.split(['/', '?', '#']).next().unwrap_or_default();
    head.starts_with('[')
        || head
            .split_once(':')
            .is_none_or(|(_, port)| !port.is_empty() && port.bytes().all(|b| b.is_ascii_digit()))
}

#[cfg(test)]
mod tests;
