//! The composer's permission-mode picker and the Full access confirm.

use super::*;

/// The mode's icon, T3's: a lock (asks), a clipboard (plans), a pen (edits go ahead), an open shield (nothing asks).
fn mode_icon(mode: skyelab_agent::Mode) -> SkyeIcon {
    use skyelab_agent::Mode;
    match mode {
        Mode::Default => SkyeIcon::Ask,
        Mode::Plan => SkyeIcon::ClipboardList,
        Mode::AcceptEdits => SkyeIcon::PenLine,
        Mode::Bypass => SkyeIcon::ShieldOff,
    }
}

/// What each permission mode is called in the composer, and what it allows.
pub(super) fn mode_label(mode: skyelab_agent::Mode) -> (&'static str, &'static str) {
    use skyelab_agent::Mode;
    match mode {
        Mode::Default => (
            "Ask",
            "The agent asks before commands and edits its rules don't already allow",
        ),
        Mode::Plan => ("Plan", "Read and plan only: no edits, no commands"),
        Mode::AcceptEdits => (
            "Accept edits",
            "File edits in this folder go ahead; anything else asks",
        ),
        Mode::Bypass => (
            "Full access",
            "Nothing asks: any command, any edit (Codex also leaves its sandbox)",
        ),
    }
}

impl ThreadView {
    /// The thread's permission mode as one chip, T3's: the mode's icon and name, a menu of the four with what each
    /// allows. Full access is drawn in the warning color. `compact` (a narrow composer) keeps only the icon.
    pub(in crate::workspace) fn render_mode_picker(
        &self,
        compact: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = cx.theme();
        let (label, tip) = mode_label(self.mode);
        if self.is_side_chat() {
            // Locked: no menu, and the tooltip says why.
            return Button::new("permission-mode")
                .ghost()
                .small()
                .icon(Icon::new(mode_icon(self.mode)))
                .when(!compact, |b| b.label(label))
                .tooltip(super::super::side_chat::PLAN_ONLY)
                .into_any_element();
        }
        let view = cx.entity().downgrade();
        let current = self.mode;
        Button::new("permission-mode")
            .ghost()
            .small()
            .icon(Icon::new(mode_icon(self.mode)))
            .when(!compact, |b| b.label(label).dropdown_caret(true))
            .tooltip(format!("Permissions: {label}. {tip}"))
            .when(self.mode == skyelab_agent::Mode::Bypass, |b| {
                b.text_color(ui::ink(theme, theme.warning))
            })
            .dropdown_menu_with_anchor(gpui::Anchor::BottomLeft, move |mut menu, _, _| {
                menu = menu.label("Permissions");
                for mode in skyelab_agent::Mode::ALL {
                    let (label, tip) = mode_label(mode);
                    let view = view.clone();
                    let item = PopupMenuItem::element(move |_, cx| {
                        let theme = cx.theme();
                        // A set width, so each description wraps inside the menu.
                        v_flex()
                            .w(px(300.))
                            .py_0p5()
                            .child(
                                h_flex()
                                    .gap_2()
                                    .child(Icon::new(mode_icon(mode)).size_3p5())
                                    .child(label),
                            )
                            .child(
                                div()
                                    .pl(px(22.))
                                    .text_size(ui::META)
                                    .text_color(theme.muted_foreground)
                                    .child(tip),
                            )
                    })
                    .checked(mode == current)
                    .on_click(move |_, _, cx| {
                        let _ = view.update(cx, |this, cx| this.set_mode(mode, cx));
                    });
                    menu = menu.item(item);
                }
                menu
            })
            .into_any_element()
    }

    /// Full access was picked: say what it allows and ask before turning it on.
    pub(super) fn render_bypass_confirm(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.confirm_bypass {
            return None;
        }
        let actions = h_flex()
            .gap_2()
            .child(
                Button::new("confirm-bypass")
                    .danger()
                    .small()
                    .label("Turn on Full access")
                    .on_click(cx.listener(|this, _, _, cx| this.confirm_mode(true, cx))),
            )
            .child(
                Button::new("cancel-bypass")
                    .ghost()
                    .small()
                    .label("Keep asking")
                    .on_click(cx.listener(|this, _, _, cx| this.confirm_mode(false, cx))),
            );
        Some(banner(
            "Give this thread's agent full access?",
            "It will run any command and change any file without asking, under this folder's account. A Codex \
             agent also leaves its sandbox: it can write outside the folder and use the network.",
            div()
                .debug_selector(|| "bypass-confirm".into())
                .child(actions),
            cx,
        ))
    }
}
