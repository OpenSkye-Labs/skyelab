//! Shared by the migration tests that rebuild an older database.

/// Undoes migration 11: the search trigger indexes prompts only again, as version 7 made it.
pub const UNDO_V11: &str = "
DROP TRIGGER event_text_insert;
CREATE TRIGGER event_text_insert AFTER INSERT ON events BEGIN
    INSERT INTO event_text (body, thread_id)
    SELECT body, NEW.thread_id FROM (
        SELECT CASE WHEN NEW.kind = 'prompt' AND json_valid(NEW.payload_json)
                         AND json_type(NEW.payload_json) = 'text'
                    THEN json_extract(NEW.payload_json, '$') END AS body
        UNION ALL
        SELECT json_extract(value, '$.text') FROM json_each(
            CASE WHEN NEW.kind = 'agent' AND json_valid(NEW.payload_json) THEN NEW.payload_json ELSE '{}' END,
            '$.Assistant.content')
        WHERE json_extract(value, '$.type') = 'text'
    ) WHERE body IS NOT NULL AND body <> '';
END;
";

/// Undoes migrations 14 and 13: the PR state; titles' source, forks and drafts.
pub const UNDO_V13: &str = "
DROP TRIGGER drafts_thread_delete; DROP TABLE drafts; DROP TABLE draft_files;
ALTER TABLE threads DROP COLUMN title_source; ALTER TABLE threads DROP COLUMN forked_from;
ALTER TABLE threads DROP COLUMN fork_context;
";

/// Undoes migration 23: comparison groups.
pub const UNDO_V23: &str = "ALTER TABLE threads DROP COLUMN limit_resume_at; ALTER TABLE threads DROP COLUMN scroll_mark; DROP INDEX threads_by_compare_group; ALTER TABLE threads DROP COLUMN compare_group;";
/// Undoes migration 22: settled and snoozed threads.
pub const UNDO_V22: &str =
    "ALTER TABLE threads DROP COLUMN settled_at; ALTER TABLE threads DROP COLUMN snoozed_until;";
/// Undoes migration 21: scheduled tasks.
pub const UNDO_V21: &str = "DROP TABLE bot_runs; DROP TABLE bot_seen; DROP TABLE scheduled_tasks; DROP INDEX threads_scheduled_task; ALTER TABLE threads DROP COLUMN scheduled_task_id;";
pub const UNDO_V19: &str =
    "DROP TRIGGER side_chats_delete; ALTER TABLE threads DROP COLUMN side_of;";
/// Undoes migration 16: the turn usage table.
pub const UNDO_V16: &str = "DROP TRIGGER turn_usage_insert; DROP TABLE turn_usage;";
pub const UNDO_V15: &str = "ALTER TABLE threads DROP COLUMN pr_state;";
/// Undoes migration 14: the imported session column.
pub const UNDO_V14: &str = "ALTER TABLE threads DROP COLUMN imported_session;";
