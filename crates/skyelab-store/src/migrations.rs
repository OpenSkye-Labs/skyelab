//! The schema, as an append-only list of upgrade steps.

/// `skyelab_agent::turn_cost` in SQL: what a result with the running total `$cur` adds, given `$base`, a query for the
/// thread's baseline (its latest earlier non-zero total, or 0 for an imported turn: unknown). The baseline is looked up
/// only for a non-zero total, so a long Codex thread (all zero) isn't scanned per result.
macro_rules! cost_rule {
    ([$($cur:tt)*], [$($base:tt)*]) => {
        concat!(
            "(CASE WHEN ", $($cur)*, " <= 0 THEN 0 ELSE COALESCE((SELECT CASE WHEN b.v = 0 THEN 0 WHEN ", $($cur)*,
            " < b.v THEN ", $($cur)*, " ELSE ", $($cur)*, " - b.v END FROM (", $($base)*, ") b), ", $($cur)*, ") END)"
        )
    };
}

/// The cost of the stored `Result` event `$e` (a row of `events`): [`cost_rule`] over the thread's earlier results.
/// An imported turn (`skyelab_agent::import`: no cost, duration or usage) ends the search as an unknown baseline.
macro_rules! turn_cost {
    ($e:literal) => {
        turn_cost!($e, "")
    };
    // `$skip`: more conditions on `p`, the earlier results searched for the baseline.
    ($e:literal, $skip:expr) => {
        cost_rule!(
            [concat!("COALESCE(json_extract(", $e, ".payload_json, '$.Result.total_cost_usd'), 0)")],
            [concat!(
                "SELECT COALESCE(json_extract(p.payload_json, '$.Result.total_cost_usd'), 0) AS v FROM events p
                 WHERE p.thread_id = ", $e, ".thread_id AND p.seq < ", $e, ".seq AND p.kind = 'agent'
                   AND instr(p.payload_json, '\"Result\"') > 0 AND json_valid(p.payload_json)
                   AND json_type(p.payload_json, '$.Result') = 'object'
                   AND (json_extract(p.payload_json, '$.Result.total_cost_usd') > 0
                        OR (COALESCE(json_extract(p.payload_json, '$.Result.total_cost_usd'), 0) = 0
                            AND json_extract(p.payload_json, '$.Result.duration_ms') = 0
                            AND json_extract(p.payload_json, '$.Result.is_error') = 0
                            AND COALESCE(json_type(p.payload_json, '$.Result.usage'), 'null') = 'null'))",
                $skip,
                "
                 ORDER BY p.seq DESC LIMIT 1"
            )]
        )
    };
}

/// [`turn_cost`]'s extra condition from version 20: a result copied into a fork (dated before the fork was made) is no
/// baseline for the fork's turns.
macro_rules! fork_copy {
    () => {
        "
                   AND NOT EXISTS (SELECT 1 FROM threads f WHERE f.id = p.thread_id AND f.forked_from IS NOT NULL
                                   AND p.created_at <= f.created_at)"
    };
}

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
    // Version 13: generated titles (who set the title: 'prompt', 'generated', 'user' or 'fork'); forks (the source
    // thread, kept without a foreign key so an unsent fork stays sendable after its source is deleted, and the context
    // prompt its first session starts with); composer drafts per thread or per project's "New thread" tab (`owner` is
    // `thread:<id>` or `project:<id>`).
    "
ALTER TABLE threads ADD COLUMN title_source TEXT NOT NULL DEFAULT 'prompt';
ALTER TABLE threads ADD COLUMN forked_from INTEGER;
ALTER TABLE threads ADD COLUMN fork_context TEXT;
CREATE TABLE drafts (
    owner      TEXT PRIMARY KEY,
    text       TEXT NOT NULL,
    updated_at INTEGER NOT NULL
) WITHOUT ROWID;
CREATE TABLE draft_files (
    owner TEXT NOT NULL,
    idx   INTEGER NOT NULL,
    name  TEXT NOT NULL,
    data  BLOB NOT NULL,
    PRIMARY KEY (owner, idx)
) WITHOUT ROWID;
CREATE TRIGGER drafts_thread_delete AFTER DELETE ON threads BEGIN
    DELETE FROM drafts WHERE owner = 'thread:' || OLD.id;
    DELETE FROM draft_files WHERE owner = 'thread:' || OLD.id;
