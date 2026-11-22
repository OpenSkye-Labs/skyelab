//! Android entry point. The only crate in `mobile/` allowed `unsafe_code`, because `android_main` must be an
//! unmangled symbol (CLAUDE.md: FFI lives in one tiny crate).
#![allow(unsafe_code)]

#[cfg(target_os = "android")]
// SAFETY: `android-activity` looks this symbol up by name and calls it once, on its own thread, with the
// `AndroidApp` it owns; no other item in the cdylib exports `android_main`, and the signature matches its contract.
#[unsafe(no_mangle)]
fn android_main(native: android_activity::AndroidApp) {
    use gpui_mobile::android::jni;
    android_logger::init_once(
        android_logger::Config::default()
            .with_max_level(log::LevelFilter::Info)
            .with_tag("skyelab-mobile"),
    );
    jni::install_panic_hook();
    let data_dir = native
        .internal_data_path()
        .unwrap_or_else(|| std::path::PathBuf::from("."));
    let _platform = jni::init_platform(&native);
    let Some(shared) = jni::shared_platform() else {
        log::error!("gpui-mobile did not create its platform");
        return;
    };
    gpui::Application::with_platform(shared.into_rc()).run(move |cx| {
        if let Err(e) = skyelab_mobile::app::run(data_dir, cx) {
            log::error!("could not open the window: {e:#}");
        }
    });
}
