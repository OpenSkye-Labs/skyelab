//! The one rule for what a child process may inherit from SkyeLab's env: agents, project terminals and every
//! git/gh call SkyeLab runs. Everything not listed (credentials, providers, base URLs, the parent's
//! CLAUDE_CONFIG_DIR/CODEX_HOME/CURSOR_CONFIG_DIR/GH_* and GIT_*) is dropped; callers then add the bound account's env.

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

/// Parent env that passes through on unix. `LC_*` and `XDG_*` pass too.
const UNIX_ENV: &[&str] = &[
    "PATH", "HOME", "USER", "LOGNAME", "SHELL", "TMPDIR", "LANG", "TERM",
];
/// Network plumbing (and TZ) from the user's own env, on every OS. None of these choose the account or provider; without
/// them agents fail behind proxies.
const SHARED_ENV: &[&str] = &[
    "HTTP_PROXY",
    "HTTPS_PROXY",
    "NO_PROXY",
    "ALL_PROXY",
    "NODE_EXTRA_CA_CERTS",
    "SSL_CERT_FILE",
    "SSL_CERT_DIR",
    "TZ",
];
const WINDOWS_ENV: &[&str] = &[
    "SystemDrive",
    "WINDIR",
    "ProgramFiles",
    "ProgramFiles(x86)",
    "ProgramData",
    "HOMEDRIVE",
    "HOMEPATH",
    "CLAUDE_CODE_GIT_BASH_PATH",
    "PATH",
    "SystemRoot",
    "USERPROFILE",
    "APPDATA",
    "LOCALAPPDATA",
    "PATHEXT",
    "COMSPEC",
    "TEMP",
    "TMP",
];
/// The ssh-agent socket. An ssh key isn't bound to any account, so agents get it only when the account opts in
/// (`Account::ssh_agent`); terminals, the user's own shell, always do.
const SSH_AGENT: &str = "SSH_AUTH_SOCK";
/// What a shell needs beyond the agent list: editor, pager, colours, display, tool homes. None names an account,
/// a provider or a login.
const TERMINAL_ENV: &[&str] = &[
    "EDITOR",
    "VISUAL",
    "PAGER",
    "MANPAGER",
    "MANPATH",
    "INFOPATH",
    "LESS",
    "LESSOPEN",
    "COLORTERM",
    "CLICOLOR",
    "LSCOLORS",
    "LS_COLORS",
    "TERM_PROGRAM",
    "TERM_PROGRAM_VERSION",
    "ZDOTDIR",
    "HISTFILE",
    "DISPLAY",
    "WAYLAND_DISPLAY",
    "DBUS_SESSION_BUS_ADDRESS",
    "__CF_USER_TEXT_ENCODING",
    "HOMEBREW_PREFIX",
    "HOMEBREW_CELLAR",
    "HOMEBREW_REPOSITORY",
    "CARGO_HOME",
    "RUSTUP_HOME",
    "GOPATH",
    "GOROOT",
    "NVM_DIR",
    "PYENV_ROOT",
    "JAVA_HOME",
    "ANDROID_HOME",
    "ANDROID_SDK_ROOT",
    "USERNAME",
    "COMPUTERNAME",
    "OS",
    "PROCESSOR_ARCHITECTURE",
    "NUMBER_OF_PROCESSORS",
    "PSModulePath",
    "ProgramW6432",
    "CommonProgramFiles",
    "PUBLIC",
    "ALLUSERSPROFILE",
    "CommonProgramFiles(x86)",
    "CommonProgramW6432",
    "PROCESSOR_IDENTIFIER",
    "PROCESSOR_LEVEL",
    "PROCESSOR_REVISION",
    "USERDOMAIN",
    "USERDOMAIN_ROAMINGPROFILE",
    "LOGONSERVER",
    "SESSIONNAME",
    "OneDrive",
    "DriverData",
    "PROMPT",
];

/// Whether the parent's `name` may reach an agent (and SkyeLab's own git/gh). Windows names are case-insensitive.
pub fn agent_allowed(name: &OsStr, windows: bool, ssh: bool) -> bool {
    let Some(name) = name.to_str() else {
        return false;
    };
    if ssh && is(name, SSH_AGENT, windows) {
        return true;
    }
    if windows {
        WINDOWS_ENV
            .iter()
            .chain(SHARED_ENV)
            .any(|a| a.eq_ignore_ascii_case(name))
    } else {
        UNIX_ENV.contains(&name)
            || name.starts_with("LC_")
            || name.starts_with("XDG_")
            // unix tools read both spellings of the proxy vars
            || SHARED_ENV.iter().any(|a| *a == name || a.to_ascii_lowercase() == name)
    }
}

