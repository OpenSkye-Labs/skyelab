//! A subagent's card in the timeline. The model (status, nested calls) is shared with the phone in
//! `skyelab_agent::timeline::subagent`; this file adds the card.

pub use skyelab_agent::timeline::subagent::*;

#[path = "subagent_card.rs"]
pub(super) mod card;
pub use card::render;
