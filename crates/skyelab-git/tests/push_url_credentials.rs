//! A push must authenticate only through the bound gh, never with credentials embedded in the remote URL.
//! The "remote" is a local TCP listener on 127.0.0.1 that answers 401 and records Authorization headers.

use std::fs;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};

use skyelab_git::GitHub;

fn git(dir: &Path, args: &[&str]) {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .expect("git runs");
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn push_never_authenticates_with_credentials_in_the_remote_url() {
    let tmp = tempfile::TempDir::new().expect("tempdir");
    let base = fs::canonicalize(tmp.path()).expect("canonical");
    let repo = base.join("repo");
    let gh_dir = base.join("gh");
    fs::create_dir_all(&repo).expect("mkdir");
    fs::create_dir_all(&gh_dir).expect("mkdir");
    git(&repo, &["init", "-q", "-b", "main"]);
    for (k, v) in [
        ("user.name", "t"),
        ("user.email", "t@example.com"),
        ("commit.gpgsign", "false"),
    ] {
        git(&repo, &["config", k, v]);
    }
    fs::write(repo.join("a.txt"), "one\n").expect("write");
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "init"]);
    git(&repo, &["checkout", "-q", "-b", "feature"]);

    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("addr").port();
    let seen: Arc<Mutex<Vec<String>>> = Arc::default();
    let rec = seen.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut s) = stream else { continue };
            let mut req = Vec::new();
            let mut buf = [0u8; 4096];
            while !req.windows(4).any(|w| w == b"\r\n\r\n") {
                match s.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => req.extend_from_slice(&buf[..n]),
                }
            }
            for line in String::from_utf8_lossy(&req).lines() {
                if line.to_ascii_lowercase().starts_with("authorization:") {
                    rec.lock().expect("lock").push(line.to_owned());
                }
            }
            let _ = s.write_all(
                b"HTTP/1.1 401 Unauthorized\r\nWWW-Authenticate: Basic realm=\"x\"\r\n\
                  Content-Length: 0\r\nConnection: close\r\n\r\n",
            );
        }
    });
    git(
        &repo,
        &[
            "remote",
            "add",
            "origin",
            &format!("http://a:b@127.0.0.1:{port}/acme/demo.git"),
        ],
    );

    let bin: PathBuf = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fake-gh.sh");
    let gh = GitHub {
        gh: &bin,
        config_dir: &gh_dir,
    };
    let _ = skyelab_git::push(&repo, &gh);

    let auth = seen.lock().expect("lock").clone();
    assert!(
        !auth.iter().any(|l| l.contains("YTpi")),
        "push authenticated with the URL's credentials, not the bound gh: {auth:?}"
    );
}
