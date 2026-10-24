//! Turning a published port into an address you can open
//! (owner's request, 2026-10-24).
//!
//! A port in a pane is only useful if clicking it reaches the thing. The
//! arithmetic is small but every part of it has a wrong answer that
//! looks right:
//!
//! - `0.0.0.0` and `::` mean *every* interface, not an address. Putting
//!   them in a URL gives `http://0.0.0.0:9443`, which works on some
//!   machines, fails on others, and is never what was meant.
//! - An IPv6 literal must be bracketed in a URL, and the engine already
//!   reports it bracketed in one form and bare in another.
//! - A port published on a **remote** engine is not on this machine, so
//!   `localhost` would quietly open the wrong thing — the most
//!   dangerous answer of the three, because it may well respond.
//! - Only TCP is openable. A UDP port has no URL.

use crate::model::Port;
use crate::profile::{ContainerFields, Transport};

/// Where the engine's published ports actually live, from the host's own
/// settings. The caller resolves an SSH profile's hostname and passes it
/// in, because this crate must not depend on `hs-ssh`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reachable {
    /// On this machine: a local socket, or an engine in WSL, whose
    /// published ports Windows forwards to localhost.
    Localhost,
    /// On another machine, at this hostname or address.
    Host(String),
    /// Nowhere this app can work out - an SSH profile that has been
    /// deleted, say. A port is then shown but not offered as a link,
    /// which is honest.
    Unknown,
}

impl Reachable {
    /// Where a host's ports are, from its transport.
    ///
    /// `ssh_host` is the hostname of the saved SSH profile, which only
    /// the caller can look up.
    pub fn of(fields: &ContainerFields, ssh_host: Option<&str>) -> Self {
        match fields.transport {
            // WSL forwards a published port to the Windows localhost,
            // which is why a WSL engine behaves like a local one here.
            Transport::Local | Transport::Wsl => Reachable::Localhost,
            Transport::Tcp => {
                if fields.host.trim().is_empty() {
                    Reachable::Unknown
                } else {
                    Reachable::Host(fields.host.trim().to_string())
                }
            }
            Transport::Ssh => match ssh_host {
                Some(h) if !h.trim().is_empty() => Reachable::Host(h.trim().to_string()),
                _ => Reachable::Unknown,
            },
        }
    }
}

/// Ports that are conventionally TLS. Not exhaustive and not meant to
/// be: a wrong guess costs one redirect, while refusing to guess costs
/// every HTTPS service a broken link.
const TLS_PORTS: &[u16] = &[443, 8443, 9443, 4443, 10443];

/// The address to open for a published port, or `None` when there is
/// nothing sensible to open.
///
/// `None` for an unpublished port (exposed only, nowhere to go), for a
/// protocol other than TCP, and for a host this app cannot name.
pub fn port_url(port: &Port, where_: &Reachable) -> Option<String> {
    let host_port = port.host?;
    if host_port == 0 {
        return None;
    }
    // `tcp` or unset; `udp` and `sctp` have no URL.
    let proto = port.protocol.trim().to_ascii_lowercase();
    if !proto.is_empty() && proto != "tcp" {
        return None;
    }
    let authority = authority(&port.host_ip, where_)?;
    let scheme = if TLS_PORTS.contains(&host_port) {
        "https"
    } else {
        "http"
    };
    Some(format!("{scheme}://{authority}:{host_port}"))
}

/// The host part of the URL: the published address when it names one,
/// otherwise where the engine is.
fn authority(host_ip: &str, where_: &Reachable) -> Option<String> {
    let ip = host_ip.trim();
    let wildcard = ip.is_empty() || ip == "0.0.0.0" || ip == "::" || ip == "[::]" || ip == "*";
    if wildcard {
        return match where_ {
            Reachable::Localhost => Some("localhost".to_string()),
            Reachable::Host(h) => Some(bracket_if_ipv6(h)),
            Reachable::Unknown => None,
        };
    }
    Some(bracket_if_ipv6(ip))
}

