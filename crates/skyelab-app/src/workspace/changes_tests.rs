//! The Changes tab's tests, kept out of `mod.rs`. Each test file names workspace items through `super::`, which
//! this glob makes resolve as before.

use super::*;

#[path = "changes_cap_test.rs"]
mod changes_cap_test;

#[path = "changes_find_test.rs"]
mod changes_find_test;

#[path = "changes_follow_folder_test.rs"]
mod changes_follow_folder_test;

#[path = "changes_long_test.rs"]
mod changes_long_test;

#[cfg(unix)]
#[path = "changes_selection_test.rs"]
mod changes_selection_test;

#[path = "changes_tree_test.rs"]
mod changes_tree_test;

#[path = "changes_whitespace_test.rs"]
mod changes_whitespace_test;

#[path = "changes_loading_test.rs"]
mod changes_loading_test;
