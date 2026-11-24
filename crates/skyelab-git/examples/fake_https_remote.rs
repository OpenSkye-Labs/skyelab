//! The fake https git remote for live runs (`/loop-verify-ui`); never shipped. See `tests/support/https_remote.rs`.
//!
//! `cargo run -p skyelab-git --example fake_https_remote -- <folder>` serves the bare repos in `<folder>` (make one
//! with `git init --bare <folder>/demo.git`; anything else in it is refused, since any local process with the public
//! fake token can read and push there) at `https://127.0.0.1:<port>/demo.git`, prints `url=` and `ca=` lines
//! and runs until killed. Start a debug SkyeLab with `SKYELAB_GIT_CA_FILE=<ca>` and the fake gh, and set a repo's
//! origin to the URL. `--check` serves one `ls-remote` to itself and exits 0 when it worked.

#[path = "../tests/support/https_remote.rs"]
mod https_remote;

use std::process::Command;

fn main() {
    let arg = std::env::args().nth(1).unwrap_or_default();
    if arg == "--check" {
        std::process::exit(if check() { 0 } else { 1 });
    }
    if arg.is_empty() {
        eprintln!("usage: fake_https_remote <folder of bare repos> | --check");
        std::process::exit(2);
    }
    let root = std::fs::canonicalize(&arg).expect("the folder exists");
    // Anyone on this machine with the (public) fake token can read and push here: only throwaway bare repos.
    let entries = std::fs::read_dir(&root).expect("read the folder");
    let other = entries
        .flatten()
        .find(|e| !e.file_name().to_string_lossy().ends_with(".git"));
    if let Some(e) = other {
        eprintln!(
            "{} holds {:?}: point fake_https_remote at a throwaway folder of bare *.git repos only",
            root.display(),
            e.file_name()
        );
        std::process::exit(2);
    }
    let remote = https_remote::HttpsRemote::start(&root);
    println!("url={}", remote.base);
    println!("ca={}", remote.ca_file.display());
    loop {
        std::thread::park();
    }
}

/// A bare repo with one commit, served, then listed through the CA with the fake gh's credentials.
fn check() -> bool {
    let tmp = tempfile::tempdir().expect("tmp");
    let root = std::fs::canonicalize(tmp.path()).expect("canon");
    let work = root.join("work");
    let git = |args: &[&str]| {
        Command::new("git")
            .args(args)
            .output()
            .is_ok_and(|o| o.status.success())
    };
    let w = work.to_str().expect("utf8");
    let made = git(&[
        "init",
        "-q",
        "--bare",
        root.join("demo.git").to_str().expect("utf8"),
    ]) && git(&["init", "-q", "-b", "main", w])
        && git(&[
            "-C",
            w,
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@example.com",
            "commit",
            "-q",
            "--allow-empty",
            "-m",
            "init",
        ])
        && git(&["-C", w, "push", "-q", "../demo.git", "main"]);
    if !made {
        eprintln!("couldn't make the repo");
        return false;
    }
    let remote = https_remote::HttpsRemote::start(&root);
    let out = Command::new("git")
        .args([
            "-c",
            "credential.helper=",
            "-c",
            "credential.helper=!f() { echo username=x-access-token; echo password=fake-token; }; f",
            "ls-remote",
            &format!("{}/demo.git", remote.base),
        ])
        .env("GIT_SSL_CAINFO", &remote.ca_file)
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .expect("git runs");
    let listed = String::from_utf8_lossy(&out.stdout);
    println!("{}", listed.trim());
    let seen = remote.seen.lock().expect("seen lock").clone();
    for s in &seen {
        println!("{} {} authorized={}", s.method, s.target, s.authorized);
    }
    out.status.success() && listed.contains("refs/heads/main") && seen.iter().any(|s| s.authorized)
}
