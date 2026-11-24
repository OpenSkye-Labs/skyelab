//! What a thread has beyond running its agent: the defaults new threads start with (Settings → Threads), generated
//! titles and forks. Drafts and prompt history are the composer's (`composer_draft.rs`).

#[path = "fork.rs"]
pub(super) mod fork;
#[path = "handoff.rs"]
pub(super) mod handoff;
#[path = "thread_prefs.rs"]
pub(crate) mod prefs;
#[path = "thread_title.rs"]
pub(super) mod title;

#[cfg(test)]
#[path = "context_fence_test.rs"]
mod context_fence_test;
#[cfg(test)]
#[path = "context_fence_title_test.rs"]
mod context_fence_title_test;
#[cfg(all(test, unix))]
#[path = "draft_test.rs"]
mod draft_test;
#[cfg(test)]
#[path = "fork_cost_reload_test.rs"]
mod fork_cost_reload_test;
#[cfg(test)]
#[path = "fork_cost_test.rs"]
mod fork_cost_test;
#[cfg(test)]
#[path = "fork_cut_gone_test.rs"]
mod fork_cut_gone_test;
#[cfg(all(test, unix))]
#[path = "fork_test.rs"]
mod fork_test;
#[cfg(all(test, unix))]
#[path = "handoff_chain_test.rs"]
mod handoff_chain_test;
#[cfg(all(test, unix))] // the fake codex is a shell script
#[path = "handoff_test.rs"]
mod handoff_test;
#[cfg(all(test, unix))]
#[path = "prompt_history_test.rs"]
mod prompt_history_test;
#[cfg(all(test, unix))]
#[path = "thread_defaults_test.rs"]
mod thread_defaults_test;
#[cfg(all(test, unix))]
#[path = "thread_title_off_test.rs"]
mod thread_title_off_test;
#[cfg(all(test, unix))]
#[path = "thread_title_regen_test.rs"]
mod thread_title_regen_test;
#[cfg(all(test, unix))] // the fake agents are shell scripts
#[path = "thread_title_test.rs"]
mod thread_title_test;
