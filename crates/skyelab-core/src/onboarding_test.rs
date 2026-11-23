use super::onboarding::*;
use super::*;
use std::fs;

fn tree(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for e in fs::read_dir(dir).expect("read_dir").flatten() {
        let p = e.path();
        if p.is_dir() {
            out.extend(tree(&p));
        }
        out.push(p);
    }
    out.sort();
    out
}

fn home() -> tempfile::TempDir {
    let home = tempfile::tempdir().expect("tempdir");
    let h = home.path();
    fs::create_dir_all(h.join(".claude/projects")).expect("mkdir");
    fs::create_dir_all(h.join(".claude-work")).expect("mkdir");
    fs::write(h.join(".claude-work/.claude.json"), "{}").expect("write");
    fs::create_dir_all(h.join(".codex")).expect("mkdir");
    fs::write(h.join(".codex/config.toml"), "").expect("write");
    fs::create_dir_all(h.join(".claude-empty")).expect("mkdir"); // no marker: not a login
    fs::create_dir_all(h.join(".claudette/projects")).expect("mkdir"); // not ".claude-*"
    fs::write(h.join(".claude.json"), "{}").expect("write"); // a file, not a config dir
    home
}

#[test]
fn onboarding_finds_marked_config_dirs_only() {
    let home = home();
    let found = find_accounts(home.path(), &AccountsConfig::default());
    let got: Vec<_> = found
        .iter()
        .map(|f| (f.provider, f.id.as_str(), f.label.as_str()))
        .collect();
    assert_eq!(
        got,
        [
            (Provider::Claude, "claude", "Claude"),
            (Provider::Claude, "claude-work", "Claude (work)"),
            (Provider::Codex, "codex", "Codex"),
        ]
    );
    assert_eq!(found[1].config_dir, home.path().join(".claude-work"));
}

#[test]
fn onboarding_finds_nothing_in_an_empty_or_missing_home() {
    let home = tempfile::tempdir().expect("tempdir");
    assert!(find_accounts(home.path(), &AccountsConfig::default()).is_empty());
    assert!(find_accounts(&home.path().join("gone"), &AccountsConfig::default()).is_empty());
    assert!(find_repos(home.path()).is_empty());
}

#[test]
fn onboarding_skips_added_dirs_and_keeps_ids_unique() {
    let home = home();
    let mut config = AccountsConfig::default();
    for (id, dir) in [("claude", "/elsewhere"), ("x", "")] {
        let dir = if dir.is_empty() {
            home.path().join(".codex")
        } else {
            PathBuf::from(dir)
        };
        config
            .add_account(Account {
                id: id.into(),
                label: id.into(),
                provider: Provider::Claude,
                config_dir: dir,
                gh_config_dir: None,
                ssh_agent: false,
            })
            .expect("add");
    }
    let found = find_accounts(home.path(), &config);
    let ids: Vec<_> = found.iter().map(|f| f.id.as_str()).collect();
    assert_eq!(ids, ["claude-2", "claude-work"], "codex is added already");
}

#[test]
fn onboarding_detection_writes_nothing() {
    let home = home();
    let before = tree(home.path());
    find_accounts(home.path(), &AccountsConfig::default());
    find_repos(home.path());
    assert_eq!(tree(home.path()), before);
}

/// Credential files and markers are FIFOs: opening one blocks forever, so this only finishes if detection never
/// opens a file.
#[cfg(unix)]
#[test]
fn onboarding_never_opens_credentials_or_markers() {
    let home = tempfile::tempdir().expect("tempdir");
    let h = home.path();
    fs::create_dir_all(h.join(".claude")).expect("mkdir");
    fs::create_dir_all(h.join(".codex")).expect("mkdir");
    for f in [
        ".claude/.credentials.json",
        ".claude/settings.json",
        ".codex/auth.json",
        ".codex/config.toml",
    ] {
        let ok = std::process::Command::new("mkfifo")
            .arg(h.join(f))
            .status()
            .expect("mkfifo runs");
        assert!(ok.success());
    }
    let found = find_accounts(h, &AccountsConfig::default());
    assert_eq!(found.len(), 2);
}

#[test]
fn onboarding_find_repos_scans_one_level() {
    let home = tempfile::tempdir().expect("tempdir");
    let h = home.path();
    for d in [
        "code/a/.git",
        "projects/b/.git",
        "dev/c",
        "src/deep/d/.git",
        "other/e/.git",
    ] {
        fs::create_dir_all(h.join(d)).expect("mkdir");
    }
    fs::create_dir_all(h.join("src/wt")).expect("mkdir");
    fs::write(h.join("src/wt/.git"), "gitdir: x").expect("write"); // a worktree's .git file
    let mut got = find_repos(h);
    got.sort();
    let want: Vec<PathBuf> = ["code/a", "projects/b", "src/wt"]
        .iter()
        .map(|p| h.join(p))
        .collect();
    assert_eq!(got, want);
}

#[test]
fn bind_project_binds_once_and_never_overwrites() {
    let root = tempfile::tempdir().expect("tempdir");
    let (work, repo) = (root.path().join("work"), root.path().join("work/repo"));
    fs::create_dir_all(&repo).expect("mkdir");
    let mut config: AccountsConfig = toml::from_str("[[account]]\nid='a'\nlabel='A'\nconfig_dir='/a'\n[[account]]\nid='b'\nlabel='B'\nconfig_dir='/b'\n")
    .expect("parse");
    config
        .add_binding(Binding::new(work.clone(), "a".into()))
        .expect("bind");
    bind_project(&mut config, &repo, "a").expect("already a's");
    assert_eq!(config.bindings.len(), 1, "inherited binding is enough");
    bind_project(&mut config, &repo, "b").expect("deeper binding");
    assert_eq!(config.resolve(&repo).map(|a| a.id.as_str()), Some("b"));
    assert!(
        bind_project(&mut config, &repo, "a").is_err(),
        "bound to b: not overwritten"
    );
    assert!(bind_project(&mut config, &repo, "nobody").is_err());
}
