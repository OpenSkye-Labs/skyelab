//! Continue a thread under the other provider.

use gpui::{Action, actions};
use skyelab_core::Provider;

actions!(skyelab, [ContinueInCodex, ContinueInClaude]);

#[derive(Clone, Debug, PartialEq, Action)]
#[action(namespace = skyelab, no_json)]
pub struct HandOffThread {
    pub id: i64,
    pub to: Provider,
}
