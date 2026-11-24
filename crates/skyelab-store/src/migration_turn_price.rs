//! Version 24: a result that prices its own turn (`turn_cost_usd`, Claude's `model_usage_since_last_result`) is
//! stored at that price. Claude's running total can drop on a resume (after Restart Agent Session it left out the
//! killed process's last turn), which the running-total rule reads as a fresh session and counts whole. Rows stored
//! before carry no price and keep their cost.

pub(crate) const MIGRATION_V24: &str = concat!(
    "
DROP TRIGGER turn_usage_insert;
CREATE TRIGGER turn_usage_insert AFTER INSERT ON events
WHEN NEW.kind = 'agent' AND instr(NEW.payload_json, '\"Result\"') > 0 AND json_valid(NEW.payload_json)
     AND json_type(NEW.payload_json, '$.Result') = 'object'
BEGIN
    INSERT OR REPLACE INTO turn_usage
        SELECT NEW.thread_id, NEW.seq, t.account_id, t.project_id,
               COALESCE(json_extract(NEW.payload_json, '$.Result.turn_cost_usd'), ",
    turn_cost!("NEW", fork_copy!()),
    "),
               COALESCE(json_extract(NEW.payload_json, '$.Result.usage.input'), 0),
               COALESCE(json_extract(NEW.payload_json, '$.Result.usage.output'), 0),
               NEW.created_at
        FROM threads t WHERE t.id = NEW.thread_id
          AND ((t.imported_session IS NULL AND t.forked_from IS NULL) OR NEW.created_at > t.created_at);
END;
"
);
