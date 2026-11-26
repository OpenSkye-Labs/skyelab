//! A page that didn't load, as the pane explains it (T3's "This site can't be reached"): the engine's error turned
//! into a host, a plain reason and, for a local address, a hint that no dev server is answering.

use gpui::SharedString;

/// A failed load the pane shows in place of the page.
#[derive(Debug, Clone, PartialEq)]
pub struct LoadError {
    /// "This site can't be reached", or "Nothing is running at localhost:3000" for a local address that refused.
    pub title: SharedString,
    /// The address that failed; what Retry loads again. Empty when the engine didn't name it.
    pub url: String,
    /// `localhost:3000`, or the whole URL when it has no host.
    pub host: SharedString,
    /// "the connection was refused".
    pub reason: SharedString,
    /// A next step, when there is an obvious one.
    pub hint: Option<SharedString>,
    /// The engine's own words and code, for a bug report.
    pub detail: SharedString,
}

/// `None` for a load that didn't fail for the user: one cancelled by the next navigation, a download, or a page the
/// navigation policy turned away (the old page stays).
pub fn describe(domain: &str, code: isize, description: &str, url: &str) -> Option<LoadError> {
    let reason = match (domain, code) {
        ("NSURLErrorDomain", -999) => return None, // cancelled: another load took over
        ("WebKitErrorDomain", 102 | 204) => return None, // interrupted by policy (a download) / handled by a plug-in
        ("NSURLErrorDomain", -1004) => "the connection was refused",
        ("NSURLErrorDomain", -1003) => "its address couldn't be found",
        ("NSURLErrorDomain", -1001) => "it took too long to answer",
        ("NSURLErrorDomain", -1009) => "this Mac is offline",
        ("NSURLErrorDomain", -1005) => "the connection was lost",
        ("NSURLErrorDomain", -1201..=-1200 | -1206..=-1203) => "the secure connection failed",
        ("NSURLErrorDomain", -1202) => "its certificate isn't trusted",
        ("NSURLErrorDomain", -1100) => "the file doesn't exist",
        ("WebKitErrorDomain", 100) => "this file type can't be shown here",
        ("NSURLErrorDomain", -1002) | ("WebKitErrorDomain", 101) => {
            "this kind of address can't be opened here"
        }
        _ if description.is_empty() => "the page didn't load",
        _ => description,
    };
    let parsed = url::Url::parse(url).ok();
    let host = parsed
        .as_ref()
        .and_then(|u| {
            let host = u.host_str()?;
            Some(match u.port() {
                Some(port) => format!("{host}:{port}"),
                None => host.to_owned(),
            })
        })
        .unwrap_or_else(|| url.to_owned());
    let host = if host.is_empty() {
        "This page".to_owned()
    } else {
        host
    };
    let local = parsed
        .as_ref()
        .and_then(|u| u.host())
        .is_some_and(|h| match h {
            url::Host::Domain(d) => d == "localhost" || d.ends_with(".localhost"),
            url::Host::Ipv4(ip) => ip.is_loopback() || ip.is_unspecified(),
            url::Host::Ipv6(ip) => ip.is_loopback() || ip.is_unspecified(),
        });
    let refused_here = local && code == -1004;
    let hint = match (local, code) {
        (true, -1004) => Some("Start your dev server, then press Retry.".to_owned()),
        (false, -1003 | -1009 | -1005 | -1001) => {
            Some("Check your connection, proxy and firewall.".to_owned())
        }
        _ => None,
    };
    Some(LoadError {
        title: if refused_here {
            format!("Nothing is running at {host}")
        } else {
            "This site can't be reached".to_owned()
        }
        .into(),
        url: url.to_owned(),
        host: host.into(),
        reason: reason.to_owned().into(),
        hint: hint.map(Into::into),
        detail: match description {
            "" => format!("{domain} {code}"),
            d => format!("{d} ({domain} {code})"),
        }
        .into(),
    })
}

/// The loopback addresses a local http(s) URL names (no DNS lookup, no I/O), or `None` for any other URL: what
/// [`refused_at`] checks before the URL loads.
pub fn local_addrs(url: &url::Url) -> Option<Vec<std::net::SocketAddr>> {
    use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
    if !matches!(url.scheme(), "http" | "https") {
        return None;
    }
    let ips: Vec<IpAddr> = match url.host()? {
        url::Host::Domain(d) if d == "localhost" || d.ends_with(".localhost") => {
            vec![Ipv4Addr::LOCALHOST.into(), Ipv6Addr::LOCALHOST.into()]
        }
        url::Host::Ipv4(ip) if ip.is_loopback() => vec![ip.into()],
        url::Host::Ipv6(ip) if ip.is_loopback() => vec![ip.into()],
        _ => return None,
    };
    let port = url.port_or_known_default()?;
    Some(
        ips.into_iter()
            .map(|ip| SocketAddr::new(ip, port))
            .collect(),
    )
}

