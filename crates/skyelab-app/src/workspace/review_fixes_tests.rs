//! Tests for batches of review fixes, kept out of `mod.rs`. Each test file names workspace items through `super::`, which this glob makes resolve.

use super::*;

#[path = "review_fixes_d_test.rs"]
mod review_fixes_d_test;

#[path = "project_name_test.rs"]
mod project_name_test;

#[path = "pr_unlink_test.rs"]
mod pr_unlink_test;

#[cfg(unix)] // the fake gh is a shell script
#[path = "pr_newest_test.rs"]
mod pr_newest_test;

#[cfg(unix)] // the fake gh is a shell script
#[path = "pr_poll_test.rs"]
mod pr_poll_test;

#[path = "panels/sidebar_card_test.rs"]
mod sidebar_card_test;

#[cfg(unix)] // the fake agent is a shell script
#[path = "git_menu_rebind_test.rs"]
mod git_menu_rebind_test;

#[cfg(unix)] // the fake agent is a shell script
#[path = "launch_rebind_test.rs"]
mod launch_rebind_test;

#[cfg(unix)] // the fake agent is a shell script
#[path = "review_second_row_test.rs"]
mod review_second_row_test;
