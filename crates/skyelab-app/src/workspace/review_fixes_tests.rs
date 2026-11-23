//! Tests for batches of review fixes, kept out of `mod.rs`. Each test file names workspace items through `super::`, which this glob makes resolve.

use super::*;

#[path = "review_fixes_d_test.rs"]
mod review_fixes_d_test;

#[path = "project_name_test.rs"]
mod project_name_test;

#[path = "pr_unlink_test.rs"]
mod pr_unlink_test;

#[path = "panels/sidebar_card_test.rs"]
mod sidebar_card_test;

#[cfg(unix)] // the fake agent is a shell script
#[path = "git_menu_rebind_test.rs"]
mod git_menu_rebind_test;
