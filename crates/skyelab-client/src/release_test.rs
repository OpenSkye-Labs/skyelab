use crate::release::{
    Got, MAX_BODY, Release, exchange, parse_npm_latest, parse_release, redirect_ok,
};
use skyelab_core::update::Asset;
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};

/// Runs a GET against a fake server on the other end of an in-memory pipe; returns the result and what was sent.
fn fetch(answer: Vec<u8>) -> (std::io::Result<Vec<u8>>, String) {
    let (got, body, sent) = fetch_capped(answer, MAX_BODY as u64);
    let got = got.and_then(|g| match g {
        Got::Body => Ok(body),
        Got::Redirect(_) => Err(std::io::Error::other("redirect")),
    });
    (got, sent)
}

/// [`fetch`] with its own cap, returning what the exchange said and the body it streamed.
fn fetch_capped(answer: Vec<u8>, cap: u64) -> (std::io::Result<Got>, Vec<u8>, String) {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");
    rt.block_on(async {
        let (client, mut server) = tokio::io::duplex(64 * 1024);
        let serve = tokio::spawn(async move {
            let mut req = vec![0u8; 4096];
            let n = server.read(&mut req).await.expect("request");
            // Large answers fill the pipe; the client may give up early, so a failed write is fine.
            let _ = server.write_all(&answer).await;
            drop(server);
            String::from_utf8_lossy(&req[..n]).into_owned()
        });
        let mut body = Vec::new();
        let got = exchange(
            client,
            "api.github.com",
            "/repos/o/r/releases/latest",
            "application/vnd.github+json",
            cap,
            &mut body,
            &mut |_, _| {},
        )
        .await;
        (got, body, serve.await.expect("server"))
    })
}

fn ok_answer(body: &str) -> Vec<u8> {
    format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n\r\n{body}").into_bytes()
}

#[test]
fn plain_get_and_parse() {
    let body = r#"{"tag_name":"v0.3.0","html_url":"https://github.com/OpenSkye-Labs/skyelab/releases/tag/v0.3.0","draft":false}"#;
    let (got, sent) = fetch(ok_answer(body));
    let release = parse_release(&got.expect("200")).expect("parses");
    assert_eq!(
        release,
        Release {
            tag: "v0.3.0".into(),
            url: "https://github.com/OpenSkye-Labs/skyelab/releases/tag/v0.3.0".into(),
            assets: vec![],
        }
    );
    assert!(
        sent.starts_with("GET /repos/o/r/releases/latest HTTP/1.0\r\n"),
        "{sent}"
    );
    let lower = sent.to_ascii_lowercase();
    assert!(lower.contains("host: api.github.com\r\n"));
    assert!(
        !lower.contains("authorization") && !lower.contains("cookie"),
        "{sent}"
    );
}

#[test]
fn non_200_fails() {
    let (got, _) = fetch(b"HTTP/1.1 403 Forbidden\r\n\r\n{\"message\":\"rate limited\"}".to_vec());
    let err = got.expect_err("403 refused");
    assert!(err.to_string().contains("403"), "{err}");
    assert_eq!(err.kind(), std::io::ErrorKind::Other);
    // A repo with no release yet: GitHub's latest release is a 404, which the app says plainly.
    let (got, _) = fetch(b"HTTP/1.1 404 Not Found\r\n\r\n{\"message\":\"Not Found\"}".to_vec());
    assert_eq!(got.expect_err("404").kind(), std::io::ErrorKind::NotFound);
}

#[test]
fn oversized_body_refused() {
    let big = "x".repeat(MAX_BODY + 32 * 1024);
    let (got, _) = fetch(ok_answer(&big));
    assert!(got.is_err());
}

#[test]
fn missing_fields_refused() {
    assert!(parse_release(br#"{"tag_name":"v1"}"#).is_err());
    assert!(parse_release(b"not json").is_err());
}

#[test]
fn npm_latest_reads_a_plain_version_only() {
    let got = parse_npm_latest(br#"{"name":"@openai/codex","version":"0.47.0"}"#).expect("parses");
    assert_eq!(got, "0.47.0");
    assert!(parse_npm_latest(br#"{"name":"x"}"#).is_err());
    assert!(parse_npm_latest(br#"{"version":"1.0 <script>"}"#).is_err());
    assert!(parse_npm_latest(b"not json").is_err());
}

#[test]
fn release_lists_its_assets() {
    let body = r#"{"tag_name":"v0.3.0","html_url":"https://github.com/o/r/releases/tag/v0.3.0","assets":[
        {"name":"SHA256SUMS","browser_download_url":"https://github.com/o/r/releases/download/v0.3.0/SHA256SUMS"},
        {"name":"no url"}]}"#;
    let release = parse_release(body.as_bytes()).expect("parses");
    assert_eq!(
        release.assets,
        vec![Asset {
            name: "SHA256SUMS".into(),
            url: "https://github.com/o/r/releases/download/v0.3.0/SHA256SUMS".into()
        }]
    );
}

#[test]
fn redirects_are_returned_not_followed() {
    let (got, body, _) = fetch_capped(
        b"HTTP/1.1 302 Found\r\nLocation: https://release-assets.githubusercontent.com/x?sig=1\r\n\r\nmoved".to_vec(),
        100,
    );
    assert_eq!(
        got.expect("redirect"),
        Got::Redirect("https://release-assets.githubusercontent.com/x?sig=1".into())
    );
    assert!(body.is_empty(), "a redirect's body never reaches the file");
}

#[test]
fn download_caps_and_length() {
    let answer = |len: usize, body: &str| {
        format!("HTTP/1.1 200 OK\r\nContent-Length: {len}\r\n\r\n{body}").into_bytes()
    };
    let (got, body, _) = fetch_capped(answer(5, "hello"), 5);
    assert_eq!(got.expect("fits"), Got::Body);
    assert_eq!(body, b"hello");
    let (got, _, _) = fetch_capped(answer(6, "hello!"), 5);
    assert!(got.is_err(), "over the cap");
    let (got, _, _) = fetch_capped(b"HTTP/1.1 200 OK\r\n\r\nhello!".to_vec(), 5);
    assert!(got.is_err(), "over the cap with no length");
    // A server that sends the stated length and more stops being read at the length.
    let (got, body, _) = fetch_capped(answer(5, "hello, and more"), 100);
    assert_eq!(got.expect("stops at the length"), Got::Body);
    assert_eq!(body, b"hello");
    let (got, _, _) = fetch_capped(answer(10, "hello"), 100);
    assert!(got.expect_err("short").to_string().contains("cut short"));
}

#[test]
fn redirects_stay_on_github() {
    assert!(redirect_ok(
        "https://release-assets.githubusercontent.com/github-production-release-asset/1?x=y"
    ));
    assert!(redirect_ok("https://objects.githubusercontent.com/a"));
    assert!(redirect_ok("https://github.com/o/r/releases/download/v1/x"));
    assert!(
        !redirect_ok("http://objects.githubusercontent.com/a"),
        "plain http"
    );
    assert!(!redirect_ok("https://githubusercontent.com.evil.example/a"));
    assert!(!redirect_ok(
        "https://evil.example/.githubusercontent.com/a"
    ));
    assert!(!redirect_ok("https://.githubusercontent.com/a"));
    assert!(
        !redirect_ok("https://raw.githubusercontent.com/someone/else/main/x"),
        "user content"
    );
    assert!(!redirect_ok("https://user@objects.githubusercontent.com/a"));
}
