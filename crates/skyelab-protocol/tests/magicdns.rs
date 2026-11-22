//! The one rule for a machine name a person types, shared by the desktop picker and the phone: a MagicDNS name,
//! `<machine>.<tailnet>.ts.net`. TLS certificates exist only for those names.

use skyelab_protocol::is_magicdns_name;

#[test]
fn accepts_magicdns_names() {
    for ok in [
        "laptop.tail1234.ts.net",
        "Laptop.Tail1234.TS.NET",
        "my-box.example-corp.ts.net",
        "a.b.ts.net",
        &format!("{}.tail1234.ts.net", "a".repeat(63)),
    ] {
        assert!(is_magicdns_name(ok), "{ok:?} must be accepted");
    }
}

#[test]
fn rejects_everything_else() {
    for bad in [
        "",
        "laptop",
        "localhost",
        "localhost.ts.net",
        "100.64.0.1",
        "fd7a:115c:a1e0::1",
        "[fd7a:115c:a1e0::1]",
        "tail1234.ts.net",
        "ts.net",
        "a.b.c.ts.net",
        "laptop.tail1234.ts.net.",
        "laptop.tail1234.example.com",
        "laptop.tail1234.ts.network",
        "laptop.tail1234.ts-net",
        "-laptop.tail1234.ts.net",
        "laptop-.tail1234.ts.net",
        "laptop.-tail.ts.net",
        "lap top.tail1234.ts.net",
        "lap_top.tail1234.ts.net",
        "ws://laptop.tail1234.ts.net",
        "laptop.tail1234.ts.net:7287",
        "laptop.tail1234.ts.net/x",
        "laptop..ts.net",
        ".laptop.tail1234.ts.net",
        " laptop.tail1234.ts.net",
        "läptop.tail1234.ts.net",
        &format!("{}.tail1234.ts.net", "a".repeat(64)),
    ] {
        assert!(!is_magicdns_name(bad), "{bad:?} must be rejected");
    }
}
