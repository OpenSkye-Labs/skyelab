//! A portable handoff: a thread's history condensed for another provider's first session.

use crate::timeline::Record;
use serde_json::Value;

pub const BUDGET: usize = 16_000;

#[derive(Debug, Clone, PartialEq)]
pub struct Item {
    pub role: &'static str,
    pub kind: &'static str,
    pub text: String,
}

pub fn items(_records: &[Record]) -> Vec<Item> {
    Vec::new()
}

fn line(_i: &Item) -> String {
    String::new()
}

pub fn prompt(_title: &str, _from: &str, _to: &str, _items: &[Item], _budget: usize) -> String {
    let _ = Value::Null;
    String::new()
}

#[cfg(test)]
#[path = "handoff_tests.rs"]
mod tests;
