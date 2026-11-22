//! A run dir left by a crashed run with the same pid must not block new grants.
#![cfg(unix)]
use skyelab_agent::mcp::{Caller, Host, Proxy, Server, Tool};
use std::sync::Arc;

struct Nop;
impl Host for Nop {
    fn call(&self, _: &Caller, _: Tool) -> Result<serde_json::Value, String> {
        Err("no".into())
    }
}

#[test]
fn a_reused_run_dir_still_grants() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let run = tmp.path().join("run");
    std::fs::create_dir_all(&run).expect("mkdir");
    std::fs::write(run.join("grant-0.json"), "{\"stale\":true}").expect("stale grant");
    let server = Server::start(&run, Arc::new(Nop)).expect("start");
    let g = server.grant(
        Caller {
            thread_id: 1,
            account_id: "acme".into(),
        },
        &Proxy {
            program: "/bin/true".into(),
            args: vec![],
        },
    );
    assert!(g.is_ok(), "grant failed on a leftover file: {:?}", g.err());
}
