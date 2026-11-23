//! What one `AccountsConfig::resolve` costs on local disk, for the decision in docs/decisions.md.
//! Run: cargo test -p skyelab-core --release --test resolve_cost -- --ignored --nocapture
use skyelab_core::{Account, AccountsConfig, Binding};
use std::time::Instant;

#[test]
#[ignore = "a measurement, run on demand"]
fn resolve_costs_well_under_a_millisecond_with_twenty_bindings() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = std::fs::canonicalize(tmp.path()).expect("canonicalize");
    let mut c = AccountsConfig::default();
    for i in 0..20 {
        let id = format!("a{i}");
        c.add_account(Account {
            id: id.clone(),
            label: id.clone(),
            config_dir: root.join(format!(".claude-{i}")),
            provider: Default::default(),
            gh_config_dir: None,
            ssh_agent: false,
        })
        .expect("account");
        let dir = root.join(format!("work/team-{i}/repo"));
        std::fs::create_dir_all(&dir).expect("mkdir");
        c.add_binding(Binding::new(dir, id)).expect("bind");
    }
    let deep = root.join("work/team-7/repo/crates/app/src/deep");
    std::fs::create_dir_all(&deep).expect("mkdir");
    let missing = root.join("work/team-3/repo/not/yet/made");
    for (what, dir) in [("existing", &deep), ("missing", &missing)] {
        assert!(c.resolve(dir).is_some());
        let n = 2_000;
        let at = Instant::now();
        for _ in 0..n {
            std::hint::black_box(c.resolve(std::hint::black_box(dir)));
        }
        let each = at.elapsed() / n;
        println!("resolve() cost, 20 bindings, {what} folder: {each:?} per call");
        assert!(
            each.as_micros() < 1_000,
            "{what}: {each:?} per call is too slow for the UI thread"
        );
    }
}
