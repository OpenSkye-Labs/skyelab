//! A stored thread event, as the store keeps it: `(kind, payload_json)`.

use crate::Event;
use anyhow::{Context as _, Result};
use serde_json::Value;

/// One stored thread event. The store's `kind` column says which.
#[derive(Debug, Clone, PartialEq)]
pub enum Record {
    Prompt(String),
    /// A message steered into the running turn: part of that turn, not the start of a new one.
    Steer(String),
    Agent(Event),
}

const PROMPT: &str = "prompt";
const STEER: &str = "steer";
const AGENT: &str = "agent";

impl Record {
    /// The store row: `(kind, payload_json)`.
    pub fn to_row(&self) -> (&'static str, String) {
        match self {
            Record::Prompt(text) => (PROMPT, Value::String(text.clone()).to_string()),
            Record::Steer(text) => (STEER, Value::String(text.clone()).to_string()),
            Record::Agent(event) => (
                AGENT,
                serde_json::to_string(event)
                    .expect("an Event holds only strings, numbers and JSON values"),
            ),
        }
    }

    pub fn from_row(kind: &str, payload: &str) -> Result<Record> {
        match kind {
            PROMPT => Ok(Record::Prompt(
                serde_json::from_str(payload).context("stored prompt")?,
            )),
            STEER => Ok(Record::Steer(
                serde_json::from_str(payload).context("stored steer")?,
            )),
            AGENT => Ok(Record::Agent(
                serde_json::from_str(payload).context("stored agent event")?,
            )),
            other => anyhow::bail!("unknown stored event kind '{other}'"),
        }
    }
}
