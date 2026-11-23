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
