//! Acceptance: "Allow for this project" rules are stored per project and per account, edited and removed, and go
//! with their project.

use skyelab_core::project_rules::{Kind, Rule};
use skyelab_store::Store;
use std::path::Path;

fn rule(tool: &str, kind: Kind, pattern: &str) -> Rule {
    Rule {
        tool: tool.into(),
        kind,
        pattern: pattern.into(),
    }
}

#[test]
fn rules_never_cross_accounts_or_projects() {
    let store = Store::open_in_memory().expect("store");
    let a = store.create_project("a", Path::new("/a")).expect("a");
    let b = store.create_project("b", Path::new("/b")).expect("b");
    let test = rule("Bash", Kind::Prefix, "npm test");
    store.add_project_rule(a.id, "work", &test).expect("add");
    store
        .add_project_rule(a.id, "work", &test)
        .expect("again keeps one row");
    store
        .add_project_rule(a.id, "work", &rule("Edit", Kind::Path, "src/**"))
        .expect("add");
    let work = store.project_rules(a.id, "work").expect("list");
    assert_eq!(
        work.iter().map(|r| r.rule.clone()).collect::<Vec<_>>(),
        [test.clone(), rule("Edit", Kind::Path, "src/**")]
    );
    assert!(
        store
            .project_rules(a.id, "personal")
            .expect("list")
            .is_empty(),
        "another account"
    );
    assert!(
        store.project_rules(b.id, "work").expect("list").is_empty(),
        "another project"
    );
    assert!(
        store.add_project_rule(999, "work", &test).is_err(),
        "no such project"
    );
}

#[test]
fn rules_are_edited_removed_and_go_with_the_project() {
    let store = Store::open_in_memory().expect("store");
    let a = store.create_project("a", Path::new("/a")).expect("a");
    store
        .add_project_rule(a.id, "work", &rule("Bash", Kind::Prefix, "npm test"))
        .expect("add");
    let id = store.project_rules(a.id, "work").expect("list")[0].id;
    assert!(
        store
            .set_project_rule_pattern(id, "personal", "rm")
            .is_err(),
        "only by its own account"
    );
    store
        .set_project_rule_pattern(id, "work", "npm run test")
        .expect("edit");
    assert_eq!(
        store.project_rules(a.id, "work").expect("list")[0]
            .rule
            .pattern,
        "npm run test"
    );
    assert!(
        store.delete_project_rule(id, "personal").is_err(),
        "only by its own account"
    );
    store.delete_project_rule(id, "work").expect("delete");
    assert!(store.project_rules(a.id, "work").expect("list").is_empty());
    assert!(store.delete_project_rule(id, "work").is_err());
    store
        .add_project_rule(a.id, "work", &rule("Bash", Kind::Exact, "ls"))
        .expect("add");
    store.delete_project(a.id).expect("delete project");
    assert!(
        store.project_rules(a.id, "work").expect("list").is_empty(),
        "gone with the project"
    );
}
