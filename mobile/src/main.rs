//! iOS entry point (also builds on the desktop host, for `cargo test` and clippy).

use std::path::PathBuf;

fn main() {
    // The app sandbox's HOME on iOS; the desktop host only runs this for a quick look.
    let data_dir = std::env::var_os("HOME")
        .map(|home| PathBuf::from(home).join("Library/Application Support/SkyeLab"))
        .unwrap_or_else(|| PathBuf::from("."));
    gpui_platform::application().run(move |cx| skyelab_mobile::app::run(data_dir, cx));
}
