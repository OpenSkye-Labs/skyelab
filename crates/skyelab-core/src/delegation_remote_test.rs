use super::*;
use crate::{Account, AccountsConfig, AppSettings, Binding, Provider};
use std::path::PathBuf;

fn login(email: &str, org: &str) -> Login {
    Login {
        provider: Provider::Claude,
        email: email.into(),
        org_id: org.into(),
    }
}

fn account(id: &str, provider: Provider, root: &std::path::Path) -> Account {
    Account {
        id: id.into(),
        label: id.into(),
        provider,
        config_dir: root.join(format!(".cfg-{id}")),
        gh_config_dir: None,
        ssh_agent: false,
    }
}

struct Rig {
    _dir: tempfile::TempDir,
    config: AccountsConfig,
    work: PathBuf,
    personal: PathBuf,
    codex: PathBuf,
    unbound: PathBuf,
}

fn rig() -> Rig {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = std::fs::canonicalize(dir.path()).expect("canonicalize");
    let mut config = AccountsConfig::default();
    let [work, personal, codex, unbound] =
        ["work", "personal", "codex", "unbound"].map(|n| root.join(n));
    for d in [&work, &personal, &codex, &unbound] {
        std::fs::create_dir_all(d).expect("mkdir");
    }
    for (id, p) in [
        ("work", Provider::Claude),
        ("personal", Provider::Claude),
        ("cx", Provider::Codex),
    ] {
        config.add_account(account(id, p, &root)).expect("account");
    }
    for (d, id) in [(&work, "work"), (&personal, "personal"), (&codex, "cx")] {
        config
            .add_binding(Binding::new(d.clone(), id.into()))
            .expect("bind");
    }
    Rig {
        _dir: dir,
        config,
        work,
        personal,
        codex,
        unbound,
    }
}

fn link(local: &str) -> DelegationLink {
    DelegationLink {
        node_id: "nA".into(),
        key_hash: "k".into(),
        machine: "a-mac.example.ts.net".into(),
        origin_account: "x".into(),
        origin_label: "Work".into(),
        login: login("me@acme.com", "org-acme"),
        local_account: local.into(),
    }
}

const WHO: (&str, &str, &str) = ("nA", "k", "x");

fn start(
    r: &Rig,
    links: &[DelegationLink],
    caller: Option<Login>,
    local: Option<Login>,
    dir: &std::path::Path,
) -> Result<String, Refusal> {
    check_remote_start(
        &r.config,
        links,
        WHO,
        caller.as_ref(),
        |_| local.clone(),
        dir,
    )
    .map(|a| a.id.clone())
}

#[test]
fn a_linked_caller_with_the_same_login_runs_under_the_folders_account() {
    let r = rig();
    let me = login("me@acme.com", "org-acme");
    assert_eq!(
        start(&r, &[link("work")], Some(me.clone()), Some(me), &r.work),
        Ok("work".into())
    );
}

#[test]
fn every_unproven_case_refuses() {
    let r = rig();
    let me = login("me@acme.com", "org-acme");
    let not = |res: Result<String, Refusal>| matches!(res, Err(Refusal::NotAllowed(_)));
    assert!(
        not(start(&r, &[], Some(me.clone()), Some(me.clone()), &r.work)),
        "no link"
    );
    assert!(
        not(start(
            &r,
            &[link("work")],
            Some(me.clone()),
            Some(me.clone()),
            &r.personal
        )),
        "a folder bound to another account than the link's"
    );
    let personal_plan = login("me@acme.com", "org-personal");
    assert!(
        not(start(
            &r,
            &[link("work")],
            Some(personal_plan.clone()),
            Some(personal_plan),
            &r.work
        )),
        "same email, another org"
    );
    assert!(
        not(start(
            &r,
            &[link("work")],
            Some(me.clone()),
            Some(login("me@acme.com", "org-other")),
            &r.work
        )),
        "this machine's login is another org"
    );
    assert!(
        not(start(
            &r,
            &[link("work")],
            Some(login("you@acme.com", "org-acme")),
            Some(me.clone()),
            &r.work
        )),
        "another email"
    );
    assert!(
        not(start(&r, &[link("work")], None, Some(me.clone()), &r.work)),
        "caller login unread"
    );
    assert!(
        not(start(&r, &[link("work")], Some(me.clone()), None, &r.work)),
        "local login unread"
    );
    assert!(
        not(start(
            &r,
            &[link("work")],
            Some(login("me@acme.com", "")),
            Some(login("me@acme.com", "")),
            &r.work
        )),
        "no org id: an API-key login proves nothing"
    );
    assert!(
        not(start(
            &r,
            &[link("cx")],
            Some(me.clone()),
            Some(me.clone()),
            &r.codex
        )),
        "a Codex account"
    );
    let other_device = DelegationLink {
        key_hash: "other".into(),
        ..link("work")
    };
    assert!(
        not(start(
            &r,
            &[other_device],
            Some(me.clone()),
            Some(me.clone()),
            &r.work
        )),
        "another device key"
    );
    match start(&r, &[link("work")], Some(me.clone()), Some(me), &r.unbound) {
        Err(Refusal::BadRequest(m)) => assert!(!m.contains('/'), "{m}"),
        other => panic!("an unbound folder is a bad request, got {other:?}"),
    }
}

#[test]
fn refusals_name_no_path() {
    let r = rig();
    let me = login("me@acme.com", "org-acme");
    for dir in [&r.work, &r.personal, &r.codex, &r.unbound] {
        if let Err(e) = start(
            &r,
            &[link("work")],
            Some(me.clone()),
            Some(login("x@y", "z")),
            dir,
        ) {
            assert!(!e.message().contains(dir.to_str().expect("utf8")), "{e:?}");
            assert!(!e.message().contains('/'), "{e:?}");
        }
    }
}

#[test]
fn depth_and_origin_caps() {
    assert_eq!(remote_child_depth(0), Ok(1));
    assert_eq!(remote_child_depth(1), Ok(2));
    assert!(remote_child_depth(2).is_err());
    assert!(check_origin(MAX_PER_ORIGIN - 1).is_ok());
    assert!(check_origin(MAX_PER_ORIGIN).is_err());
}

#[test]
fn clean_title_drops_control_and_bidi_chars() {
    assert_eq!(clean_title("fix\u{202E}it\u{0000}\nnow"), "fixitnow");
    assert_eq!(clean_title("\u{2066}a\u{2069}\u{200F}b"), "ab");
    assert_eq!(clean_title(&"x".repeat(500)).chars().count(), MAX_TITLE);
    assert_eq!(clean_title("  plain title "), "plain title");
}

#[test]
fn settings_keep_links_and_targets() {
    let mut s = AppSettings::default();
    assert!(
        !s.is_delegation_target("b.example.ts.net"),
        "off by default"
    );
    s.set_delegation_target("B.example.ts.net.", true);
    assert!(s.is_delegation_target("b.example.ts.net"));
    s.set_delegation_target("b.example.ts.net", false);
    assert!(!s.is_delegation_target("b.example.ts.net"));
    s.link_delegation(link("work"));
    s.link_delegation(link("personal"));
    assert_eq!(
        s.delegation.links.len(),
        1,
        "one link per (node, key, account)"
    );
    assert_eq!(s.delegation.links[0].local_account, "personal");
    let text = toml::to_string(&s).expect("serialize");
    let back: AppSettings = toml::from_str(&text).expect("parse");
    assert_eq!(back, s);
    assert!(s.revoke_delegation("nA", "k", "x"));
    assert!(!s.revoke_delegation("nA", "k", "x"));
    assert!(s.delegation.links.is_empty());
}
