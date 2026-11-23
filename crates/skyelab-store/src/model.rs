//! The composer's model pick: per thread, and each project's default for new threads.

use crate::Store;
use anyhow::{Result, bail};
use rusqlite::{OptionalExtension as _, params};

/// A model and effort the composer picked. `None` leaves it to the agent's own default.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ModelChoice {
    pub model: Option<String>,
    pub effort: Option<String>,
}

impl Store {
    /// The model and effort picked for thread `id` (both `None` when never picked).
    pub fn thread_model(&self, id: i64) -> Result<ModelChoice> {
        self.model_of("SELECT model, effort FROM threads WHERE id = ?1", id)
    }

    /// Stores thread `id`'s pick. Not a thread update the sidebar orders by, so `updated_at` stays.
    pub fn set_thread_model(&self, id: i64, choice: &ModelChoice) -> Result<()> {
        self.set_model_of(
            "UPDATE threads SET model = ?2, effort = ?3 WHERE id = ?1",
            id,
            choice,
        )
    }

    /// The model and effort new threads of project `id` start with.
    pub fn project_model(&self, id: i64) -> Result<ModelChoice> {
        self.model_of(
            "SELECT default_model, default_effort FROM projects WHERE id = ?1",
            id,
        )
    }

    pub fn set_project_model(&self, id: i64, choice: &ModelChoice) -> Result<()> {
        self.set_model_of(
            "UPDATE projects SET default_model = ?2, default_effort = ?3 WHERE id = ?1",
            id,
            choice,
        )
    }

    fn model_of(&self, sql: &str, id: i64) -> Result<ModelChoice> {
        let row = self
            .conn
            .query_row(sql, [id], |r| {
                Ok(ModelChoice {
                    model: r.get(0)?,
                    effort: r.get(1)?,
                })
            })
            .optional()?;
        Ok(row.unwrap_or_default())
    }

    fn set_model_of(&self, sql: &str, id: i64, choice: &ModelChoice) -> Result<()> {
        let n = self
            .conn
            .execute(sql, params![id, choice.model, choice.effort])?;
        if n == 0 {
            bail!("no row {id} to store a model on");
        }
        Ok(())
    }
}