/// Whether the parent's `name` may reach a project terminal: the agent list, ssh-agent, and [`TERMINAL_ENV`].
pub fn terminal_allowed(name: &OsStr, windows: bool) -> bool {
    agent_allowed(name, windows, true)
        || name
            .to_str()
            .is_some_and(|n| TERMINAL_ENV.iter().any(|a| is(n, a, windows)))
}

fn is(name: &str, allowed: &str, windows: bool) -> bool {
    if windows {
        name.eq_ignore_ascii_case(allowed)
    } else {
        name == allowed
    }
}

/// The part of `parent` an agent (or SkyeLab's git) inherits. PATH keeps only absolute entries: a relative one
/// would resolve inside the untrusted project.
pub fn agent_vars(
    parent: impl IntoIterator<Item = (OsString, OsString)>,
    windows: bool,
    ssh: bool,
) -> Vec<(OsString, OsString)> {
    parent
        .into_iter()
        .filter(|(k, _)| agent_allowed(k, windows, ssh))
        .filter_map(|(k, v)| {
            if !k.eq_ignore_ascii_case("PATH") {
                return Some((k, v));
            }
            // An empty PATH would still let a shell look in the cwd: no PATH at all instead.
            std::env::join_paths(absolute_entries(&v))
                .ok()
                .filter(|p| !p.is_empty())
                .map(|p| (k, p))
        })
        .collect()
}

/// The part of `parent` a project terminal inherits. PATH is the user's own, relative entries included.
pub fn terminal_vars(
    parent: impl IntoIterator<Item = (OsString, OsString)>,
    windows: bool,
) -> Vec<(OsString, OsString)> {
    parent
        .into_iter()
        .filter(|(k, _)| terminal_allowed(k, windows))
        .collect()
}

/// [`agent_vars`] of this process's env.
pub fn inherited_agent_env(ssh: bool) -> Vec<(OsString, OsString)> {
    agent_vars(std::env::vars_os(), cfg!(windows), ssh)
}

pub fn absolute_entries(path: &OsStr) -> Vec<PathBuf> {
    std::env::split_paths(path)
        .filter(|p| p.is_absolute())
        .collect()
}

/// The `GIT_CONFIG_COUNT`/`KEY_n`/`VALUE_n` env that binds git's https credentials to an account, the way
/// SkyeLab's own Push does: every helper from the user's and system config is dropped (the keychain's login among
/// them), then `gh auth git-credential` answers under the account's `GH_CONFIG_DIR` (no helper at all when the
/// account has no gh dir, so nothing answers). `http.extraHeader` is cleared, generically and for each URL in
/// `scoped_headers` (see [`scoped_extra_headers`]). Without `ssh`, git's ssh transport fails with a message, so an
/// ssh remote never pushes with a key no account binds.
pub fn git_credential_env(
    has_gh_dir: bool,
    scoped_headers: &[String],
    ssh: bool,
) -> Vec<(String, String)> {
    let mut pairs: Vec<(String, String)> = vec![("credential.helper".into(), String::new())];
    if has_gh_dir {
        pairs.push(("credential.helper".into(), "!gh auth git-credential".into()));
    }
    pairs.push(("http.extraHeader".into(), String::new()));
    // The user's askpass would answer when no helper does.
    pairs.push(("core.askPass".into(), String::new()));
    pairs.extend(
        scoped_headers
            .iter()
            .map(|url| (format!("http.{url}.extraHeader"), String::new())),
    );
    if !ssh {
        pairs.push((
            "core.sshCommand".into(),
            "echo 'SkyeLab: ssh is off for this account (set ssh_agent = true on it to allow)' >&2; exit 1; :"
                .into(),
        ));
    }
    let mut env = vec![("GIT_CONFIG_COUNT".to_string(), pairs.len().to_string())];
    for (i, (k, v)) in pairs.into_iter().enumerate() {
        env.push((format!("GIT_CONFIG_KEY_{i}"), k));
        env.push((format!("GIT_CONFIG_VALUE_{i}"), v));
    }
    env
}

/// What git run for `account` in `dir` gets, by an agent or a repo's hooks: [`git_credential_env`] for the account's
/// gh login and ssh setting, and no prompts.
pub fn account_git_env(account: &crate::Account, dir: &Path) -> Vec<(String, String)> {
    let mut env = git_credential_env(
        account.gh_config_dir.is_some(),
        &scoped_extra_headers(dir),
        account.ssh_agent,
    );
    env.push(("GIT_TERMINAL_PROMPT".into(), "0".into()));
    env
}

