//! Settings → About (T3's About): this build's version and Check now.

use super::{Settings, widgets};
use crate::ui;
use crate::ui::Button;
use crate::updates::{self, UpdateStatus};
use gpui::{AnyElement, Context, prelude::*};
use gpui_component::ActiveTheme as _;

impl Settings {
    pub(super) fn render_about(&self, cx: &mut Context<Self>) -> AnyElement {
        let u = cx.try_global::<updates::Updates>();
        let status = u.map_or(UpdateStatus::Idle, |u| u.status.clone());
        let said = match &status {
            UpdateStatus::Idle => None,
            UpdateStatus::Checking => Some("Checking…".to_owned()),
            UpdateStatus::UpToDate => Some("SkyeLab is up to date.".to_owned()),
            UpdateStatus::NoRelease => Some("No release published yet.".to_owned()),
            UpdateStatus::Available { tag, .. } | UpdateStatus::Updating { tag, .. } => {
                Some(format!(
                    "SkyeLab {} is available.",
                    skyelab_core::update::display_version(tag)
                ))
            }
            UpdateStatus::Failed(e) => Some(format!("Couldn't check: {e}")),
        };
        let version = widgets::setting(
            true,
            &format!("SkyeLab {}", env!("CARGO_PKG_VERSION")),
            &said.unwrap_or_else(|| "Releases come from GitHub.".to_owned()),
            Button::new("about-check")
                .outline()
                .small()
                .label("Check now")
                .disabled(status == UpdateStatus::Checking)
                .on_click(|_, _, cx| updates::check(true, cx)),
            cx,
        );
        let brand = widgets::row(false, cx)
            .gap_3()
            .child(ui::peek_mark(gpui::px(28.), cx.theme().foreground))
            .child(widgets::label(
                "Every folder on its own account",
                "SkyeLab binds each folder to one Claude, Codex, or Antigravity login and never starts an agent under another.",
                cx,
            ))
            .into_any_element();
        widgets::section(
            "About",
            "This build, and where releases come from.",
            [version, brand],
            cx,
        )
        .into_any_element()
    }
}
