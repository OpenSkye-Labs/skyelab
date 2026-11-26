//! Issue #11: before dictation in a language the Mac can't recognize on the device, SkyeLab asks once per language,
//! with the microphone off, whether its audio may go to Apple. Allow is saved in `settings.toml`; Cancel saves nothing.

use super::super::thread::ThreadView;
use crate::ui;
use crate::ui::Button;
use crate::workspace::thread_extras::prefs;
use gpui::{Context, InteractiveElement as _, Window, prelude::*, px};
use gpui_component::WindowExt as _;

/// Opens "Dictation in <locale> sends audio to Apple". Allow saves the locale, then starts dictation again (now
/// allowed); a failed save shows in the composer and starts nothing.
pub(super) fn ask(locale: String, window: &mut Window, cx: &mut Context<ThreadView>) {
    let thread = cx.entity().downgrade();
    ui::open_dialog(window, cx, move |dialog, _, _| {
        let (locale, thread) = (locale.clone(), thread.clone());
        dialog
            .w(px(460.))
            .title(format!("Dictation in {locale} sends audio to Apple"))
            .child(format!(
                "This Mac can't recognize {locale} on the device, so macOS would send what you say to Apple's servers \
                 to turn it into text. Allow it for {locale}, and SkyeLab won't ask again for this language. Remove \
                 the permission with Restore defaults in Settings → Chat."
            ))
            .footer(
                ui::dialog_footer()
                    .child(ui::dialog_cancel("dictation-consent-cancel"))
                    .child(
                        Button::new("dictation-consent-allow")
                            .debug_selector(|| "dictation-consent-allow".into())
                            .primary()
                            .small()
                            .label(format!("Allow for {locale}"))
                            .on_click(move |_, window, cx| {
                                window.close_dialog(cx);
                                let save = prefs::allow_server_dictation(locale.clone(), cx);
                                let thread = thread.clone();
                                window
                                    .spawn(cx, async move |cx| {
                                        let saved = save.await;
                                        let _ = thread.update_in(cx, |t, window, cx| match saved {
                                            Ok(()) => t.start_dictation(window, cx),
                                            Err(e) => {
                                                t.composer.notice = Some(
                                                    format!("Saving the dictation permission failed: {e:#}")
                                                        .into(),
                                                );
                                                cx.notify();
                                            }
                                        });
                                    })
                                    .detach();
                            }),
                    ),
            )
    });
}
