//! The delegation-across-machines tests, kept out of `mod.rs`. Each runs the fake claude (a shell script), so
//! they are unix-only, and names workspace items through `super::`, which this glob makes resolve as before.

use super::*;

#[path = "remote_delegation_harness.rs"]
mod remote_delegation_harness;

#[path = "remote_delegation_refuse_test.rs"]
mod remote_delegation_refuse_test;

#[path = "remote_delegation_start_test.rs"]
mod remote_delegation_start_test;

#[path = "remote_delegation_cap_test.rs"]
mod remote_delegation_cap_test;

#[path = "remote_delegation_depth_test.rs"]
mod remote_delegation_depth_test;

#[path = "remote_delegation_projects_test.rs"]
mod remote_delegation_projects_test;

#[path = "remote_delegation_reach_test.rs"]
mod remote_delegation_reach_test;

#[path = "remote_delegation_retry_test.rs"]
mod remote_delegation_retry_test;

#[path = "remote_delegation_title_test.rs"]
mod remote_delegation_title_test;

#[path = "remote_delegation_worktree_test.rs"]
mod remote_delegation_worktree_test;