END;
",
    // Version 14: the agent session a thread was imported from. Its live session id can change on resume, so the
    // import picker and `import_thread` also match on this.
    "
ALTER TABLE threads ADD COLUMN imported_session TEXT;
",
    // Version 15: the linked pull request's last read state ('open', 'draft', 'merged', 'closed'), so an ended PR
    // isn't read again after a restart.
    "ALTER TABLE threads ADD COLUMN pr_state TEXT;",
    // Version 16: each turn's cost and tokens (a stored `Result` event), with the account and project of its thread
    // when it ran, for the Usage page. No foreign key: spend outlives a purged thread. History an imported thread
    // (dated at its import) or a fork (copied with its old times) starts with is not new spend, so it is left out;
    // turns after the thread was made count.
    "
CREATE TABLE turn_usage (
    thread_id     INTEGER NOT NULL,
    seq           INTEGER NOT NULL,
    account_id    TEXT NOT NULL,
    project_id    INTEGER NOT NULL,
    cost_usd      REAL NOT NULL,
    input_tokens  INTEGER NOT NULL,
    output_tokens INTEGER NOT NULL,
    created_at    INTEGER NOT NULL,
    PRIMARY KEY (thread_id, seq)
) WITHOUT ROWID;
CREATE INDEX turn_usage_by_time ON turn_usage(created_at);
CREATE TRIGGER turn_usage_insert AFTER INSERT ON events
WHEN NEW.kind = 'agent' AND instr(NEW.payload_json, '\"Result\"') > 0 AND json_valid(NEW.payload_json)
     AND json_type(NEW.payload_json, '$.Result') = 'object'
BEGIN
    INSERT OR REPLACE INTO turn_usage
        SELECT NEW.thread_id, NEW.seq, t.account_id, t.project_id,
               COALESCE(json_extract(NEW.payload_json, '$.Result.total_cost_usd'), 0),
               COALESCE(json_extract(NEW.payload_json, '$.Result.usage.input'), 0),
               COALESCE(json_extract(NEW.payload_json, '$.Result.usage.output'), 0),
               NEW.created_at
        FROM threads t WHERE t.id = NEW.thread_id
          AND ((t.imported_session IS NULL AND t.forked_from IS NULL) OR NEW.created_at > t.created_at);
END;
INSERT OR REPLACE INTO turn_usage
    SELECT e.thread_id, e.seq, t.account_id, t.project_id,
           COALESCE(json_extract(e.payload_json, '$.Result.total_cost_usd'), 0),
           COALESCE(json_extract(e.payload_json, '$.Result.usage.input'), 0),
           COALESCE(json_extract(e.payload_json, '$.Result.usage.output'), 0),
           e.created_at
    FROM events e JOIN threads t ON t.id = e.thread_id
    WHERE e.kind = 'agent' AND instr(e.payload_json, '\"Result\"') > 0 AND json_valid(e.payload_json)
      AND json_type(e.payload_json, '$.Result') = 'object'
      AND ((t.imported_session IS NULL AND t.forked_from IS NULL) OR e.created_at > t.created_at);
