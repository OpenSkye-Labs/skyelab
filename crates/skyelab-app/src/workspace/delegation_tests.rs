//! The delegation tests (a thread's agent starting others), kept out of `mod.rs`. Each test file names workspace
//! items through `super::`, which this glob makes resolve as before.

use super::*;

#[cfg(unix)]
#[path = "delegation_after_stop_test.rs"]
mod delegation_after_stop_test;

#[path = "delegation_cap_test.rs"]
mod delegation_cap_test;

#[cfg(unix)]
#[path = "delegation_idempotent_test.rs"]
mod delegation_idempotent_test;

#[cfg(unix)]
#[path = "delegation_reply_on_row_test.rs"]
mod delegation_reply_on_row_test;

#[path = "delegation_resume_test.rs"]
mod delegation_resume_test;

#[cfg(unix)]
#[path = "delegation_review_test.rs"]
mod delegation_review_test;

#[cfg(unix)]
#[path = "delegation_send_cap_test.rs"]
mod delegation_send_cap_test;

#[cfg(unix)]
#[path = "delegation_starting_cap_test.rs"]
mod delegation_starting_cap_test;

#[cfg(unix)]
#[path = "delegation_stopped_parent_test.rs"]
mod delegation_stopped_parent_test;

#[cfg(unix)] // the fake agent is a shell script
#[path = "delegation_turns_test.rs"]
mod delegation_turns_test;
