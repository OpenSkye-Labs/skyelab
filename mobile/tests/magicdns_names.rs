//! Acceptance: the picker takes only MagicDNS names, `<machine>.<tailnet>.ts.net` (docs/architecture.md, Decisions).

use skyelab_mobile::env_store::validate;

#[test]
fn magicdns_names_are_accepted_and_normalized() {
    for (input, stored) in [
        ("laptop.tail1234.ts.net", "laptop.tail1234.ts.net"),
        ("  Laptop.Tail1234.TS.NET ", "laptop.tail1234.ts.net"),
        (
            "my-mac.example-tailnet.ts.net",
            "my-mac.example-tailnet.ts.net",
        ),
    ] {
        assert_eq!(validate(input).as_deref(), Ok(stored), "{input:?}");
    }
}

#[test]
fn ips_localhost_and_short_names_get_the_magicdns_message() {
    for bad in [
        "100.64.0.1",
        "192.168.1.5",
        "127.0.0.1",
        "::1",
        "[fd7a:115c:a1e0::1]",
        "fd7a:115c:a1e0::1",
        "localhost",
        "LOCALHOST",
        "localhost.localdomain",
        "laptop",
        "laptop.local",
    ] {
        let why = validate(bad).expect_err(bad);
        assert!(
            why.contains("<machine>.<tailnet>.ts.net"),
            "{bad:?}: message {why:?} must name the MagicDNS form"
        );
    }
}

#[test]
fn other_names_are_rejected() {
    for bad in [
        "",
        "   ",
        "laptop.example.com",
        "laptop.tail1234.example.net",
        "tail1234.ts.net",
        "ts.net",
        "a.b.tail1234.ts.net",
        "laptop.tail1234.ts.net.",
        "ws://laptop.tail1234.ts.net",
        "laptop.tail1234.ts.net:7287",
        "lap top.tail1234.ts.net",
        "-laptop.tail1234.ts.net",
        "laptop-.tail1234.ts.net",
        "laptop..ts.net",
        "laptop.tail_1234.ts.net",
        &format!("{}.tail1234.ts.net", "a".repeat(64)),
    ] {
        assert!(validate(bad).is_err(), "{bad:?} must be rejected");
    }
}
