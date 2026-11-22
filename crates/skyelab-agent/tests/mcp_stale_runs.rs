//! Run dirs of SkyeLab processes that are gone are removed; live ones are kept.
#![cfg(unix)]

use skyelab_agent::mcp::remove_stale_runs;

#[test]
fn removes_dirs_of_dead_runs_only() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();
    let mut dead = std::process::Command::new("/usr/bin/true")
        .spawn()
        .expect("spawn");
    dead.wait().expect("reap");
    let mut live = std::process::Command::new("/bin/sleep")
        .arg("30")
        .spawn()
        .expect("spawn");
    let dirs = [
        dead.id().to_string(),
        live.id().to_string(),
        std::process::id().to_string(),
        "notes".to_owned(),
    ];
    for d in &dirs {
        std::fs::create_dir_all(root.join(d)).expect("mkdir");
        std::fs::write(root.join(d).join("mcp.sock"), "").expect("file");
    }
    remove_stale_runs(root);
    let _ = live.kill();
    let _ = live.wait();
    assert!(!root.join(&dirs[0]).exists(), "a dead run's dir goes");
    for d in &dirs[1..] {
        assert!(root.join(d).exists(), "{d} is kept");
    }
}
