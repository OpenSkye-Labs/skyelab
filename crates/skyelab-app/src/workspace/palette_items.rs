//! What the command palette lists for this workspace: actions, open tabs, projects and threads.

use super::Workspace;
use crate::palette::Entry;
use gpui::{Context, FocusHandle, Window};

impl Workspace {
    pub(crate) fn palette_entries(
        &self,
        _prev: &FocusHandle,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Vec<Entry> {
        todo!()
    }

    pub(super) fn open_palette(
        &mut self,
        _: &crate::palette::OpenCommandPalette,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) {
        todo!()
    }
}
