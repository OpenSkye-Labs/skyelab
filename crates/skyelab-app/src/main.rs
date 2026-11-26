// Release builds on Windows are GUI apps: no console window opens beside SkyeLab. Debug builds keep it for logs.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

/// `--features dhat-heap`: dhat counts every allocation (docs/debugging.md). Never in a default build.
#[cfg(feature = "dhat-heap")]
#[global_allocator]
static ALLOC: dhat::Alloc = dhat::Alloc;

fn main() {
    skyelab_app::run();
}
