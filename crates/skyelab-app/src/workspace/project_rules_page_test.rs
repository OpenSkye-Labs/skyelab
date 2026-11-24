//! Settings → Chat's permission rules for a picked project: listed for the folder's account only, a pattern edited
//! (and refused when it would widen or chain), and a rule removed.

use super::ProjectRulesPage;
use gpui::TestAppContext;
use skyelab_core::project_rules::{Kind, Rule};
use skyelab_store::Store;
use std::path::Path;
use std::sync::{Arc, Mutex};

fn rule(tool: &str, kind: Kind, pattern: &str) -> Rule {
    Rule { tool: tool.into(), kind, pattern: pattern.into() }
}

#[gpui::test]
fn lists_edits_and_removes_the_accounts_rules(cx: &mut TestAppContext) {
    cx.update(gpui_component::init);
    let store = Store::open_in_memory().expect("store");
    let dir = Path::new("/w/proj");
    let p = store.create_project("proj", dir).expect("project");
    store.add_project_rule(p.id, "work", &rule("Bash", Kind::Prefix, "npm test")).expect("add");
    store.add_project_rule(p.id, "work", &rule("Edit", Kind::Path, "src/**")).expect("add");
    store.add_project_rule(p.id, "personal", &rule("Bash", Kind::Exact, "ls")).expect("add");
    let store = Arc::new(Mutex::new(store));
    let (page, cx) = cx.add_window_view(|_, cx| ProjectRulesPage::new(store.clone(), cx));

    page.update(cx, |pg, cx| pg.show(Some(dir), Some("work".into()), cx));
    let listed = |cx: &mut gpui::VisualTestContext| {
        page.read_with(cx, |pg, _| pg.rules.iter().map(|r| r.rule.pattern.clone()).collect::<Vec<_>>())
    };
    assert_eq!(listed(cx), ["npm test", "src/**"], "the bound account's only");
    page.update(cx, |pg, cx| pg.show(Some(dir), None, cx));
    assert!(listed(cx).is_empty(), "an unbound folder lists none");
    page.update(cx, |pg, cx| pg.show(Some(dir), Some("work".into()), cx));

    // Edit: a compound prefix and a path out of the project are refused; a narrower prefix is saved.
    let id = page.read_with(cx, |pg, _| pg.rules[0].id);
    for (bad, ix) in [("npm test && rm -rf ~", 0usize), ("../**", 1)] {
        let target = page.read_with(cx, |pg, _| pg.rules[ix].id);
        page.update_in(cx, |pg, window, cx| pg.edit(target, window, cx));
        let field = page.read_with(cx, |pg, _| pg.editing.clone().expect("editing").1);
        field.update_in(cx, |f, window, cx| f.set_value(bad, window, cx));
        page.update(cx, |pg, cx| pg.save_edit(cx));
        assert!(page.read_with(cx, |pg, _| pg.error.is_some()), "{bad} was taken");
        assert_eq!(listed(cx), ["npm test", "src/**"], "nothing changed");
    }
    page.update_in(cx, |pg, window, cx| pg.edit(id, window, cx));
    let field = page.read_with(cx, |pg, _| pg.editing.clone().expect("editing").1);
    field.update_in(cx, |f, window, cx| f.set_value("npm test --watch", window, cx));
    page.update(cx, |pg, cx| pg.save_edit(cx));
    assert_eq!(listed(cx), ["npm test --watch", "src/**"]);
    assert!(page.read_with(cx, |pg, _| pg.editing.is_none() && pg.error.is_none()));

    page.update(cx, |pg, cx| pg.remove(id, cx));
    assert_eq!(listed(cx), ["src/**"]);
    let left = store.lock().expect("lock").project_rules(p.id, "personal").expect("list");
    assert_eq!(left.len(), 1, "another account's rule is untouched");
}
