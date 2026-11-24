use crate::release::{MAX_BODY, Release, get_over, parse_npm_latest, parse_release};
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};

/// Runs a GET against a fake server on the other end of an in-memory pipe; returns the result and what was sent.
fn fetch(answer: Vec<u8>) -> (std::io::Result<Vec<u8>>, String) {
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
        let got = get_over(
            client,
            "api.github.com",
            "/repos/o/r/releases/latest",
            "application/vnd.github+json",
        )
        .await;
        (got, serve.await.expect("server"))
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
            url: "https://github.com/OpenSkye-Labs/skyelab/releases/tag/v0.3.0".into()
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
