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
