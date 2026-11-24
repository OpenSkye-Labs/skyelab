//! The machine list's saves to settings.toml (recents, linked machines, Forget), chained in order.

use super::super::Workspace;
use gpui::Context;
use skyelab_client::is_magicdns_name;
use skyelab_core::AppSettings;

impl Workspace {
    /// Applies `change` to settings.toml off the UI thread, after any earlier save, and shows the saved recents. A
    /// file that doesn't parse is left alone and the picker says so.
    pub(in crate::workspace) fn save_env_settings(
        &mut self,
        change: impl FnOnce(&mut AppSettings) + Send + 'static,
        cx: &mut Context<Self>,
    ) {
        let Some(path) = self.env.settings_path.clone() else {
            return;
        };
        let picker = self.env.picker.as_ref().map(|p| p.downgrade());
        let prev = self.env._save.take();
        let write = cx.background_executor().clone();
        // Chained, so quick switches save in the order they happened.
        self.env._save = Some(cx.spawn(async move |_, cx| {
            if let Some(prev) = prev {
                prev.await;
            }
            let saved = write
                .spawn(async move {
                    let (settings, ()) = AppSettings::update(&path, change)?;
                    anyhow::Ok(settings.recent_environments)
                })
                .await;
            let Some(picker) = picker else { return };
            let _ = picker.update(cx, |p, cx| {
                match saved {
                    Ok(recents) => {
                        p.recents = recents;
                        p.recents.retain(|h| is_magicdns_name(h));
                    }
                    Err(e) => {
                        p.error = Some(format!("couldn't save the machine list: {e:#}").into())
                    }
                }
                cx.notify();
            });
        }));
    }
}
