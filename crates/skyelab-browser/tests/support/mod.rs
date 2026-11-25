//! These binaries have no libtest harness (a window needs the main thread), so each is one test. cargo-nextest lists
//! a binary's tests with `--list --format terse` and then runs each by name: answer the list with this binary's name,
//! and run the whole test for any other call.

/// True when this run only lists the tests (the caller then runs nothing).
pub fn listed() -> bool {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if !args.iter().any(|a| a == "--list") {
        return false;
    }
    // Nothing here is #[ignore]d, so the list of ignored tests is empty.
    if !args.iter().any(|a| a == "--ignored") {
        println!("{}: test", env!("CARGO_CRATE_NAME"));
    }
    true
}
