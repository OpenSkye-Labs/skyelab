//! `TailscaleCli::cert`: a fake `tailscale` that answers `status` and `cert` like the real one.
#![cfg(unix)]

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use skyelab_tailnet::{Error, Tailnet, TailscaleCli};

const NAME: &str = "laptop.example.ts.net";

fn status(cert_domains: &str) -> String {
    format!(
        r#"{{"BackendState":"Running","Self":{{"ID":"nSelf","DNSName":"{NAME}.","UserID":1,"TailscaleIPs":["100.64.0.1"]}},"CertDomains":{cert_domains}}}"#
    )
}

/// A fake CLI whose `cert` runs `cert_body` (sh, with "$@" being the cert args), and whose status has `domains`.
fn fake_cli(dir: &Path, domains: &str, cert_body: &str) -> TailscaleCli {
    let bin = dir.join("tailscale");
    let script = format!(
        "#!/bin/sh\ncase \"$1\" in\n  status) echo '{}' ;;\n  cert) shift; {cert_body} ;;\nesac\n",
        status(domains)
    );
    std::fs::write(&bin, script).expect("write fake cli");
    std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).expect("chmod fake cli");
    TailscaleCli::with_binaries(vec![bin])
        .with_cert_dir(dir.join("tls"))
        // Long enough for a loaded machine to start the fake; the hang test still finishes far below its 30 s sleep.
        .with_cert_timeout(Duration::from_secs(3))
}

/// Writes the files named by --cert-file / --key-file, as the real CLI does, and logs its args.
const WRITES: &str = r#"echo "$@" > "$(dirname "$0")/args"
while [ $# -gt 0 ]; do
  case "$1" in
    --cert-file) printf 'CERT' > "$2"; shift ;;
    --key-file) printf 'KEY' > "$2"; shift ;;
  esac
  shift
done"#;

#[test]
fn writes_into_private_dir_and_returns_pems() {
    let dir = tempfile::tempdir().expect("temp dir");
    let cli = fake_cli(dir.path(), &format!(r#"["{NAME}"]"#), WRITES);
    let cert = cli.cert(NAME).expect("cert");
    assert_eq!(cert.cert_pem, b"CERT");
    assert_eq!(cert.key_pem, b"KEY");
    let mode = std::fs::metadata(dir.path().join("tls"))
        .expect("cert dir")
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o700, "cert dir mode {mode:o}");
    let args = std::fs::read_to_string(dir.path().join("args")).expect("args");
    assert!(args.trim_end().ends_with(NAME), "{args}");
    assert!(
        args.contains("--min-validity"),
        "renews ahead of expiry: {args}"
    );
}

#[test]
fn existing_cert_dir_is_made_private() {
    let dir = tempfile::tempdir().expect("temp dir");
    let tls: PathBuf = dir.path().join("tls");
    std::fs::create_dir(&tls).expect("mkdir");
    std::fs::set_permissions(&tls, std::fs::Permissions::from_mode(0o755)).expect("chmod");
    let cli = fake_cli(dir.path(), &format!(r#"["{NAME}"]"#), WRITES);
    cli.cert(NAME).expect("cert");
    let mode = std::fs::metadata(&tls).expect("dir").permissions().mode();
    assert_eq!(mode & 0o777, 0o700);
}

#[test]
fn hung_cert_times_out_and_fails_closed() {
    let dir = tempfile::tempdir().expect("temp dir");
    let cli = fake_cli(dir.path(), &format!(r#"["{NAME}"]"#), "exec sleep 30");
    let start = Instant::now();
    let err = cli.cert(NAME).unwrap_err();
    assert!(
        matches!(
            err,
            Error::Timeout {
                command: "cert",
                ..
            }
        ),
        "{err:?}"
    );
    assert!(
        start.elapsed() < Duration::from_secs(15),
        "{:?}",
        start.elapsed()
    );
}

#[test]
fn certs_disabled_is_precise() {
    for domains in ["null", "[]"] {
        let dir = tempfile::tempdir().expect("temp dir");
        let cli = fake_cli(dir.path(), domains, WRITES);
        let err = cli.cert(NAME).unwrap_err();
        assert!(matches!(err, Error::CertsDisabled), "{domains}: {err:?}");
        let msg = err.to_string();
        assert!(msg.contains("HTTPS"), "{msg}");
        assert!(msg.contains("login.tailscale.com/admin/dns"), "{msg}");
        assert!(!dir.path().join("args").exists(), "cert must not run");
    }
}

#[test]
fn name_outside_cert_domains_is_refused() {
    let dir = tempfile::tempdir().expect("temp dir");
    let cli = fake_cli(dir.path(), r#"["other.example.ts.net"]"#, WRITES);
    let err = cli.cert(NAME).unwrap_err();
    assert!(err.to_string().contains("other.example.ts.net"), "{err}");
    assert!(!dir.path().join("args").exists(), "cert must not run");
}

#[test]
fn cli_failure_is_reported() {
    let dir = tempfile::tempdir().expect("temp dir");
    let cli = fake_cli(
        dir.path(),
        &format!(r#"["{NAME}"]"#),
        "echo '500 Internal Server Error: boom' >&2; exit 1",
    );
    let err = cli.cert(NAME).unwrap_err();
    assert!(err.to_string().contains("boom"), "{err}");
}

#[test]
fn hostile_names_never_reach_the_cli() {
    let dir = tempfile::tempdir().expect("temp dir");
    let cli = fake_cli(dir.path(), r#"["--serve-demo"]"#, WRITES);
    for name in [
        "",
        "--serve-demo",
        "a b.ts.net",
        "../x.ts.net",
        "a/b.ts.net",
    ] {
        assert!(cli.cert(name).is_err(), "{name:?} accepted");
    }
    assert!(!dir.path().join("args").exists(), "cert must not run");
}

#[test]
fn no_cert_dir_is_an_error() {
    let cli = TailscaleCli::with_binaries(vec![PathBuf::from("/nonexistent/tailscale")]);
    assert!(cli.cert(NAME).is_err());
}

#[test]
fn default_tailnet_cert_fails_closed() {
    struct Bare;
    impl Tailnet for Bare {
        fn self_node(&self) -> Result<skyelab_tailnet::Node, Error> {
            Err(Error::NotInstalled)
        }
        fn peers(&self) -> Result<Vec<skyelab_tailnet::Node>, Error> {
            Ok(vec![])
        }
        fn whois(&self, _: std::net::SocketAddr) -> Result<skyelab_tailnet::WhoIs, Error> {
            Err(Error::NotInstalled)
        }
    }
    assert!(Bare.cert(NAME).is_err());
}
