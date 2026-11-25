//! First-run setup: which agent config dirs and git repos a home folder already has. Read-only: it checks that
//! names exist and never opens a file, so no credential or token is ever read.

use crate::{AccountsConfig, Binding, Provider};
use anyhow::Result;
use std::path::{Path, PathBuf};

/// A config dir under home that looks like an agent login, offered as an account.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FoundAccount {
    pub provider: Provider,
    pub config_dir: PathBuf,
    /// Unique among the config's accounts and the other found ones.
    pub id: String,
    pub label: String,
}

/// Where a new account keeps its login when the user names no folder: `~/.claude-<id>` or `~/.codex-<id>`, the
/// pattern [`find_accounts`] offers back. `None` unless the id is a plain name that can't climb out of home.
pub fn default_config_dir(home: &Path, provider: Provider, id: &str) -> Option<PathBuf> {
    plain(id).then(|| home.join(format!(".{}-{id}", provider.label().to_lowercase())))
}

/// Where an account keeps its own `gh` login when the user names no folder: `~/.config/gh-<id>`; `None` as above.
pub fn default_gh_dir(home: &Path, id: &str) -> Option<PathBuf> {
    plain(id).then(|| home.join(".config").join(format!("gh-{id}")))
}

/// Letters, digits, `-`, `_` and `.`, not starting with `.`: one path component, never `..`.
fn plain(id: &str) -> bool {
    !id.is_empty()
        && !id.starts_with('.')
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
}

/// Names whose presence marks a provider's config dir. Never credential files: only their absence matters here.
fn markers(provider: Provider) -> &'static [&'static str] {
    match provider {
        Provider::Claude => &["settings.json", ".claude.json", "projects"],
        Provider::Codex => &["config.toml", "sessions", "history.jsonl"],
        Provider::Cursor => &["cli-config.json", "chats", "mcp.json"],
    }
}

/// `~/.claude`, `~/.claude-*`, `~/.codex` and `~/.codex-*` dirs holding a marker, minus dirs `config` already has.
/// Sorted by provider, then the plain dir first.
pub fn find_accounts(home: &Path, config: &AccountsConfig) -> Vec<FoundAccount> {
    let Ok(entries) = std::fs::read_dir(home) else {
        return Vec::new();
    };
    // Compared by the real folder, so a link to an added (or another found) dir is not a second account.
    let real = |p: &Path| std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    let mut seen: Vec<PathBuf> = config
        .accounts
        .iter()
        .map(|a| real(&a.config_dir))
        .collect();
    let mut dirs: Vec<(Provider, String, PathBuf)> = entries
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().into_string().ok()?;
            let provider = Provider::ALL.into_iter().find(|p| {
                let base = format!(".{}", p.label().to_lowercase());
                name == base
                    || name
                        .strip_prefix(&base)
                        .is_some_and(|s| s.len() > 1 && s.starts_with('-'))
            })?;
            let path = e.path();
            let marked = path.is_dir() && markers(provider).iter().any(|m| path.join(m).exists());
            marked.then_some((provider, name, path))
        })
        .collect();
    // Plain names first, so the link is the one dropped rather than the folder it points at.
    dirs.sort_by_key(|(_, _, p)| {
        std::fs::symlink_metadata(p).is_ok_and(|m| m.file_type().is_symlink())
    });
    dirs.retain(|(_, _, p)| {
        let r = real(p);
        let new = !seen.contains(&r);
        seen.push(r);
        new
    });
    dirs.sort_by(|a, b| (a.0 as u8, &a.1).cmp(&(b.0 as u8, &b.1)));
    let mut taken: Vec<String> = config.accounts.iter().map(|a| a.id.clone()).collect();
    dirs.into_iter()
        .map(|(provider, name, config_dir)| {
            let base = name.trim_start_matches('.').to_string();
            let id = (1..)
                .map(|n| {
                    if n == 1 {
                        base.clone()
                    } else {
                        format!("{base}-{n}")
                    }
                })
                .find(|id| !taken.contains(id))
                .expect("an unbounded range always has a free id");
            taken.push(id.clone());
            let label = match base.split_once('-') {
                Some((_, suffix)) => format!("{} ({suffix})", provider.label()),
                None => provider.label().to_string(),
            };
            FoundAccount {
                provider,
                config_dir,
                id,
                label,
            }
        })
        .collect()
}

/// Binds `dir` to `account` for a new project, unless it already resolves to that account. A folder bound to
/// another account stays an error from `add_binding`: the user picks again, nothing is overwritten.
pub fn bind_project(config: &mut AccountsConfig, dir: &Path, account: &str) -> Result<()> {
    if config.resolve(dir).is_some_and(|a| a.id == account) {
        return Ok(());
    }
    config.add_binding(Binding::new(dir.to_path_buf(), account.to_string()))
}

/// Whether `defaults read com.apple.universalaccess reduceMotion` printed that Reduce Motion is on.
pub fn reduce_motion_on(defaults_output: &str) -> bool {
    defaults_output.trim() == "1"
}
