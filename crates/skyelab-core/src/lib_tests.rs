use super::*;

const CFG: &str = r#"
    [[account]]
    id = "acme"
    label = "Acme (Team)"
    config_dir = "/home/me/.claude-acme"

    [[account]]
    id = "personal"
    label = "Personal (Max)"
    config_dir = "/home/me/.claude"

    [[binding]]
    path = "/nonexistent/work/acme"
    resolved = "/nonexistent/work/acme"
    account = "acme"

    [[binding]]
    path = "/nonexistent/work/acme/oss-fork"
    resolved = "/nonexistent/work/acme/oss-fork"
    account = "personal"
"#;

fn cfg() -> AccountsConfig {
    let c: AccountsConfig = toml::from_str(CFG).expect("parse");
    c.validate().expect("valid");
    c
}

fn resolved(dir: &str) -> Option<String> {
    cfg().resolve(Path::new(dir)).map(|a| a.id.clone())
}

#[test]
fn deepest_binding_wins() {
    assert_eq!(
        resolved("/nonexistent/work/acme/api").as_deref(),
        Some("acme")
    );
    assert_eq!(
        resolved("/nonexistent/work/acme/oss-fork/src").as_deref(),
        Some("personal")
    );
}

#[test]
fn prefix_is_component_wise_and_unbound_is_none() {
    assert_eq!(resolved("/nonexistent/work/acme-old"), None);
    assert_eq!(resolved("/nonexistent/elsewhere"), None);
    assert!(
        cfg()
            .launch_env(Path::new("/nonexistent/elsewhere"))
            .is_err()
    );
}

#[test]
fn launch_env_refuses_a_relative_config_dir_even_unvalidated() {
    let mut c = cfg();
    c.accounts[0].config_dir = ".claude".into();
    assert!(c.launch_env(Path::new("/nonexistent/work/acme")).is_err());
}

#[test]
fn relative_dir_and_dotdot_binding_never_resolve() {
    assert_eq!(resolved("nonexistent/work/acme"), None);
    let mut c = cfg();
    c.bindings[0].path = "/nonexistent/x/../work/acme".into();
    assert!(c.validate().is_err());
    assert!(c.resolve(Path::new("/nonexistent/work/acme")).is_none());
}

#[test]
fn unknown_account_is_rejected() {
    let mut c = cfg();
    c.bindings[0].account = "typo".into();
    assert!(c.validate().is_err());
}

fn hooks(c: &AccountsConfig, dir: &str, worktrees: &[&str]) -> bool {
    let wt: Vec<PathBuf> = worktrees.iter().map(PathBuf::from).collect();
    c.repo_hooks_allowed(Path::new(dir), &wt)
}

fn bind(c: &mut AccountsConfig, path: &str, account: &str) {
    c.bindings.push(Binding {
        path: path.into(),
        account: account.into(),
        resolved: Some(path.into()),
    });
}

#[test]
fn single_account_repo_keeps_hooks() {
    let c = cfg();
    let repo = "/nonexistent/work/acme/api";
    assert!(hooks(&c, repo, &[repo]));
    // A worktree under the same account, and a deeper binding to the same account, still keep them.
    let mut c = cfg();
    bind(&mut c, "/nonexistent/work/acme/api/web", "acme");
    assert!(hooks(&c, repo, &[repo, "/nonexistent/work/acme/api-wt"]));
}

#[test]
fn shared_worktree_on_another_account_skips_hooks() {
    let c = cfg();
    // The main checkout is acme's; the worktree is personal's (and the other way round).
    let (main, wt) = (
        "/nonexistent/work/acme/api",
        "/nonexistent/work/acme/oss-fork/wt",
    );
    assert!(!hooks(&c, wt, &[main, wt]));
    assert!(!hooks(&c, main, &[main, wt]));
}

#[test]
fn nested_binding_inside_the_repo_skips_hooks() {
    // /work/acme is acme's, /work/acme/oss-fork inside it is personal's: one repo at /work/acme.
    let c = cfg();
    let repo = "/nonexistent/work/acme";
    assert!(!hooks(&c, repo, &[repo]));
    assert!(!hooks(&c, "/nonexistent/work/acme/oss-fork", &[repo]));
}

#[test]
fn subfolder_project_in_an_unbound_repo_skips_hooks() {
    let mut c = cfg();
    bind(&mut c, "/nonexistent/mono/app", "acme");
    assert!(!hooks(&c, "/nonexistent/mono/app", &["/nonexistent/mono"]));
    // An unbound worktree of a bound repo counts as another account too.
    assert!(!hooks(
        &c,
        "/nonexistent/work/acme/api",
        &["/nonexistent/work/acme/api", "/nonexistent/elsewhere/wt"]
    ));
}

#[test]
fn unbound_dir_or_stale_binding_inside_skips_hooks() {
    let c = cfg();
    assert!(!hooks(
        &c,
        "/nonexistent/elsewhere",
        &["/nonexistent/elsewhere"]
    ));
    let mut c = cfg();
    // Confirmed at a folder inside the repo, but its path now points elsewhere: stale, so not acme's.
    c.bindings.push(Binding {
        path: "/nonexistent/other/place".into(),
        account: "acme".into(),
        resolved: Some("/nonexistent/work/acme/api/old".into()),
    });
    let repo = "/nonexistent/work/acme/api";
    assert!(!hooks(&c, repo, &[repo]));
}

#[test]
fn a_repo_inside_another_accounts_folder_skips_hooks() {
    // oss-fork is personal's own repo, but it sits in acme's folder: acme's agents could write its .git.
    let c = cfg();
    let inner = "/nonexistent/work/acme/oss-fork";
    assert!(!hooks(
        &c,
        inner,
        &[inner, "/nonexistent/work/acme/oss-fork/.git"]
    ));
    // The hooks dir counts too: one in another account's folder could be written from there.
    let repo = "/nonexistent/work/acme/api";
    assert!(!hooks(
        &c,
        repo,
        &[repo, "/nonexistent/work/acme/oss-fork/hooks"]
    ));
    assert!(!hooks(&c, repo, &[]));
}
