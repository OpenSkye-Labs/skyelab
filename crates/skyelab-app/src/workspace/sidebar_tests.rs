//! The sidebar's tests (rows, sections, accounts, status, management), kept out of `mod.rs`. Each test file names
//! workspace items through `super::`, which this glob makes resolve as before.

use super::*;

#[path = "manage_sidebar_test.rs"]
mod manage_sidebar_test;

#[path = "sidebar_account_test.rs"]
mod sidebar_account_test;

#[path = "sidebar_flat_test.rs"]
mod sidebar_flat_test;

#[cfg(unix)] // the fake agent is a shell script
#[path = "sidebar_status_test.rs"]
mod sidebar_status_test;

#[cfg(unix)] // the fake agent is a shell script
#[path = "working_shelf_test.rs"]
mod working_shelf_test;

#[cfg(unix)] // the fake agent is a shell script
#[path = "working_elapsed_test.rs"]
mod working_elapsed_test;

#[cfg(unix)] // the fake agent is a shell script
#[path = "sidebar_rebuild_test.rs"]
mod sidebar_rebuild_test;
