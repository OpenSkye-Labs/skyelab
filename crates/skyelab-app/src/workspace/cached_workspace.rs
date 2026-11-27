//! The window's workspace view, cached, and kept whole in the accessibility tree.

use gpui::{IntoElement, ParentElement as _, Styled as _, Window, div};

/// Root's child: the workspace as a cached view. A dialog in Root (the command palette) re-renders Root on every
/// keystroke and hover, which would re-render the workspace too; cached, it renders only on its own notify or a
/// window refresh (docs/gpui/redraw.md). The bool flips every frame while an accessibility client is attached.
pub(super) struct CachedWorkspace(pub(super) gpui::AnyView, pub(super) bool);

/// The largest f32 below 1: times any 8-bit channel it still rounds to that channel.
const NEXT_BELOW_ONE: f32 = 1.0 - f32::EPSILON / 2.0;

impl gpui::Render for CachedWorkspace {
    fn render(&mut self, window: &mut Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
        let workspace = crate::ui::cached_view(
            self.0.clone(),
            gpui::StyleRefinement::default().size_full(),
            window,
        );
        if !window.is_a11y_active() {
            return workspace;
        }
        // GPUI 0.3.14 doesn't re-emit a reused cached view's a11y nodes, and gpui-component's dock caches every
        // panel, so a frame that redraws only some views would send a tree missing the rest. A cached view is reused
        // only while the opacity it is drawn at stays the same, so every other frame draws the workspace at the
        // next opacity below 1: no cache under it matches, and no pixel changes.
        self.1 = !self.1;
        div()
            .size_full()
            .opacity(if self.1 { NEXT_BELOW_ONE } else { 1.0 })
            .child(workspace)
            .into_any_element()
    }
}
