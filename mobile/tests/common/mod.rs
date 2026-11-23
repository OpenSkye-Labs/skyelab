//! The shared remote-test harness (`skyelab_daemon::testing`), seeded the way these tests expect: one thread with a
//! stored prompt and agent reply, served on loopback.

use skyelab_daemon::testing::ca;
pub use skyelab_daemon::testing::{Env, NODE_NAME, Peer, Ran};

/// [`Env::start`] plus two stored events on its thread: a prompt and the agent's reply.
pub fn start() -> Env {
    let env = Env::start();
    env.append(env.thread_id, "prompt", "\"fix the login\"");
    env.append(
        env.thread_id,
        "agent",
        r#"{"Assistant":{"content":[{"type":"text","text":"on it"}]}}"#,
    );
    env
}

/// Client options for the loopback daemon (test-only feature): trusts the test CA and dials `NODE_NAME` on loopback,
/// so TLS still checks the name. Quick retries.
pub fn options(port: u16) -> skyelab_client::Options {
    skyelab_client::testing::options(port, ca().root())
}
