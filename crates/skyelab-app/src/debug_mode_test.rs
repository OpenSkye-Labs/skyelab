//! The debug state dump's format.

use super::*;

#[test]
fn the_snapshot_has_every_section_and_no_extra_text() {
    let mut entry = debug::Entry::new(debug::Kind::Rpc, "client list_threads")
        .ms(Duration::from_millis(7))
        .detail("120 B");
    entry.at_ms = 1;
    let value = snapshot(
        FrameStats {
            fps: 59.8,
            last: Duration::from_micros(4_150),
            worst: Duration::from_millis(18),
        },
        &[("Workspace", 60), ("ThreadView", 58)],
        &[("windows", 1), ("terminals", 0)],
        &[ChildProc {
            pid: 42,
            name: "claude".into(),
            run_secs: 9,
        }],
        &[entry],
    );
    let mut keys: Vec<&str> = value
        .as_object()
        .expect("an object")
        .keys()
        .map(String::as_str)
        .collect();
    keys.sort_unstable();
    assert_eq!(
        keys,
        [
            "build",
            "child_processes",
            "debug_mode",
            "frames",
            "live",
            "log",
            "os",
            "renders_per_second",
            "skyelab"
        ]
    );
    assert_eq!(value["frames"]["fps"], 59.8);
    assert_eq!(value["frames"]["last_ms"], 4.2);
    assert_eq!(value["renders_per_second"]["ThreadView"], 58);
    assert_eq!(value["live"]["windows"], 1);
    assert_eq!(value["child_processes"][0]["name"], "claude");
    assert_eq!(value["log"][0]["what"], "client list_threads");
    assert_eq!(value["log"][0]["kind"], "rpc");
    assert_eq!(value["log"][0]["ms"], 7);
}

#[test]
fn child_processes_lists_a_child_this_process_started() {
    #[cfg(unix)]
    {
        let mut child = std::process::Command::new("/bin/sleep")
            .arg("30")
            .spawn()
            .expect("sleep starts");
        let found = child_processes().iter().any(|c| c.pid == child.id());
        let _ = child.kill();
        let _ = child.wait();
        assert!(found, "the child is listed");
    }
}