/// The error page for `url` when every one of `addrs` refuses a connection (nothing listening); `None` when something
/// answers or the check can't tell (a timeout). Blocks up to 3 s per address: call it off the UI thread.
pub fn refused_at(addrs: &[std::net::SocketAddr], url: &url::Url) -> Option<LoadError> {
    // Windows retries the SYN to a closed loopback port for ~2 s before refusing; macOS and Linux refuse at once.
    let limit = std::time::Duration::from_secs(3);
    let all_refused = addrs.iter().all(|addr| {
        std::net::TcpStream::connect_timeout(addr, limit)
            .is_err_and(|e| e.kind() == std::io::ErrorKind::ConnectionRefused)
    });
    if !all_refused {
        return None;
    }
    let mut error = describe("NSURLErrorDomain", -1004, "", url.as_str())?;
    error.detail = "Connection refused".into();
    Some(error)
}

/// [`local_addrs`] then [`refused_at`], blocking: for tests and callers already off the UI thread.
pub fn refused_local(url: &url::Url) -> Option<LoadError> {
    refused_at(&local_addrs(url)?, url)
}

/// A link the engine can't open (`mailto:`, an app's scheme, a file type it can't show) fails like a load, but the
/// page it was clicked on is still there and fine: keep it rather than cover it with an error.
pub fn keeps_page(domain: &str, code: isize) -> bool {
    matches!(
        (domain, code),
        ("WebKitErrorDomain", 100 | 101) | ("NSURLErrorDomain", -1002)
    )
}

#[cfg(test)]
mod tests {
    use super::{describe, keeps_page, refused_local};

    #[test]
    fn refused_localhost_says_no_dev_server() {
        let e = describe(
            "NSURLErrorDomain",
            -1004,
            "Could not connect to the server.",
            "http://localhost:3000/",
        )
        .expect("a failure");
        assert_eq!(e.host, "localhost:3000");
        assert_eq!(e.reason, "the connection was refused");
        assert!(
            e.hint.as_deref().is_some_and(|h| h.contains("dev server")),
            "{e:?}"
        );
        assert_eq!(e.title, "Nothing is running at localhost:3000");
        assert!(e.detail.contains("-1004"), "{e:?}");
        assert_eq!(e.url, "http://localhost:3000/", "Retry loads it again");
        for url in [
            "http://127.0.0.1:5173",
            "http://[::1]:8080/",
            "http://app.localhost:4000",
        ] {
            let e = describe("NSURLErrorDomain", -1004, "", url).expect("a failure");
            assert!(e.hint.is_some(), "{url} is local");
        }
    }

    #[test]
    fn remote_failures_name_the_cause() {
        let dns = describe(
            "NSURLErrorDomain",
            -1003,
            "A server with the specified hostname could not be found.",
            "https://nope.invalid/x",
        )
        .expect("a failure");
        assert_eq!(dns.host, "nope.invalid");
        assert_eq!(dns.reason, "its address couldn't be found");
        assert!(
            dns.hint
                .as_deref()
                .is_some_and(|h| h.contains("connection"))
        );
        let refused = describe("NSURLErrorDomain", -1004, "", "https://example.com:8443/")
            .expect("a failure");
        assert_eq!(refused.hint, None, "no dev server hint off this machine");
        assert_eq!(refused.title, "This site can't be reached");
        let tls = describe(
            "NSURLErrorDomain",
            -1202,
            "",
            "https://self-signed.example/",
        )
        .expect("a failure");
        assert_eq!(tls.reason, "its certificate isn't trusted");
        let other = describe(
            "NSURLErrorDomain",
            -1234,
            "Something odd.",
            "https://a.example/",
        )
        .expect("a failure");
        assert_eq!(other.reason, "Something odd.");
        let blank = describe("X", 1, "", "").expect("a failure");
        assert_eq!(
            (blank.detail.as_ref(), blank.host.as_ref()),
            ("X 1", "This page")
        );
        assert!(keeps_page("WebKitErrorDomain", 101) && keeps_page("NSURLErrorDomain", -1002));
        assert!(!keeps_page("NSURLErrorDomain", -1004));
    }

    #[test]
    fn a_local_port_with_no_listener_is_refused_before_loading() {
        // A port that was just free: bound, then let go.
        let port = std::net::TcpListener::bind("127.0.0.1:0")
            .and_then(|l| l.local_addr())
            .expect("a free port")
            .port();
        for url in [
            format!("http://localhost:{port}/"),
            format!("http://127.0.0.1:{port}/x"),
        ] {
            let e = refused_local(&url::Url::parse(&url).expect("url")).expect("refused");
            assert_eq!(e.title, format!("Nothing is running at {}", e.host));
            assert_eq!(e.url, url, "Retry loads it again");
        }
        // Something listening: loaded as usual.
        let live = std::net::TcpListener::bind("127.0.0.1:0").expect("listen");
        let url = format!(
            "http://127.0.0.1:{}/",
            live.local_addr().expect("addr").port()
        );
        assert_eq!(refused_local(&url::Url::parse(&url).expect("url")), None);
        // Not local, or not http: never probed.
        for url in ["https://example.com:1/", "file:///tmp/x.html"] {
            assert_eq!(refused_local(&url::Url::parse(url).expect("url")), None);
        }
    }

    #[test]
    fn cancelled_and_policy_loads_are_not_errors() {
        assert_eq!(
            describe("NSURLErrorDomain", -999, "cancelled", "https://a.example/"),
            None
        );
        assert_eq!(
            describe(
                "WebKitErrorDomain",
                102,
                "Frame load interrupted",
                "https://a.example/f.zip"
            ),
            None
        );
    }
}
