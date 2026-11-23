//! Embedded terminal: a PTY-backed model on `alacritty_terminal` and a GPUI view that draws it.

mod keys;
mod links;
mod model;
mod mouse;
mod paint;
mod probe;
mod view;

pub use alacritty_terminal::{grid::Scroll, index::Side, selection::SelectionType};
pub use keys::{OptionKey, OptionKeys, key_to_bytes};
pub use links::{Found, LinkKind, find_links, path_target};
pub use model::{Event, Link, Listener, SpawnOptions, Terminal};
pub use probe::{Foreground, Probe};
pub use mouse::{MouseAction, MouseButton, MouseMods, mouse_report};
pub use paint::AnsiColors;
pub use view::{AddToPrompt, Copy, Paste, TerminalEvent, TerminalView, init};