/// An IPv6 literal needs brackets in a URL, and must not be bracketed
/// twice when the engine already did it.
fn bracket_if_ipv6(host: &str) -> String {
    let h = host.trim();
    if h.starts_with('[') {
        return h.to_string();
    }
    // A hostname never contains a colon; an IPv6 literal always does.
    if h.contains(':') {
        return format!("[{h}]");
    }
    h.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn port(host: Option<u16>, host_ip: &str, container: u16, protocol: &str) -> Port {
        Port {
            container,
            host,
            host_ip: host_ip.to_string(),
            protocol: protocol.to_string(),
        }
    }

    /// `0.0.0.0` means every interface, not an address. A URL built from
    /// it works on some machines and not others, which is the worst kind
    /// of wrong.
    #[test]
    fn a_wildcard_address_becomes_localhost_for_a_local_engine() {
        let p = port(Some(9443), "0.0.0.0", 9443, "tcp");
        assert_eq!(
            port_url(&p, &Reachable::Localhost).as_deref(),
            Some("https://localhost:9443")
        );
        let bare = port(Some(8080), "", 80, "tcp");
        assert_eq!(
            port_url(&bare, &Reachable::Localhost).as_deref(),
            Some("http://localhost:8080")
        );
    }

    /// A port on a remote engine is not on this machine. Opening
    /// localhost would be the dangerous answer: something may well
    /// answer there.
    #[test]
    fn a_wildcard_on_a_remote_engine_uses_that_machine() {
        let p = port(Some(8080), "0.0.0.0", 80, "tcp");
        assert_eq!(
            port_url(&p, &Reachable::Host("web01.example.com".into())).as_deref(),
            Some("http://web01.example.com:8080")
        );
        // And when the machine cannot be named, nothing is offered.
        assert_eq!(port_url(&p, &Reachable::Unknown), None);
    }

    /// A specific published address is used as given, whatever the
    /// transport: the engine has already said where it listens.
    #[test]
    fn a_specific_address_is_kept() {
        let p = port(Some(5432), "127.0.0.1", 5432, "tcp");
        assert_eq!(
            port_url(&p, &Reachable::Host("elsewhere".into())).as_deref(),
            Some("http://127.0.0.1:5432")
        );
    }

    /// An IPv6 literal must be bracketed, and never bracketed twice.
    #[test]
    fn ipv6_is_bracketed_exactly_once() {
        let already = port(Some(9443), "[::1]", 9443, "tcp");
        assert_eq!(
            port_url(&already, &Reachable::Localhost).as_deref(),
            Some("https://[::1]:9443")
        );
        let bare = port(Some(8080), "fe80::1", 80, "tcp");
        assert_eq!(
            port_url(&bare, &Reachable::Localhost).as_deref(),
            Some("http://[fe80::1]:8080")
        );
        // `[::]` is the IPv6 wildcard, so it is an address of nothing.
        let wild = port(Some(8080), "[::]", 80, "tcp");
        assert_eq!(
            port_url(&wild, &Reachable::Localhost).as_deref(),
            Some("http://localhost:8080")
        );
        // A remote host given as an IPv6 literal is bracketed too.
        let p = port(Some(80), "0.0.0.0", 80, "tcp");
        assert_eq!(
            port_url(&p, &Reachable::Host("2001:db8::5".into())).as_deref(),
            Some("http://[2001:db8::5]:80")
        );
    }

    /// An exposed-but-unpublished port has nowhere to go, so it must
    /// not look like a link.
    #[test]
    fn an_unpublished_port_has_no_url() {
        let p = port(None, "", 8000, "tcp");
        assert_eq!(port_url(&p, &Reachable::Localhost), None);
        let zero = port(Some(0), "", 8000, "tcp");
        assert_eq!(port_url(&zero, &Reachable::Localhost), None);
    }

    #[test]
    fn only_tcp_is_openable() {
        let udp = port(Some(53), "0.0.0.0", 53, "udp");
        assert_eq!(port_url(&udp, &Reachable::Localhost), None);
        let sctp = port(Some(99), "0.0.0.0", 99, "sctp");
        assert_eq!(port_url(&sctp, &Reachable::Localhost), None);
        // An engine that omits the protocol means tcp.
        let bare = port(Some(80), "0.0.0.0", 80, "");
        assert!(port_url(&bare, &Reachable::Localhost).is_some());
    }

    /// The scheme is a guess, and the cost of each wrong answer is why
    /// it is made this way: a wrong `http` costs one redirect, a
    /// missing `https` costs a broken link on every TLS service.
    #[test]
    fn the_conventional_tls_ports_get_https() {
        for p in [443u16, 8443, 9443] {
            let got = port_url(&port(Some(p), "0.0.0.0", p, "tcp"), &Reachable::Localhost);
            assert!(
                got.as_deref().is_some_and(|u| u.starts_with("https://")),
                "{p} should be https, got {got:?}"
            );
        }
        for p in [80u16, 8080, 3000, 5432] {
            let got = port_url(&port(Some(p), "0.0.0.0", p, "tcp"), &Reachable::Localhost);
            assert!(
                got.as_deref().is_some_and(|u| u.starts_with("http://")),
                "{p} should be http, got {got:?}"
            );
        }
    }

    /// WSL forwards a published port to the Windows localhost, so a WSL
    /// engine is reachable the same way a local one is - which is the
    /// whole reason this distinction is drawn from the transport rather
    /// than guessed.
    #[test]
    fn where_a_hosts_ports_live_follows_its_transport() {
        let local = ContainerFields::default();
        assert_eq!(Reachable::of(&local, None), Reachable::Localhost);

        let wsl = ContainerFields {
            transport: Transport::Wsl,
            distro: "Ubuntu-24.04".into(),
            ..Default::default()
        };
        assert_eq!(Reachable::of(&wsl, None), Reachable::Localhost);

        let tcp = ContainerFields {
            transport: Transport::Tcp,
            host: "10.0.0.5".into(),
            ..Default::default()
        };
        assert_eq!(
            Reachable::of(&tcp, None),
            Reachable::Host("10.0.0.5".into())
        );

        let ssh = ContainerFields {
            transport: Transport::Ssh,
            tunnel_ssh: "web01".into(),
            ..Default::default()
        };
        assert_eq!(
            Reachable::of(&ssh, Some("web01.example.com")),
            Reachable::Host("web01.example.com".into())
        );
        // A deleted SSH profile: nothing can be named, so nothing is
        // offered rather than localhost being guessed.
        assert_eq!(Reachable::of(&ssh, None), Reachable::Unknown);
        assert_eq!(Reachable::of(&ssh, Some("  ")), Reachable::Unknown);
    }

    /// A TCP host with no hostname configured cannot be addressed.
    #[test]
    fn a_tcp_host_with_no_address_is_unknown() {
        let tcp = ContainerFields {
            transport: Transport::Tcp,
            host: "   ".into(),
            ..Default::default()
        };
        assert_eq!(Reachable::of(&tcp, None), Reachable::Unknown);
    }
}
