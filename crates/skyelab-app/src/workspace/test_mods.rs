// The workspace's test modules, kept out of mod.rs (its size cap); pulled in with include!, so they stay children of
// `workspace`.

#[cfg(all(test, unix))]
mod accounts_file_reread_test;
// Unix: the rows show `~/` paths and the rig's gh is a shell script.
#[cfg(all(test, unix))]
mod add_project_bind_test;
#[cfg(all(test, unix, debug_assertions))]
mod add_project_clone_test;
#[cfg(all(test, unix))]
mod add_project_keys_test;
#[cfg(all(test, unix))]
mod add_project_new_test;
#[cfg(all(test, unix))]
mod add_project_rig;
#[cfg(all(test, unix))]
mod add_project_test;
#[cfg(test)]
mod agent_bin_test;
#[cfg(all(test, unix))]
mod browser_agent_cap_test;
#[cfg(all(test, unix))]
mod browser_agent_review_test;
#[cfg(all(test, unix))]
mod browser_agent_test;
#[cfg(all(test, unix))]
mod browser_controls_test;
#[cfg(test)]
mod browser_tests;
#[cfg(test)]
mod changes_model_test;
#[cfg(test)]
mod changes_tests;
#[cfg(all(test, unix))]
mod checkpoint_midturn_test;
#[cfg(test)]
mod checkpoint_purge_test;
#[cfg(all(test, unix))]
mod checkpoint_turns_test;
#[cfg(all(test, unix))]
mod claude_folded_turns_test;
#[cfg(test)]
mod close_tab_test;
#[cfg(all(test, unix))] // the fake codex is a shell script
mod codex_hook_status_test;
#[cfg(all(test, unix))]
mod codex_open_failure_turns_test;
#[cfg(all(test, unix))] // the fake codex is a shell script
mod codex_thread_prefix_test;
#[cfg(all(test, unix))] // the fake codex is a shell script
mod codex_session_gone_test;
#[cfg(all(test, unix))] // the fake codex is a shell script
mod codex_thread_test;
#[cfg(all(test, unix))]
mod commit_hook_env_test;
#[cfg(test)]
mod commit_snapshot_test;
#[cfg(all(test, unix))]
mod compare_base_test;
#[cfg(all(test, unix))]
mod compare_cap_test;
#[cfg(test)]
mod compare_pin_test;
#[cfg(all(test, unix))]
mod compare_test;
#[cfg(test)]
mod delegation_test;
#[cfg(test)]
mod delegation_tests;
#[cfg(all(test, unix))]
mod delegation_window_close_test;
#[cfg(test)]
mod device_agent_test;
#[cfg(test)]
mod devices_review_tests;
#[cfg(test)]
mod devices_tests;
#[cfg(test)]
mod diff_prefs_test;
#[cfg(test)]
mod drive_rebound_test;
#[cfg(test)]
mod env_approval_test;
#[cfg(test)]
mod env_flat_list_test;
#[cfg(test)]
mod env_followups_test;
#[cfg(test)]
mod env_forward_test;
#[cfg(test)]
mod env_harness;
#[cfg(test)]
mod env_identity_gate_test;
#[cfg(test)]
mod env_picker_test;
#[cfg(test)]
mod env_pin_test;
#[cfg(test)]
mod env_shelve_test;
#[cfg(test)]
mod env_prompt_loss_test;
#[cfg(test)]
mod env_queue_test;
#[cfg(test)]
mod env_states_test;
#[cfg(test)]
mod dock_motion_test;
#[cfg(test)]
mod sidebar_drag_test;
#[cfg(test)]
mod env_switch_test;
#[cfg(test)]
mod env_whois_test;
#[cfg(all(test, unix))]
mod exit_holding_stdout_test;
#[cfg(test)]
mod focus_areas_test;
#[cfg(test)]
mod folder_watch_test;
#[cfg(all(test, unix))] // uses publish_test's fake-gh helpers
mod generate_text_test;
#[cfg(test)]
mod image_viewer_test;
#[cfg(all(test, unix))]
mod import_refused_test;
#[cfg(all(test, unix))]
mod import_sessions_test;
#[cfg(test)]
mod layout_test;
#[cfg(test)]
mod left_rail_test;
#[cfg(all(test, unix))] // the fake claude is a shell script
mod login_banner_test;
#[cfg(test)]
mod long_thread_test;
#[cfg(test)]
mod manage_harness;
#[cfg(test)]
mod manage_projects_test;
#[cfg(test)]
mod manage_threads_test;
#[cfg(all(test, unix))] // the fake claude is a shell script
mod quit_guard_test;
#[cfg(all(test, unix))] // the fake claude is a shell script
mod quit_no_window_test;
#[cfg(test)]
mod mermaid_cap_test;
#[cfg(test)]
mod mermaid_href_test;
#[cfg(test)]
mod mermaid_test;
#[cfg(test)]
mod multi_window_test;
#[cfg(all(test, unix))]
mod onboarding_test;
#[cfg(test)]
mod palette_remote_test;
#[cfg(test)]
mod palette_search_test;
#[cfg(test)]
mod palette_gallery_test;
#[cfg(test)]
mod palette_test;
#[cfg(test)]
mod palette_pages_test;
#[cfg(all(test, unix))] // the fake agent is a shell script
mod permission_mode_test;
#[cfg(all(test, unix))] // the fake agent is a shell script
mod permission_thread_test;

