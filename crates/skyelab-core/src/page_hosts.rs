//! The hosts a visual reply's page may load scripts, styles and fonts from (Settings → Chat → Visual replies). Each
//! one becomes `https://<host>` in the page's CSP and the Windows lock's list, so this is a trust boundary: an entry
//! that could reach this machine, its networks or any host is refused, here and again wherever the list is used.

/// A fresh install's list: the common script CDNs.
pub const DEFAULT_PAGE_HOSTS: [&str; 3] = ["cdnjs.cloudflare.com", "cdn.jsdelivr.net", "unpkg.com"];

/// Names that resolve inside this machine or its networks, never on the public internet, and public names made to
/// resolve to any address they spell (`127.0.0.1.nip.io`).
const LOCAL_SUFFIXES: [&str; 19] = [
    "localhost",
    "local",
    "localdomain",
    "internal",
    "intranet",
    "corp",
    "private",
    "lan",
    "home",
    "home.arpa",
    "arpa",
    "ts.net",
    "test",
    "invalid",
    "onion",
    "nip.io",
    "sslip.io",
    "localtest.me",
    "lvh.me",
];

/// `input` as a list entry: a public host name (`cdn.example.com`) or a wildcard for its subdomains
/// (`*.example.com`), lowercase. An `https://` in front and one `/` after are dropped; anything else (another
/// scheme, a path, a port, an IP address, a local name, a bare `*` or a one-label name) is refused with why.
pub fn parse_host(input: &str) -> Result<String, &'static str> {
    let mut host = input.trim().to_ascii_lowercase();
    if let Some((scheme, rest)) = host.split_once("://") {
        if scheme != "https" {
            return Err("Only https hosts can be allowed.");
        }
        host = rest.to_owned();
    }
    if let Some(bare) = host.strip_suffix('/') {
        host = bare.to_owned();
    }
    if host.contains(['/', '?', '#', '\\']) {
        return Err("A host only, without a path.");
    }
    if host.contains([':', '[', ']']) {
        return Err("A host name only, without a port or IPv6 address.");
    }
    let name = host.strip_prefix("*.").unwrap_or(&host);
    if name.is_empty() || name.contains('*') {
        return Err("A wildcard covers one name's subdomains only, as in *.example.com.");
    }
    if name.len() > 253 {
        return Err("That name is too long.");
    }
    let labels: Vec<&str> = name.split('.').collect();
    let label_ok = |l: &&str| {
        (1..=63).contains(&l.len())
            && !l.starts_with('-')
            && !l.ends_with('-')
            && l.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
    };
    if !labels.iter().all(label_ok) {
        return Err("That isn't a host name (letters, digits, dots and dashes).");
    }
    if LOCAL_SUFFIXES
        .iter()
        .any(|s| name == *s || name.ends_with(&format!(".{s}")))
    {
        return Err(
            "A local name can't be allowed: pages never reach this machine or its networks.",
        );
    }
    if labels.len() < 2 {
        return Err("A full host name, as in cdn.example.com.");
    }
    // `*.github.io` or `*.co.uk` would be anyone's host.
    if host.starts_with("*.") && psl::suffix_str(name) == Some(name) {
        return Err("A wildcard over a public suffix would allow anyone's host.");
    }
    // An IPv4 address (and the forms browsers read as one, like 0x7f.1) ends in a numeric label; no public TLD does.
    let last = labels[labels.len() - 1];
    if last.chars().all(|c| c.is_ascii_digit()) || last.starts_with("0x") {
        return Err("An IP address can't be allowed, only a host name.");
    }
    Ok(host)
}

/// The entries of `list` that [`parse_host`] accepts, normalized, each once. A hand-edited settings file can hold
/// anything; this is what the page gets.
pub fn valid_hosts(list: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for host in list.iter().filter_map(|h| parse_host(h).ok()) {
        if !out.contains(&host) {
            out.push(host);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{DEFAULT_PAGE_HOSTS, parse_host, valid_hosts};

    #[test]
    fn accepts_public_hosts_and_subdomain_wildcards() {
        for (input, host) in [
            ("cdnjs.cloudflare.com", "cdnjs.cloudflare.com"),
            ("  CDN.JSDelivr.net ", "cdn.jsdelivr.net"),
            ("https://unpkg.com", "unpkg.com"),
            ("HTTPS://unpkg.com/", "unpkg.com"),
            ("*.example.com", "*.example.com"),
            ("fonts.gstatic.com", "fonts.gstatic.com"),
            ("xn--bcher-kva.example", "xn--bcher-kva.example"),
        ] {
            assert_eq!(parse_host(input).as_deref(), Ok(host), "{input}");
        }
        for host in DEFAULT_PAGE_HOSTS {
            assert!(parse_host(host).is_ok(), "{host}");
        }
    }

    #[test]
    fn refuses_anything_that_reopens_a_local_or_any_host_hole() {
        for input in [
            "",
            "*",
            "*.",
            "*.com",
            "**.example.com",
            "a.*.example.com",
            "https://*",
            "http://cdn.example.com",
            "ws://cdn.example.com",
            "file:///etc",
            "data:text/html,x",
            "cdn.example.com/lib.js",
            "https://cdn.example.com/x",
            "cdn.example.com?x",
            "cdn.example.com:8443",
            "user@cdn.example.com",
            "cdn.example.com;script-src *",
            "cdn.example.com 'unsafe-inline'",
            "cdn example.com",
            "\"cdn.example.com\"",
            "127.0.0.1",
            "10.0.0.1",
            "192.168.1.10",
            "0x7f.1",
            "[::1]",
            "::1",
            "localhost",
            "app.localhost",
            "printer.local",
            "*.local",
            "db.internal",
            "router.lan",
            "nas.home.arpa",
            "1.0.0.127.in-addr.arpa",
            "box.tail1234.ts.net",
            "intranet",
            "wiki.corp",
            "127.0.0.1.nip.io",
            "10-0-0-1.sslip.io",
            "localtest.me",
            "*.co.uk",
            "*.github.io",
            "*.workers.dev",
            "*.vercel.app",
            "-bad.example.com",
            "bad-.example.com",
            "a..example.com",
            "cdn.example.com.",
        ] {
            assert!(parse_host(input).is_err(), "{input:?} was accepted");
        }
        for local in ["localhost", "printer.local", "box.tail1234.ts.net"] {
            let why = parse_host(local).expect_err(local);
            assert!(why.contains("local name"), "{local}: {why}");
        }
    }

    #[test]
    fn valid_hosts_drops_bad_entries_and_repeats() {
        let list = [
            "unpkg.com",
            "localhost",
            "UNPKG.com",
            "*",
            "https://cdn.jsdelivr.net",
        ]
        .map(String::from);
        assert_eq!(valid_hosts(&list), ["unpkg.com", "cdn.jsdelivr.net"]);
    }
}
