//! Embedded terminal: a PTY-backed model on `alacritty_terminal` and a GPUI view that draws it.

mod keys;
mod model;
mod view;

pub use keys::{OptionKey, key_to_bytes};
pub use model::{Event, Listener, SpawnOptions, Terminal};
pub use view::{Paste, TerminalView, init};
