//! S2 (session end): an agent that exits while a tool it started still holds its stdout is seen as ended once it
//! exits, not when that tool lets go of the pipe; and the tool, in the agent's process group, is ended with it.

use super::claude_folded_turns_test::{one_view, row, wait_until};
use gpui::TestAppContext;
use skyelab_core::Provider;
use std::path::Path;
use std::time::{Duration, Instant};

fn alive(pid: &str) -> bool {
    std::process::Command::new("/bin/kill")
        .args(["-0", pid])
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

#[gpui::test]
fn an_agent_that_exits_ends_its_row_and_its_group(cx: &mut TestAppContext) {
    let bin = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/exit-holding-stdout.sh");
    let (one, cx) = one_view(cx, Provider::Claude, bin, None);
    one.view
        .update_in(cx, |v, window, cx| v.submit_remote("go", window, cx))
        .expect("started");
    let pid_file = one.dir.join(".holder-pid");
    wait_until(cx, "the agent's exit", |_| {
        std::fs::read_to_string(&pid_file).is_ok_and(|p| !p.trim().is_empty())
    });
    let exited = Instant::now();
    let holder = std::fs::read_to_string(&pid_file).expect("pid");
    let holder = holder.trim();
    wait_until(cx, "the row to say exited", |cx| {
        row(&one, cx).is_some_and(|(status, _)| status == "exited")
    });
    assert!(
        exited.elapsed() < Duration::from_secs(5),
        "exited only after {:?}",
        exited.elapsed()
    );
    assert!(
        !alive(holder),
        "the tool holding stdout ({holder}) was ended"
    );
}
