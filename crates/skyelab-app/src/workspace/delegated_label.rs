//! How a thread started by another machine's agent names where it came from: "Delegated by <title> on <machine>".

use super::thread::ThreadView;
use gpui::SharedString;

/// A MagicDNS name's first label: "desk" for desk.tail1.ts.net.
pub(super) fn short_machine(name: &str) -> &str {
    name.split('.').next().unwrap_or(name)
}

/// The label for a parent on another machine. The title is cleaned again here: it came from that machine.
pub(super) fn delegated_by(title: &str, machine: &str) -> SharedString {
    let title = skyelab_core::delegation_remote::clean_title(title);
    format!("Delegated by {title} on {}", short_machine(machine)).into()
}

impl ThreadView {
    /// The header's label for a thread another machine's agent started, `None` for any other thread.
    pub(super) fn delegated_label(&self) -> Option<SharedString> {
        let o = self.remote_parent.as_ref()?;
        Some(delegated_by(&o.parent_title, &o.origin_machine))
    }
}
