//! The schema, as an append-only list of upgrade steps.

/// Index `i` upgrades the schema from `user_version` `i` to `i + 1`. Only append.
pub(crate) const MIGRATIONS: &[&str] = &[
    "
CREATE TABLE projects (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    name       TEXT NOT NULL,
    path       TEXT NOT NULL UNIQUE,
    created_at INTEGER NOT NULL
);
CREATE TABLE threads (
    id               INTEGER PRIMARY KEY AUTOINCREMENT,
    project_id       INTEGER NOT NULL REFERENCES projects(id) ON DELETE RESTRICT,
    title            TEXT NOT NULL,
    cwd              TEXT NOT NULL,
    account_id       TEXT NOT NULL,
    agent_session_id TEXT,
    status           TEXT NOT NULL,
    created_at       INTEGER NOT NULL,
    updated_at       INTEGER NOT NULL
);
CREATE INDEX threads_by_project ON threads(project_id, updated_at DESC);
CREATE TABLE events (
    thread_id    INTEGER NOT NULL REFERENCES threads(id) ON DELETE CASCADE,
    seq          INTEGER NOT NULL,
    kind         TEXT NOT NULL,
    payload_json TEXT NOT NULL,
    created_at   INTEGER NOT NULL,
    PRIMARY KEY (thread_id, seq)
) WITHOUT ROWID;
",
    "
ALTER TABLE threads ADD COLUMN worktree_path TEXT;
ALTER TABLE threads ADD COLUMN worktree_branch TEXT;
",
    "
ALTER TABLE threads ADD COLUMN worktree_base TEXT;
",
    "ALTER TABLE threads ADD COLUMN pr_url TEXT;",
    "
ALTER TABLE threads ADD COLUMN parent_thread_id INTEGER REFERENCES threads(id) ON DELETE SET NULL;
CREATE INDEX threads_by_parent ON threads(parent_thread_id);
",
    // Version 6: per-thread permission mode (agent permissions).
    "ALTER TABLE threads ADD COLUMN permission_mode TEXT NOT NULL DEFAULT 'default';",
    // Version 7: thread and project management (pin, archive, unread, undoable delete, project removal) and message
    // search. `event_text` holds only message text: prompt strings and the assistant's `text` blocks, pulled out by a
    // trigger so `append_event` stays opaque to callers. Trigram gives substring matches, like the title filter.
    "
ALTER TABLE threads ADD COLUMN pinned_at INTEGER;
ALTER TABLE threads ADD COLUMN archived_at INTEGER;
ALTER TABLE threads ADD COLUMN deleted_at INTEGER;
ALTER TABLE threads ADD COLUMN unread INTEGER NOT NULL DEFAULT 0;
ALTER TABLE projects ADD COLUMN removed_at INTEGER;
CREATE VIRTUAL TABLE event_text USING fts5(body, thread_id UNINDEXED, tokenize = 'trigram');
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
CREATE TRIGGER event_text_thread_delete AFTER DELETE ON threads BEGIN
    DELETE FROM event_text WHERE thread_id = OLD.id;
END;
INSERT INTO event_text (body, thread_id)
    SELECT json_extract(payload_json, '$'), thread_id FROM events
    WHERE kind = 'prompt' AND json_valid(payload_json) AND json_type(payload_json) = 'text';
INSERT INTO event_text (body, thread_id)
    SELECT json_extract(b.value, '$.text'), e.thread_id
    FROM events e, json_each(e.payload_json, '$.Assistant.content') b
    WHERE e.kind = 'agent' AND json_valid(e.payload_json) AND json_extract(b.value, '$.type') = 'text'
      AND json_extract(b.value, '$.text') <> '';
",
    // Version 8: per-project scripts (Run menu, setup in new worktrees). Needs only `projects`.
    "
CREATE TABLE project_scripts (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    project_id INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    name       TEXT NOT NULL,
    command    TEXT NOT NULL,
    setup      INTEGER NOT NULL DEFAULT 0,
    created_at INTEGER NOT NULL
);
CREATE INDEX project_scripts_by_project ON project_scripts(project_id, id);
",
    // Version 9: the composer's model and effort per thread, and each project's default for new threads.
    "
ALTER TABLE threads ADD COLUMN model TEXT;
ALTER TABLE threads ADD COLUMN effort TEXT;
ALTER TABLE projects ADD COLUMN default_model TEXT;
ALTER TABLE projects ADD COLUMN default_effort TEXT;
",
    // Version 10: each account's newest usage reading, kept as agent events arrive, so reading usage doesn't scan
    // events. Window '' is the newest `RateLimit` of any kind (whether it says limited); a named window is the newest
    // one that carries a percentage. `instr` is a cheap prefilter before the JSON is parsed.
    "
CREATE TABLE usage_readings (
    account_id   TEXT NOT NULL,
    window       TEXT NOT NULL,
    payload_json TEXT NOT NULL,
    created_at   INTEGER NOT NULL,
    PRIMARY KEY (account_id, window)
) WITHOUT ROWID;
CREATE TRIGGER usage_reading_insert AFTER INSERT ON events
WHEN NEW.kind = 'agent' AND instr(NEW.payload_json, '\"RateLimit\"') > 0 AND json_valid(NEW.payload_json)
     AND json_type(NEW.payload_json, '$.RateLimit') = 'object'
BEGIN
    INSERT OR REPLACE INTO usage_readings (account_id, window, payload_json, created_at)
        SELECT t.account_id, '', NEW.payload_json, NEW.created_at FROM threads t WHERE t.id = NEW.thread_id;
    INSERT OR REPLACE INTO usage_readings (account_id, window, payload_json, created_at)
        SELECT t.account_id, json_extract(NEW.payload_json, '$.RateLimit.window'), NEW.payload_json, NEW.created_at
        FROM threads t WHERE t.id = NEW.thread_id
          AND json_type(NEW.payload_json, '$.RateLimit.window') = 'text'
          AND json_type(NEW.payload_json, '$.RateLimit.used') IN ('integer', 'real');
END;
INSERT OR REPLACE INTO usage_readings (account_id, window, payload_json, created_at)
    SELECT t.account_id, COALESCE(w.window, json_extract(e.payload_json, '$.RateLimit.window')),
           e.payload_json, e.created_at
    FROM events e JOIN threads t ON t.id = e.thread_id,
         (SELECT '' AS window UNION ALL SELECT NULL) w
    WHERE e.kind = 'agent' AND instr(e.payload_json, '\"RateLimit\"') > 0 AND json_valid(e.payload_json)
      AND json_type(e.payload_json, '$.RateLimit') = 'object'
      AND (w.window = '' OR (json_type(e.payload_json, '$.RateLimit.window') = 'text'
                             AND json_type(e.payload_json, '$.RateLimit.used') IN ('integer', 'real')))
    ORDER BY e.created_at, e.thread_id, e.seq;
",
    // Version 11: a message steered into a running turn is stored as `steer` (its own kind, so the turn's fold can
    // reach past it); search indexes its text like a prompt's.
    "
DROP TRIGGER event_text_insert;
CREATE TRIGGER event_text_insert AFTER INSERT ON events BEGIN
    INSERT INTO event_text (body, thread_id)
    SELECT body, NEW.thread_id FROM (
        SELECT CASE WHEN NEW.kind IN ('prompt', 'steer') AND json_valid(NEW.payload_json)
                         AND json_type(NEW.payload_json) = 'text'
                    THEN json_extract(NEW.payload_json, '$') END AS body
        UNION ALL
        SELECT json_extract(value, '$.text') FROM json_each(
            CASE WHEN NEW.kind = 'agent' AND json_valid(NEW.payload_json) THEN NEW.payload_json ELSE '{}' END,
            '$.Assistant.content')
        WHERE json_extract(value, '$.type') = 'text'
    ) WHERE body IS NOT NULL AND body <> '';
END;
",
    // Version 12: image and PDF bytes are no longer stored. The rewrite of older rows is Rust (`images.rs`), run by
    // `Store::init` when it upgrades past 11; this step itself changes no schema.
    "",
];
