//! A copy button: copies its text, then shows a check for [`COPIED_FOR`] (described "Copied" to a screen reader), during
//! which another click copies nothing.

use super::{ActiveTheme as _, Button, Icon, SkyeIcon};
use gpui::{
    App, ClipboardItem, ElementId, IntoElement, ParentElement as _, RenderOnce, SharedString,
    Styled as _, Window, prelude::FluentBuilder as _,
};
use std::time::Duration;

pub const COPIED_FOR: Duration = Duration::from_secs(2);

#[derive(IntoElement)]
pub struct CopyButton {
    id: ElementId,
    value: SharedString,
    tooltip: Option<SharedString>,
    accessibility_label: Option<SharedString>,
}

impl CopyButton {
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self {
            id: id.into(),
            value: SharedString::default(),
            tooltip: None,
            accessibility_label: None,
        }
    }

    pub fn value(mut self, value: impl Into<SharedString>) -> Self {
        self.value = value.into();
        self
    }

    pub fn tooltip(mut self, tooltip: impl Into<SharedString>) -> Self {
        self.tooltip = Some(tooltip.into());
        self
    }

    /// The name a screen reader reads; "Copy" unless set.
    pub fn accessibility_label(mut self, label: impl Into<SharedString>) -> Self {
        self.accessibility_label = Some(label.into());
        self
    }
}

impl RenderOnce for CopyButton {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let copied = window.use_keyed_state(self.id.clone(), cx, |_, _| false);
        let is_copied = *copied.read(cx);
        let name = self.accessibility_label.unwrap_or_else(|| "Copy".into());
        let value = self.value;
        let theme = cx.theme();
        // gpui-component's copy button, kept pixel for pixel: a 1.25 rem square, the theme's radius, a 0.75 rem icon.
        let icon = Icon::new(if is_copied {
            SkyeIcon::CopyButtonDone
        } else {
            SkyeIcon::CopyButton
        })
        .size_3()
        .text_color(theme.secondary_foreground);
        Button::new(self.id)
            .ghost()
            .xsmall()
            .square()
            .size_5()
            .rounded(theme.radius)
            .child(icon)
            .when_some(self.tooltip, |b, text| b.tooltip(text))
            .accessibility_label(name)
            .when(is_copied, |b| b.accessibility_description("Copied"))
            .on_click(move |_, _, cx| {
                cx.stop_propagation();
                if *copied.read(cx) {
                    return;
                }
                cx.write_to_clipboard(ClipboardItem::new_string(value.to_string()));
                copied.update(cx, |c, cx| {
                    *c = true;
                    cx.notify();
                });
                let copied = copied.clone();
                cx.spawn(async move |cx| {
                    cx.background_executor().timer(COPIED_FOR).await;
                    copied.update(cx, |c, cx| {
                        *c = false;
                        cx.notify();
                    });
                })
                .detach();
            })
    }
}