#[cfg(all(test, unix))] // the fake agent is a shell script
mod project_rules_flow_test;
#[cfg(test)]
mod project_rules_page_test;
#[cfg(all(test, unix))]
mod pr_reopen_test;
#[cfg(all(test, unix))]
mod project_unread_restart_test;
#[cfg(test)]
mod prompt_handoff_test;
#[cfg(all(test, unix))] // the fake gh is a shell script
mod publish_test;
#[cfg(test)]
mod remote_resume_reply_test;
#[cfg(test)]
mod remote_resume_test;
#[cfg(all(test, unix))] // the fake agent is a shell script
mod remote_start_test;
#[cfg(all(test, unix))] // the fake agent is a shell script
mod review_cta_folder_test;
#[cfg(test)]
mod review_fixes_tests;
#[cfg(test)]
mod review_stale_config_test;
#[cfg(test)]
mod save_retry_stop_test;
#[cfg(test)]
mod save_retry_test;
#[cfg(test)]
mod save_turns_test;
#[cfg(all(test, unix))]
mod saved_view_draft_test;
#[cfg(test)]
mod scratch_test;
#[cfg(all(test, unix))]
mod scripts_env_test;
#[cfg(test)]
mod scripts_palette_test;
#[cfg(test)]
mod settings_modal_test;
#[cfg(test)]
mod sidebar_tests;
#[cfg(all(test, unix))] // the fake agent is a shell script
mod single_view_merge_test;
#[cfg(all(test, unix))]
mod single_view_test;
#[cfg(test)]
mod stale_idle_test;
#[cfg(test)]
mod stale_status_test;
#[cfg(all(test, unix))]
mod terminal_close_test;
#[cfg(all(test, unix))] // the fake agent is a shell script
mod terminal_context_test;
#[cfg(test)]
mod terminal_env_test;
#[cfg(test)]
mod terminal_link_subdir_test;
#[cfg(test)]
mod terminal_link_test;
#[cfg(test)]
mod terminal_owner_test;
#[cfg(all(test, unix))]
mod login_terminal_test;
#[cfg(all(test, unix))]
mod terminal_restore_test;
#[cfg(test)]
mod terminal_split_test;
#[cfg(all(test, unix))]
mod click_focus_test;
#[cfg(all(test, unix))]
mod terminal_title_test;
#[cfg(test)]
mod tests;
#[cfg(all(test, unix))]
mod thread_close_starting_test;
#[cfg(test)]
mod thread_keys_test;
#[cfg(test)]
mod thread_model_test;
#[cfg(test)]
mod threads_test;
#[cfg(test)]
mod timeline_tests;
#[cfg(test)]
mod turn_result_wait_test;
#[cfg(all(test, unix))] // the fake agent is a shell script
mod unread_test;
#[cfg(test)]
mod window_restore_test;
#[cfg(test)]
mod timeline_follow_test;
#[cfg(test)]
mod remote_fold_offset_test;
#[cfg(test)]
mod tool_summary_test;
#[cfg(test)]
mod timeline_scroll_jump_test;
#[cfg(test)]
mod window_scroll_test;
#[cfg(all(test, unix))]
mod worktree_base_test;
#[cfg(test)]
mod worktree_discard_test;
#[cfg(test)]
mod worktree_followups_test;
#[cfg(test)]
mod worktree_guard_test;
#[cfg(test)]
mod worktree_review_test;
#[cfg(test)]
mod worktree_thread_test;
#[cfg(all(test, unix))]
mod new_thread_no_finder_test;
#[cfg(all(test, unix))]
mod onboarding_agents_test;
#[cfg(all(test, unix))]
mod onboarding_idle_test;
#[cfg(all(test, unix))]
mod onboarding_import_test;
#[cfg(all(test, unix))]
mod onboarding_steps_test;
#[cfg(all(test, unix))]
mod onboarding_theme_test;
#[cfg(all(test, unix))]
mod onboarding_import_back_test;
#[cfg(all(test, unix))]
mod add_project_overlap_test;
#[cfg(all(test, unix))]
mod mcp_sign_in_test;

#[cfg(all(test, unix))] // the fake antigravity is a shell script
mod antigravity_thread_test;
#[cfg(all(test, unix))] // the fake cursor is a shell script
mod cursor_thread_test;
