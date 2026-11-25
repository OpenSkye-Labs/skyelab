//! claude.ai connectors on the details card: one the account never connected (Claude's `needs-auth` from source
//! `claudeai`) reads "Not connected" (no Sign in, not a failure), also once the agent has ended; a user server that
//! needs sign-in keeps "Needs sign-in". This window has no SkyeLab tools, so the launch is strict and the rows come
//! from the fake's fake-mcp-status.json rather than its connector list.

use super::*;
use crate::workspace::thread_details::mcp_state;

#[gpui::test]
fn an_unconnected_connector_is_quiet_on_the_card(cx: &mut TestAppContext) {
    setup(cx);
    let f = fixture(true);
    std::fs::create_dir_all(&f.account_dir).expect("account dir");
    std::fs::write(
        f.account_dir.join("fake-mcp-status.json"),
        r#"[{"name":"claude.ai Notion","status":"needs-auth","source":"claudeai"},{"name":"linear","status":"needs-auth","source":"user"}]"#,
    )
    .expect("statuses");
    let (ws, cx) = open_workspace(&f, cx);
    let thread = project_thread(&f, &ws, cx);
    type_and_send(&thread, cx);
    wait_done(&thread, cx);
    thread.read_with(cx, |t, _| {
        let find = |name: &str| {
            let s = t.details.mcp.iter().find(|s| s.name == name);
            s.cloned().unwrap_or_else(|| panic!("{name} not on the card: {:?}", t.details.mcp))
        };
        let notion = find("claude.ai Notion");
        assert_eq!(notion.status, "not-connected");
        assert_eq!(mcp_state(&notion, true), "Not connected", "stays after the agent ends");
        assert_eq!(find("linear").status, "needs-auth", "a user server keeps its Sign in");
    });
}
