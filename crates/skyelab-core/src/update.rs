//! The update check's rules: is a release newer than this build, may its page be opened, is a check due.
//! The fetch lives in `skyelab-client`; the notice in the app. Nothing here touches the network.

use serde::{Deserialize, Serialize};

/// The public endpoint the app asks; no auth, nothing sent but the GET.
pub const LATEST_RELEASE_URL: &str =
    "https://api.github.com/repos/OpenSkye-Labs/skyelab/releases/latest";
/// Only a page under this prefix is opened by Download.
pub const RELEASES_PAGE: &str = "https://github.com/OpenSkye-Labs/skyelab/releases/";
/// The start check runs at most this often (seconds of wall-clock time).
pub const CHECK_EVERY_SECS: u64 = 24 * 60 * 60;

/// Settings → About: the update check (`[updates]` in settings.toml).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct UpdateSettings {
    /// Check GitHub for a newer release on start, at most daily.
    pub check: bool,
    /// The last check, Unix seconds.
    pub last_check: Option<u64>,
    /// A version the user chose to skip: the automatic check doesn't show it again.
    pub skipped: Option<String>,
}

impl Default for UpdateSettings {
    fn default() -> Self {
        Self {
            check: true,
            last_check: None,
            skipped: None,
        }
    }
}

/// `v1.2.3`, `1.2`, `1.2.3-beta.1` → numeric parts; a pre-release or build suffix is ignored. `None` if not a version.
fn parts(tag: &str) -> Option<Vec<u64>> {
    let tag = tag.trim();
    let tag = tag.strip_prefix(['v', 'V']).unwrap_or(tag);
    let core = tag.split(['-', '+']).next()?;
    let parts: Option<Vec<u64>> = core.split('.').map(|p| p.parse().ok()).collect();
    parts.filter(|p| !p.is_empty() && p.len() <= 4)
}

/// Whether release `latest` is newer than `current`, comparing numerically part by part (a missing part is 0).
/// Anything that isn't a version is never newer.
pub fn is_newer(latest: &str, current: &str) -> bool {
    let (Some(mut l), Some(mut c)) = (parts(latest), parts(current)) else {
        return false;
    };
    let n = l.len().max(c.len());
    l.resize(n, 0);
    c.resize(n, 0);
    l > c
}

/// The version as shown: the tag without its `v`.
pub fn display_version(tag: &str) -> &str {
    let tag = tag.trim();
    tag.strip_prefix(['v', 'V']).unwrap_or(tag)
}

/// Whether Download may open `url`: this repo's releases page, nothing else (the API answer is untrusted).
pub fn release_page_ok(url: &str) -> bool {
    url.strip_prefix(RELEASES_PAGE).is_some_and(|rest| {
        !rest.is_empty()
            && rest
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || "-._/".contains(c))
            && !rest.contains("..")
    })
}

/// Whether the automatic check is due at `now` (Unix seconds): never checked, a day has passed, or the clock went back.
pub fn due(last_check: Option<u64>, now: u64) -> bool {
    match last_check {
        None => true,
        Some(last) => now < last || now - last >= CHECK_EVERY_SECS,
    }
}

/// Where to ask for an agent CLI's newest release: the npm registry's `latest` record of its package (T3 asks the
/// package manager that installed it). shortcut: a native-installer channel can trail npm by a few days, so the
/// notice may come early there; ask the installer when that matters.
pub fn agent_latest_url(provider: crate::Provider) -> Option<&'static str> {
    match provider {
        crate::Provider::Claude => {
            Some("https://registry.npmjs.org/@anthropic-ai/claude-code/latest")
        }
        crate::Provider::Codex => Some("https://registry.npmjs.org/@openai/codex/latest"),
        crate::Provider::Cursor => None,
    }
}

/// The version in a `--version` line: its first word that reads as one (`2.1.3 (Claude Code)`, `codex-cli 0.46.0`).
pub fn version_in(line: &str) -> Option<&str> {
    line.split_whitespace()
        .map(|w| w.trim_matches(|c: char| !c.is_ascii_alphanumeric()))
        .find(|w| w.contains('.') && parts(w).is_some())
}

/// A release's downloads (`assets[]` of GitHub's release JSON); name and URL untrusted until checked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Asset {
    pub name: String,
    pub url: String,
}

/// The checksums file the release workflow attaches (`sha256sum` over every other asset).
pub const CHECKSUMS: &str = "SHA256SUMS";

/// Whether an asset URL may be downloaded: this repo's `releases/download/<tag>/<name>`, nothing else.
pub fn asset_url_ok(url: &str) -> bool {
    url.strip_prefix(RELEASES_PAGE)
        .and_then(|r| r.strip_prefix("download/"))
        .is_some_and(|rest| {
            rest.split('/').count() == 2
                && !rest.contains("..")
                && rest
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || "-._/+".contains(c))
        })
}

/// `name`'s lowercase SHA-256 in a `sha256sum` listing (`<hex>  <name>`, or `<hex> *<name>` in binary mode).
pub fn expected_sha256(sums: &str, name: &str) -> Option<String> {
    sums.lines().find_map(|line| {
        let (hex, rest) = line.trim_end().split_once(' ')?;
        let file = rest.trim_start_matches(' ').trim_start_matches('*');
        (file == name && hex.len() == 64 && hex.chars().all(|c| c.is_ascii_hexdigit()))
            .then(|| hex.to_ascii_lowercase())
    })
}
