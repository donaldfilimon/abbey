//! `Origin` and `Host` validation — the DNS-rebinding defense.
//!
//! ## The attack this exists to stop
//!
//! A loopback HTTP server is reachable from any web page the user visits: the
//! page resolves an attacker-controlled name to `127.0.0.1` and issues requests
//! to it from the victim's browser. Binding to loopback does **not** prevent
//! this — it is precisely the attack loopback MCP servers face.
//!
//! Two independent checks close it, and both must pass:
//!
//! * **`Host`** must name a loopback address. A rebound request carries the
//!   attacker's hostname in `Host`, so this rejects it even when no `Origin` is
//!   sent. This is the load-bearing check.
//! * **`Origin`**, *when present*, must be a loopback origin. Browsers send it
//!   on every cross-origin request, so this rejects a page that somehow reached
//!   us with a correct `Host`.
//!
//! ## Why a missing `Origin` is allowed
//!
//! Native MCP clients (and `curl`) send no `Origin` at all. Requiring one would
//! reject every non-browser client while adding nothing: a browser cannot omit
//! it on a cross-origin request. The `Host` check is what covers that case.
//!
//! ## Matching is parsed, never substring
//!
//! `http://127.0.0.1.evil.com` and `http://localhost.evil.com` both *contain*
//! a loopback-looking prefix and are both rejected, because the authority is
//! parsed (via `url`) and compared structurally.

use std::net::IpAddr;

use url::{Host, Url};

/// Why an authority check failed, for a `403` body that names the real reason.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthorityRejection {
    /// No `Host` header at all — HTTP/1.1 requires one.
    MissingHost,
    /// `Host` names something other than a loopback address.
    NonLoopbackHost,
    /// `Origin` is present and is not a loopback origin.
    CrossOrigin,
}

impl AuthorityRejection {
    pub fn message(self) -> &'static str {
        match self {
            Self::MissingHost => "missing Host header",
            Self::NonLoopbackHost => {
                "Host header does not name a loopback address (DNS-rebinding defense)"
            }
            Self::CrossOrigin => "Origin header is not a loopback origin (DNS-rebinding defense)",
        }
    }
}

/// True when a parsed URL host is a loopback identity.
///
/// `localhost` is accepted by name because it is reserved to resolve to a
/// loopback address; any other domain is rejected without resolving it, so a
/// name that *currently* points at `127.0.0.1` still fails.
fn parsed_host_is_loopback(host: &Host<&str>) -> bool {
    match host {
        Host::Domain(name) => name.eq_ignore_ascii_case("localhost"),
        Host::Ipv4(address) => address.is_loopback(),
        Host::Ipv6(address) => address.is_loopback(),
    }
}

/// Validate a `Host` header value (`name`, `name:port`, or `[v6]:port`).
#[must_use]
pub fn host_header_is_loopback(value: &str) -> bool {
    let value = value.trim();
    if value.is_empty() {
        return false;
    }
    // A bracketed IPv6 literal must be split on the closing bracket, not on the
    // last colon — `[::1]:8787` has colons inside the address.
    let authority = if let Some(rest) = value.strip_prefix('[') {
        let Some((inside, tail)) = rest.split_once(']') else {
            return false;
        };
        if !tail.is_empty()
            && !(tail.starts_with(':') && tail[1..].bytes().all(|byte| byte.is_ascii_digit()))
        {
            return false;
        }
        return inside.parse::<IpAddr>().is_ok_and(|ip| ip.is_loopback());
    } else if let Some((name, port)) = value.rsplit_once(':') {
        if port.is_empty() || !port.bytes().all(|byte| byte.is_ascii_digit()) {
            // A bare unbracketed IPv6 literal is not legal in a Host header.
            return false;
        }
        name
    } else {
        value
    };
    if authority.eq_ignore_ascii_case("localhost") {
        return true;
    }
    authority
        .parse::<IpAddr>()
        .is_ok_and(|address| address.is_loopback())
}

/// Validate an `Origin` header value.
///
/// The scheme must be `http`. Nothing here terminates TLS, so an `https`
/// loopback origin cannot be a page this server produced, and `null` (a
/// sandboxed iframe or `file://` document) is not a loopback origin at all.
#[must_use]
pub fn origin_is_loopback(value: &str) -> bool {
    let value = value.trim();
    if value.is_empty() || value.eq_ignore_ascii_case("null") {
        return false;
    }
    let Ok(url) = Url::parse(value) else {
        return false;
    };
    if url.scheme() != "http" {
        return false;
    }
    url.host()
        .is_some_and(|host| parsed_host_is_loopback(&host))
}

/// Full authority check for one request.
pub fn check(host: Option<&str>, origin: Option<&str>) -> Result<(), AuthorityRejection> {
    let Some(host) = host else {
        return Err(AuthorityRejection::MissingHost);
    };
    if !host_header_is_loopback(host) {
        return Err(AuthorityRejection::NonLoopbackHost);
    }
    if let Some(origin) = origin
        && !origin_is_loopback(origin)
    {
        return Err(AuthorityRejection::CrossOrigin);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loopback_authorities_are_accepted_in_every_legal_spelling() {
        for host in [
            "127.0.0.1",
            "127.0.0.1:8787",
            "127.0.0.53",
            "localhost",
            "LocalHost:8787",
            "[::1]",
            "[::1]:8787",
        ] {
            assert!(host_header_is_loopback(host), "Host `{host}` must pass");
        }
        for origin in [
            "http://127.0.0.1:8787",
            "http://localhost:8787",
            "http://localhost",
            "http://[::1]:8787",
        ] {
            assert!(origin_is_loopback(origin), "Origin `{origin}` must pass");
        }
    }

    #[test]
    fn hostile_authorities_are_rejected_including_lookalike_prefixes() {
        for host in [
            "",
            "evil.com",
            "evil.com:8787",
            "127.0.0.1.evil.com",
            "localhost.evil.com",
            "192.168.1.5:8787",
            "0.0.0.0",
            "::1",
            "[::1]:notaport",
        ] {
            assert!(
                !host_header_is_loopback(host),
                "Host `{host}` must be rejected"
            );
        }
        for origin in [
            "http://evil.com",
            "http://127.0.0.1.evil.com",
            "http://localhost.evil.com",
            "http://xn--localhost-vg9d.com",
            "https://127.0.0.1:8787",
            "null",
            "file://",
            "not a url",
        ] {
            assert!(
                !origin_is_loopback(origin),
                "Origin `{origin}` must be rejected"
            );
        }
    }

    #[test]
    fn a_missing_origin_is_allowed_but_a_missing_host_is_not() {
        assert_eq!(check(Some("127.0.0.1:8787"), None), Ok(()));
        assert_eq!(check(None, None), Err(AuthorityRejection::MissingHost));
        assert_eq!(
            check(Some("evil.com"), None),
            Err(AuthorityRejection::NonLoopbackHost)
        );
        assert_eq!(
            check(Some("127.0.0.1:8787"), Some("http://evil.com")),
            Err(AuthorityRejection::CrossOrigin)
        );
        assert_eq!(
            check(Some("127.0.0.1:8787"), Some("http://127.0.0.1:8787")),
            Ok(())
        );
    }
}
