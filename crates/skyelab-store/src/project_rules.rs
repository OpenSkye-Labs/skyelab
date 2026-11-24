//! "Allow for this project" rules (`skyelab_core::project_rules`), kept per project and per account: a rule saved
//! under one account is never listed or used for another.

use crate::{Store, expect_one, now_ms};
use anyhow::{Context as _, Result};
use rusqlite::params;
use skyelab_core::project_rules::{Kind, Rule};

/// Version 26. `IF NOT EXISTS`: migration tests rewind `user_version` without dropping later tables.
pub(crate) const MIGRATION_V26: &str = "
CREATE TABLE IF NOT EXISTS project_rules (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    project_id INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    account_id TEXT NOT NULL,
    tool       TEXT NOT NULL,
    kind       TEXT NOT NULL,
    pattern    TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    UNIQUE (project_id, account_id, tool, kind, pattern)
);
";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectRule {
    pub id: i64,
    pub rule: Rule,
}

impl Store {
    /// `account`'s rules in project `project_id`, oldest first. A row of a kind this build doesn't know is skipped.
    pub fn project_rules(&self, project_id: i64, account: &str) -> Result<Vec<ProjectRule>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, tool, kind, pattern FROM project_rules WHERE project_id = ?1 AND account_id = ?2 ORDER BY id",
        )?;
        let rows = stmt.query_map(params![project_id, account], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get(1)?,
                r.get::<_, String>(2)?,
                r.get(3)?,
            ))
        })?;
        let mut out = Vec::new();
        for row in rows {
            let (id, tool, kind, pattern) = row?;
            if let Some(kind) = Kind::parse(&kind) {
                out.push(ProjectRule {
                    id,
                    rule: Rule {
                        tool,
                        kind,
                        pattern,
                    },
                });
            }
        }
        Ok(out)
    }

    /// Saves `rule` for `account` in the project; saving one that is already there keeps the one row.
    pub fn add_project_rule(&self, project_id: i64, account: &str, rule: &Rule) -> Result<()> {
        self.conn
            .execute(
                "INSERT OR IGNORE INTO project_rules (project_id, account_id, tool, kind, pattern, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![project_id, account, rule.tool, rule.kind.as_str(), rule.pattern, now_ms()],
            )
            .with_context(|| format!("save a permission rule in project {project_id}"))?;
        Ok(())
    }

    /// Changes rule `id`'s pattern, only while it belongs to `account`.
    pub fn set_project_rule_pattern(&self, id: i64, account: &str, pattern: &str) -> Result<()> {
        let n = self
            .conn
            .execute(
                "UPDATE project_rules SET pattern = ?3 WHERE id = ?1 AND account_id = ?2",
                params![id, account, pattern],
            )
            .context("edit a permission rule (is the same rule already there?)")?;
        expect_one(n, "permission rule", id)
    }

    /// Removes rule `id`, only while it belongs to `account`.
    pub fn delete_project_rule(&self, id: i64, account: &str) -> Result<()> {
        let n = self.conn.execute(
            "DELETE FROM project_rules WHERE id = ?1 AND account_id = ?2",
            params![id, account],
        )?;
        expect_one(n, "permission rule", id)
    }
}