/// The URLs of every `http.<url>.extraHeader` the git config seen from `dir` sets (system, global, the repo's),
/// so [`git_credential_env`] can reset each by name. Best effort: no git, or no such keys, is an empty list.
pub fn scoped_extra_headers(dir: &Path) -> Vec<String> {
    scoped_extra_headers_with(Path::new("git"), dir)
}

/// [`scoped_extra_headers`] with another `git`, so tests can run a fake.
pub fn scoped_extra_headers_with(git: &Path, dir: &Path) -> Vec<String> {
    let mut cmd = std::process::Command::new(git);
    cmd.arg("-C")
        .arg(dir)
        .args([
            "config",
            "--name-only",
            "--get-regexp",
            r"^http\..+\.extraheader$",
        ])
        .env_clear()
        .envs(inherited_agent_env(false));
    let out = crate::process::output(&mut cmd, None);
    let Ok(out) = out else {
        return Vec::new();
    };
    let mut urls: Vec<String> = String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|k| k.strip_prefix("http.")?.strip_suffix(".extraheader"))
        .map(str::to_owned)
        .collect();
    urls.sort();
    urls.dedup();
    urls
}

#[cfg(test)]
mod tests {
    use super::agent_allowed;
    use std::ffi::OsStr;

    fn ok(name: &str, windows: bool) -> bool {
        agent_allowed(OsStr::new(name), windows, false)
    }

    #[test]
    fn unix_allowlist() {
        for n in [
            "PATH", "HOME", "TMPDIR", "LANG", "LC_CTYPE", "LC_ALL", "TERM",
        ] {
            assert!(ok(n, false), "{n}");
        }
        for n in [
            "Path",
            "CLAUDE_CONFIG_DIR",
            "ANTHROPIC_AUTH_TOKEN",
            "ANTHROPIC_FOUNDRY_API_KEY",
            "SystemRoot",
        ] {
            assert!(!ok(n, false), "{n}");
        }
    }

    /// Network plumbing from the user's own env must reach the agent (proxies, corporate CAs), but never
    /// anything that picks the account. ssh-agent only with the account's opt-in.
    #[test]
    fn network_and_session_vars_pass_auth_vars_do_not() {
        for windows in [false, true] {
            for n in [
                "HTTPS_PROXY",
                "HTTP_PROXY",
                "NO_PROXY",
                "ALL_PROXY",
                "NODE_EXTRA_CA_CERTS",
                "SSL_CERT_FILE",
                "SSL_CERT_DIR",
                "TZ",
            ] {
                assert!(ok(n, windows), "{n} windows={windows}");
            }
            for n in [
                "ANTHROPIC_BASE_URL",
                "CLAUDE_CODE_USE_BEDROCK",
                "CLAUDE_CODE_OAUTH_TOKEN",
                "CURSOR_CONFIG_DIR",
                "CURSOR_API_KEY",
                "CURSOR_AUTH_TOKEN",
                "CURSOR_BACKEND_URL",
                "SSH_AUTH_SOCK",
            ] {
                assert!(!ok(n, windows), "{n} windows={windows}");
            }
            assert!(agent_allowed(OsStr::new("SSH_AUTH_SOCK"), windows, true));
        }
        for n in [
            "https_proxy",
            "no_proxy",
            "XDG_CONFIG_HOME",
            "XDG_RUNTIME_DIR",
        ] {
            assert!(ok(n, false), "{n}");
        }
        for n in [
            "SystemDrive",
            "WINDIR",
            "ProgramFiles",
            "ProgramFiles(x86)",
            "ProgramData",
            "HOMEDRIVE",
            "HOMEPATH",
            "CLAUDE_CODE_GIT_BASH_PATH",
        ] {
            assert!(ok(n, true), "{n}");
        }
    }

    #[test]
    fn windows_allowlist_ignores_case() {
        for n in [
            "Path",
            "PATH",
            "pathext",
            "SystemRoot",
            "SYSTEMROOT",
            "UserProfile",
            "TEMP",
        ] {
            assert!(ok(n, true), "{n}");
        }
        for n in [
            "CLAUDE_CONFIG_DIR",
            "ANTHROPIC_AUTH_TOKEN",
            "HOME",
            "LC_ALL",
            "claude_config_dir",
        ] {
            assert!(!ok(n, true), "{n}");
        }
    }
}
