//! Debug binaries carry the dictation usage strings in an embedded Info.plist, so voice input works from `cargo run`;
//! without them macOS aborts the process when it asks for the microphone. SkyeLab.app gets them from cargo-packager
//! (`info-plist-path`), and release binaries are only shipped inside it.

fn main() {
    let plist = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("macos/Info.plist");
    println!("cargo:rerun-if-changed={}", plist.display());
    let mac = std::env::var("CARGO_CFG_TARGET_OS").is_ok_and(|os| os == "macos");
    if mac && std::env::var("PROFILE").is_ok_and(|p| p == "debug") {
        println!(
            "cargo:rustc-link-arg-bins=-Wl,-sectcreate,__TEXT,__info_plist,{}",
            plist.display()
        );
    }
}
