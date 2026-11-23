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

/// Names whose presence marks a provider's config dir. Never credential files: only their absence matters here.
fn markers(provider: Provider) -> &'static [&'static str] {
    match provider {
        Provider::Claude => &["settings.json", ".claude.json", "projects"],
        Provider::Codex => &["config.toml", "sessions", "history.jsonl"],
    }
}

/// `~/.claude`, `~/.claude-*`, `~/.codex` and `~/.codex-*` dirs holding a marker, minus dirs `config` already has.
/// Sorted by provider, then the plain dir first.
pub fn find_accounts(home: &Path, config: &AccountsConfig) -> Vec<FoundAccount> {
    let Ok(entries) = std::fs::read_dir(home) else {
        return Vec::new();
    };
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
            let known = path.is_dir()
                && markers(provider).iter().any(|m| path.join(m).exists())
                && !config.accounts.iter().any(|a| a.config_dir == path);
            known.then_some((provider, name, path))
        })
        .collect();
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

/// Where people keep their checkouts; scanned one level deep.
const REPO_ROOTS: [&str; 4] = ["code", "projects", "dev", "src"];
const MAX_REPOS: usize = 8;

/// Git repos directly under `~/code`, `~/projects`, `~/dev` or `~/src`, most recently changed first, at most 8.
pub fn find_repos(home: &Path) -> Vec<PathBuf> {
    let mut repos: Vec<(std::time::SystemTime, PathBuf)> = REPO_ROOTS
        .iter()
        .filter_map(|r| std::fs::read_dir(home.join(r)).ok())
        .flat_map(|d| d.flatten())
        .map(|e| e.path())
        .filter_map(|p| {
            let git = std::fs::metadata(p.join(".git")).ok()?;
            Some((git.modified().unwrap_or(std::time::UNIX_EPOCH), p))
        })
        .collect();
    repos.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
    repos.into_iter().take(MAX_REPOS).map(|(_, p)| p).collect()
}

/// Binds `dir` to `account` for a new project, unless it already resolves to that account. A folder bound to
/// another account stays an error from `add_binding`: the user picks again, nothing is overwritten.
pub fn bind_project(config: &mut AccountsConfig, dir: &Path, account: &str) -> Result<()> {
    if config.resolve(dir).is_some_and(|a| a.id == account) {
        return Ok(());
    }
    config.add_binding(Binding::new(dir.to_path_buf(), account.to_string()))
}
