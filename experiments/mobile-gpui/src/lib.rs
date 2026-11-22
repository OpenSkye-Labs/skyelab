//! Mobile spike: one GPUI view, two platform entry points (iOS binary in main.rs, Android cdylib here).

pub mod app;

/// Called by `android-activity` on its native thread; returns when the activity is destroyed.
#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
fn android_main(native: android_activity::AndroidApp) {
    use gpui_mobile::android::jni;
    android_logger::init_once(
        android_logger::Config::default()
            .with_max_level(log::LevelFilter::Info)
            .with_tag("skyelab-mobile"),
    );
    jni::install_panic_hook();
    let _platform = jni::init_platform(&native);
    let Some(shared) = jni::shared_platform() else {
        log::error!("gpui-mobile did not create its platform");
        return;
    };
    gpui::Application::with_platform(shared.into_rc()).run(app::run);
}
