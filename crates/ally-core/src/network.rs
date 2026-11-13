//! What kind of network an address belongs to. The Agent uses it to label pairing codes and
//! the Client uses it to decide which address to try first, so both read addresses alike.

use std::net::IpAddr;

/// Declared in order of preference, so sorting by it puts the best path first. Judged from
/// the address itself rather than from how it was found, because an Agent routing everything
/// over a VPN would otherwise be mislabeled.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Network {
    ThisMachine,
    Local,
    Other,
    Tailnet,
}

impl Network {
    /// Host names cannot be judged without a lookup, so they count as `Other`.
    pub fn of(host: &str) -> Network {
        let Ok(ip) = host.parse::<IpAddr>() else {
            return Network::Other;
        };
        let IpAddr::V4(v4) = ip else {
            return match ip.is_loopback() {
                true => Network::ThisMachine,
                false => Network::Other,
            };
        };
        let [first, second, ..] = v4.octets();

        match () {
            _ if v4.is_loopback() => Network::ThisMachine,
            _ if first == 100 && (64..128).contains(&second) => Network::Tailnet,
            _ if v4.is_private() => Network::Local,
            _ => Network::Other,
        }
    }

    /// How a path over this network is named in output, as in "via tailnet".
    pub fn name(self) -> &'static str {
        match self {
            Network::ThisMachine => "this machine",
            Network::Local => "local network",
            Network::Other => "network",
            Network::Tailnet => "tailnet",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Tailscale hands out 100.64.0.0 to 100.127.255.255. The rest of 100.x is public.
    #[test]
    fn each_address_is_judged_at_the_edges_of_its_range() {
        let cases = [
            ("127.0.0.1", Network::ThisMachine),
            ("127.255.255.254", Network::ThisMachine),
            ("::1", Network::ThisMachine),
            ("100.64.0.0", Network::Tailnet),
            ("100.67.90.119", Network::Tailnet),
            ("100.127.255.255", Network::Tailnet),
            ("100.63.255.255", Network::Other),
            ("100.128.0.0", Network::Other),
            ("10.0.0.193", Network::Local),
            ("172.16.0.1", Network::Local),
            ("172.31.255.255", Network::Local),
            ("172.32.0.1", Network::Other),
            ("192.168.1.20", Network::Local),
            ("8.8.8.8", Network::Other),
            ("fd7a:115c:a1e0::1", Network::Other),
            ("archbox.local", Network::Other),
            ("", Network::Other),
        ];
        for (host, network) in cases {
            assert_eq!(Network::of(host), network, "{host:?}");
        }
    }

    #[test]
    fn the_local_network_is_preferred_over_the_tailnet() {
        let mut networks = [
            Network::Tailnet,
            Network::Other,
            Network::Local,
            Network::ThisMachine,
        ];
        networks.sort();
        assert_eq!(
            networks,
            [
                Network::ThisMachine,
                Network::Local,
                Network::Other,
                Network::Tailnet
            ]
        );
    }
}
