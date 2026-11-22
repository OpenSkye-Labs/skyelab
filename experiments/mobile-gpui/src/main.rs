#[cfg(not(target_os = "android"))]
fn main() {
    gpui_platform::application().run(skyelab_mobile::app::run);
}

#[cfg(target_os = "android")]
fn main() {}
