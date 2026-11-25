//! T3's tooltip (`ui/tooltip.tsx`): radius 6, hairline border, popover fill, 12 px text, `px-2 py-1`, and the key that
//! runs the same action when there is one. GPUI draws a tooltip as a view, so this is the one component that is an
//! entity: built only when the pointer rests on the trigger, never per frame.

use super::{Kbd, META, RADIUS_MD};
use gpui::{
    Action, AnyView, App, AppContext as _, Context, IntoElement, ParentElement as _, Render,
    SharedString, Styled as _, Window, div, prelude::FluentBuilder as _,
};
use gpui_component::{ActiveTheme as _, h_flex};

pub struct Tooltip {
    text: SharedString,
    action: Option<(Box<dyn Action>, Option<SharedString>)>,
    /// A trigger under a dialog: drawn as nothing (see [`dialog_opened`]).
    covered: Covered,
}

/// Dialogs opened so far, and where the pointer was at the last one.
#[derive(Default)]
struct Dialogs {
    opened: u64,
    at: gpui::Point<gpui::Pixels>,
}

impl gpui::Global for Dialogs {}

#[derive(Default)]
enum Covered {
    #[default]
    No,
    /// Shown before dialog number `n` opened over it.
    Before(u64),
    Yes,
}

/// Called as a dialog opens. GPUI checks a tooltip's trigger by its bounds, not whether something covers it, so a
/// tooltip shown before the dialog (or shown late, after a small move over the button that opened it) would stay
/// drawn over the dialog. Tooltips built before it, or at the pointer that opened it, draw nothing while it's open.
pub(super) fn dialog_opened(window: &Window, cx: &mut App) {
    let d = cx.default_global::<Dialogs>();
    d.opened += 1;
    d.at = window.mouse_position();
}

/// What a tooltip built now must hide behind: the opener's spot right after a dialog opens.
fn covered(window: &mut Window, cx: &mut App) -> Covered {
    use gpui_component::WindowExt as _;
    let (opened, at) = cx
        .try_global::<Dialogs>()
        .map_or((0, gpui::Point::default()), |d| (d.opened, d.at));
    let dialog = window.has_active_dialog(cx);
    // shortcut: "near the opener" is 32 px of the click; a dialog control right under the click loses its tooltip
    // until the pointer moves off, which beats the opener's tooltip drawing over the dialog.
    let near = (window.mouse_position() - at).magnitude() < 32.;
    match (dialog, near) {
        (true, true) => Covered::Yes,
        (true, false) => Covered::No,
        (false, _) => Covered::Before(opened),
    }
}

impl Tooltip {
    pub fn new(text: impl Into<SharedString>) -> Self {
        Self {
            text: text.into(),
            action: None,
            covered: Covered::No,
        }
    }

    /// Shows the key bound to `action` (in `context`) beside the text.
    pub fn action(mut self, action: &dyn Action, context: Option<&str>) -> Self {
        self.action = Some((action.boxed_clone(), context.map(SharedString::new)));
        self
    }

    pub fn build(mut self, window: &mut Window, cx: &mut App) -> AnyView {
        self.covered = covered(window, cx);
        cx.new(|_| self).into()
    }

    /// For `.tooltip(..)` on any stateful element: `div().id(..).tooltip(Tooltip::text("Copy"))`.
    pub fn text(text: impl Into<SharedString>) -> impl Fn(&mut Window, &mut App) -> AnyView {
        let text = text.into();
        move |window, cx| Tooltip::new(text.clone()).build(window, cx)
    }
}

impl Tooltip {
    /// Under a dialog that opened over its trigger: drawn as nothing.
    fn hidden(&self, window: &mut Window, cx: &mut App) -> bool {
        use gpui_component::WindowExt as _;
        match self.covered {
            Covered::No => false,
            Covered::Yes => window.has_active_dialog(cx),
            Covered::Before(n) => {
                window.has_active_dialog(cx)
                    && cx.try_global::<Dialogs>().is_some_and(|d| d.opened > n)
            }
        }
    }
}

impl Render for Tooltip {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.hidden(window, cx) {
            return div();
        }
        let key = self
            .action
            .as_ref()
            .and_then(|(a, c)| Kbd::for_action(a.as_ref(), c.as_deref(), window));
        let theme = cx.theme();
        // The margin keeps the box off the pointer; GPUI places the view at the cursor.
        div().child(
            h_flex()
                .m_3()
                .max_w(gpui::px(360.))
                .gap_2()
                .px_2()
                .py_1()
                .rounded(RADIUS_MD)
                .border_1()
                .border_color(theme.border)
                .bg(theme.popover)
                .shadow_md()
                .font_family(theme.font_family.clone())
                .text_size(META)
                .text_color(theme.popover_foreground)
                .child(div().min_w_0().child(self.text.clone()))
                .when_some(key, |this, key| this.child(key)),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::Tooltip;
    use gpui::{
        AppContext as _, Context, IntoElement, Render, TestAppContext, Window, div, point, px,
    };

    struct Blank;
    impl Render for Blank {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div()
        }
    }

    #[gpui::test]
    fn a_tooltip_under_a_new_dialog_hides_and_one_inside_it_shows(cx: &mut TestAppContext) {
        cx.update(|cx| {
            gpui_component::init(cx);
            crate::ui::install_theme(cx).expect("themes parse");
        });
        let window = cx.add_window(|window, cx| {
            let view = cx.new(|_| Blank);
            gpui_component::Root::new(view, window, cx)
        });
        let cx = gpui::VisualTestContext::from_window(window.into(), cx).into_mut();
        cx.simulate_mouse_move(point(px(20.), px(20.)), None, gpui::Modifiers::none());
        let built = |cx: &mut gpui::VisualTestContext| {
            cx.update(|window, cx| {
                let mut t = Tooltip::new("Merge or review…");
                t.covered = super::covered(window, cx);
                t.hidden(window, cx)
            })
        };
        // Shown before any dialog: shows.
        let before = cx.update(|window, cx| {
            let mut t = Tooltip::new("Merge or review…");
            t.covered = super::covered(window, cx);
            t
        });
        assert!(!cx.update(|window, cx| before.hidden(window, cx)));
        cx.update(|window, cx| crate::ui::open_dialog(window, cx, |d, _, _| d.title("PR")));
        cx.run_until_parked();
        assert!(
            cx.update(|window, cx| before.hidden(window, cx)),
            "built before the dialog: hidden while it's up"
        );
        assert!(built(cx), "built late at the opener's pointer: hidden");
        cx.simulate_mouse_move(point(px(200.), px(200.)), None, gpui::Modifiers::none());
        assert!(
            !built(cx),
            "a control elsewhere in the dialog keeps its tooltip"
        );
        cx.update(|window, cx| {
            use gpui_component::WindowExt as _;
            window.close_dialog(cx)
        });
        cx.run_until_parked();
        assert!(
            !cx.update(|window, cx| before.hidden(window, cx)),
            "back once the dialog closes"
        );
    }
}