",
    // Version 17: a turn's cost is what it added to Claude's running `total_cost_usd` ([`turn_cost`]), in the trigger
    // and in the rows already stored.
    concat!(
        "
DROP TRIGGER turn_usage_insert;
CREATE TRIGGER turn_usage_insert AFTER INSERT ON events
WHEN NEW.kind = 'agent' AND instr(NEW.payload_json, '\"Result\"') > 0 AND json_valid(NEW.payload_json)
     AND json_type(NEW.payload_json, '$.Result') = 'object'
BEGIN
    INSERT OR REPLACE INTO turn_usage
        SELECT NEW.thread_id, NEW.seq, t.account_id, t.project_id, ",
        turn_cost!("NEW"),
        ",
               COALESCE(json_extract(NEW.payload_json, '$.Result.usage.input'), 0),
               COALESCE(json_extract(NEW.payload_json, '$.Result.usage.output'), 0),
               NEW.created_at
        FROM threads t WHERE t.id = NEW.thread_id
          AND ((t.imported_session IS NULL AND t.forked_from IS NULL) OR NEW.created_at > t.created_at);
END;
CREATE TEMP TABLE turn_raw AS SELECT thread_id, seq, cost_usd AS v FROM turn_usage;
UPDATE turn_usage SET cost_usd = COALESCE((SELECT ",
        turn_cost!("e"),
        " FROM events e WHERE e.thread_id = turn_usage.thread_id AND e.seq = turn_usage.seq),
    -- A purged thread's events are gone: its stored totals are the running totals to diff.
    ",
        cost_rule!(
            ["(SELECT v FROM turn_raw r WHERE r.thread_id = turn_usage.thread_id AND r.seq = turn_usage.seq)"],
            ["SELECT v FROM turn_raw r WHERE r.thread_id = turn_usage.thread_id AND r.seq < turn_usage.seq AND r.v > 0
              ORDER BY r.seq DESC LIMIT 1"]
        ),
        ");
DROP TABLE turn_raw;
"
    ),
    // Version 18: imported history wrongly counted as turns (an import dated the thread at its first event, so every
    // later imported turn passed the trigger's date check). Such a turn is the importer's empty result; a live turn
    // with no time, tokens or text added nothing either.
    "
DELETE FROM turn_usage WHERE (thread_id, seq) IN (
    SELECT e.thread_id, e.seq FROM events e JOIN threads t ON t.id = e.thread_id
    WHERE t.imported_session IS NOT NULL AND e.kind = 'agent' AND json_valid(e.payload_json)
      AND json_type(e.payload_json, '$.Result') = 'object'
      AND json_extract(e.payload_json, '$.Result.is_error') = 0
      AND json_extract(e.payload_json, '$.Result.duration_ms') = 0
      AND COALESCE(json_type(e.payload_json, '$.Result.usage'), 'null') = 'null'
      AND COALESCE(json_type(e.payload_json, '$.Result.result'), 'null') = 'null'
);
",
    // Version 19: side chats (T3's `/side`), each a thread with `side_of` set to its parent. They go when the parent is
    // deleted (or purged): their first prompt holds a copy of its conversation.
    "
ALTER TABLE threads ADD COLUMN side_of INTEGER;
CREATE TRIGGER side_chats_delete AFTER DELETE ON threads BEGIN
    DELETE FROM threads WHERE side_of = OLD.id;
END;
",
    // Version 20: a fork's turns never take its copied history (dated before the fork) as their baseline. Its first
    // session is fresh, so the running total in its first result is all that turn's own. Forks' stored turns are
    // counted again.
    concat!(
        "
DROP TRIGGER turn_usage_insert;
CREATE TRIGGER turn_usage_insert AFTER INSERT ON events
WHEN NEW.kind = 'agent' AND instr(NEW.payload_json, '\"Result\"') > 0 AND json_valid(NEW.payload_json)
     AND json_type(NEW.payload_json, '$.Result') = 'object'
BEGIN
    INSERT OR REPLACE INTO turn_usage
        SELECT NEW.thread_id, NEW.seq, t.account_id, t.project_id, ",
        turn_cost!("NEW", fork_copy!()),
        ",
               COALESCE(json_extract(NEW.payload_json, '$.Result.usage.input'), 0),
               COALESCE(json_extract(NEW.payload_json, '$.Result.usage.output'), 0),
               NEW.created_at
        FROM threads t WHERE t.id = NEW.thread_id
          AND ((t.imported_session IS NULL AND t.forked_from IS NULL) OR NEW.created_at > t.created_at);
END;
UPDATE turn_usage SET cost_usd = (SELECT ",
        turn_cost!("e", fork_copy!()),
        " FROM events e WHERE e.thread_id = turn_usage.thread_id AND e.seq = turn_usage.seq)
WHERE thread_id IN (SELECT id FROM threads WHERE forked_from IS NOT NULL)
  AND EXISTS (SELECT 1 FROM events e WHERE e.thread_id = turn_usage.thread_id AND e.seq = turn_usage.seq);
"
    ),
];
