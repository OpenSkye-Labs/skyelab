//! The rich timeline's tests (streaming, cards, questions, limits), kept out of `mod.rs`. Each test file names
//! workspace items through `super::`, which this glob makes resolve as before.

use super::*;

#[path = "timeline_model_test.rs"]
mod timeline_model_test;

#[cfg(unix)] // builds its thread on the fake agent, a shell script
#[path = "tool_cards_test.rs"]
mod tool_cards_test;

#[path = "timeline_fold_test.rs"]
mod timeline_fold_test;

#[path = "timeline_fold_stopped_test.rs"]
mod timeline_fold_stopped_test;

#[path = "image_placeholder_test.rs"]
mod image_placeholder_test;

#[path = "card_path_test.rs"]
mod card_path_test;

#[path = "review_proofs_test.rs"]
mod review_proofs_test;

#[cfg(unix)] // the fake agent is a shell script
#[path = "ask_question_e2e_test.rs"]
mod ask_question_e2e_test;

#[cfg(unix)] // the fake agent is a shell script
#[path = "delta_store_test.rs"]
mod delta_store_test;

#[cfg(unix)] // the fake agent is a shell script
#[path = "rate_limit_state_test.rs"]
mod rate_limit_state_test;

#[cfg(unix)] // the fake agent is a shell script
#[path = "streaming_queue_test.rs"]
mod streaming_queue_test;

#[path = "steer_fold_test.rs"]
mod steer_fold_test;

#[cfg(unix)] // the fake agent is a shell script
#[path = "partial_text_test.rs"]
mod partial_text_test;

#[cfg(unix)] // the fake agent is a shell script
#[path = "remote_midturn_fold_test.rs"]
mod remote_midturn_fold_test;

#[path = "subagent_test.rs"]
mod subagent_test;

#[cfg(unix)] // the fake agent is a shell script
#[path = "turn_files_test.rs"]
mod turn_files_test;

#[cfg(unix)] // the fake agent is a shell script
#[path = "failure_banner_test.rs"]
mod failure_banner_test;

#[path = "thread_find_test.rs"]
mod thread_find_test;

#[path = "claude_interrupt_banner_test.rs"]
mod claude_interrupt_banner_test;
