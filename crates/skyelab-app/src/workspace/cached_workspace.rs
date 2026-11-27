//! The window's workspace view, cached, and kept whole in the accessibility tree.

/// Root's child: the workspace as a cached view. A dialog in Root (the command palette) re-renders Root on every
/// keystroke and hover, which would re-render the workspace too; cached, it renders only on its own notify or a
/// window refresh (docs/gpui/redraw.md).
/// The bool: a full redraw is already queued for the accessibility tree.
pub(super) struct CachedWorkspace(pub(super) gpui::AnyView, pub(super) bool);

impl gpui::Render for CachedWorkspace {
    fn render(
        &mut self,
        window: &mut gpui::Window,
        cx: &mut gpui::Context<Self>,
    ) -> impl gpui::IntoElement {
        use gpui::Styled as _;
        // GPUI 0.3.14 doesn't re-emit a reused cached view's a11y nodes, and gpui-component's dock caches every panel,
        // so a frame that redraws only some views sends a tree missing the rest. With an accessibility client
        // attached, such a frame is followed by one full redraw (a refresh skips every cache), and that one not again.
        if window.is_a11y_active() {
            if self.1 {
                self.1 = false;
            } else {
                self.1 = true;
                let handle = window.window_handle();
                cx.defer(move |cx| {
                    let _ = handle.update(cx, |_, window, _| window.refresh());
                });
            }
        }
        crate::ui::cached_view(
            self.0.clone(),
            gpui::StyleRefinement::default().size_full(),
            window,
        )
    }
}
