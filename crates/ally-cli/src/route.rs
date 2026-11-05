//! How to reach an Agent. Every saved address is probed at once and the most preferred one
//! that answers wins, so an unreachable home address costs one short timeout rather than
//! one per candidate. When none answers, iroh reaches the Agent from any network.

use ally_core::config::{Agent, Config};
use ally_core::network::Network;
use ally_core::presentation::{Style, Tone};
use ally_core::tunnel;
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::Mutex;
use std::time::Duration;

const PROBE_TIMEOUT: Duration = Duration::from_millis(800);
const SSH_PORT: u16 = 22;

/// Set by the Client for the rsync helper it starts, so that process skips probing.
pub const ROUTE_ENV: &str = "ALLY_RSH_HOST";

const IROH: &str = "iroh";

#[derive(Debug, Clone, PartialEq)]
pub enum Route {
    Direct { host: String, network: Network },
    Iroh { key: String },
}

impl Route {
    fn to(host: &str) -> Route {
        Route::Direct {
            host: host.to_string(),
            network: Network::of(host),
        }
    }

    /// How the path is named in output, as in "via tailnet".
    pub fn name(&self) -> &'static str {
        match self {
            Route::Direct { network, .. } => network.name(),
            Route::Iroh { .. } => IROH,
        }
    }

    /// What the rsync helper is told, so it takes the same path.
    pub fn token(&self) -> &str {
        match self {
            Route::Direct { host, .. } => host,
            Route::Iroh { .. } => IROH,
        }
    }
}

/// One process talks to one Agent, so the first answer is kept for every later ssh call.
static CHOSEN: Mutex<Option<(String, Route)>> = Mutex::new(None);

/// The path to take to `agent`. When nothing answers and the Agent has no iroh key, the
/// pairing address is returned so ssh reports the failure in its own words.
pub fn resolve(agent: &Agent) -> Route {
    let mut chosen = CHOSEN.lock().expect("route lock was poisoned");
    if let Some((name, route)) = chosen.as_ref()
        && *name == agent.name
    {
        return route.clone();
    }
    let route = probe(agent);
    *chosen = Some((agent.name.clone(), route.clone()));
    route
}

/// Probe again on the next `resolve`. A process that outlives one network, such as the
/// menu bar helper, calls this after losing the Agent so a new network gets a new path.
pub fn forget() {
    *CHOSEN.lock().expect("route lock was poisoned") = None;
}

/// Take the path a parent process already chose, from its `token`. Ignored unless it is
/// one of the Agent's own paths.
pub fn assume(agent: &Agent, token: &str) {
    let route = match (token, iroh_key(agent)) {
        (IROH, Some(key)) => Route::Iroh { key },
        (host, _) if agent.candidates().contains(&host) => Route::to(host),
        _ => return,
    };
    *CHOSEN.lock().expect("route lock was poisoned") = Some((agent.name.clone(), route));
}

/// Take the path `--via` names for every later call, after checking once that it works,
/// so a chosen path that fails says so instead of falling back to another.
pub fn force(requested: Option<&str>, via: &str) -> anyhow::Result<()> {
    let config = Config::load()?;
    let agent = config.resolve(requested)?;
    let route = off_runtime(|| forced(agent, via))?;
    *CHOSEN.lock().expect("route lock was poisoned") = Some((agent.name.clone(), route));
    Ok(())
}

fn forced(agent: &Agent, via: &str) -> anyhow::Result<Route> {
    let name = &agent.name;
    if via == IROH {
        return iroh_key(agent)
            .map(|key| Route::Iroh { key })
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "{name} has no iroh key saved. Run ally link again, or leave out --via"
                )
            });
    }
    let (label, hint) = (label(via), hint(via));
    let Some(host) = address(agent, via) else {
        anyhow::bail!(
            "{name} has no {label} address saved. Run ally link again while both machines are on it, or leave out --via"
        );
    };
    match answers(host, agent.port.unwrap_or(SSH_PORT)) {
        true => Ok(Route::to(host)),
        false => anyhow::bail!("{name} did not answer via {label}. {hint}, or leave out --via"),
    }
}

/// The saved address on the network `via` names, which must not be iroh. Local means any
/// direct address off the tailnet, including a host name such as `archbox.local`.
fn address<'a>(agent: &'a Agent, via: &str) -> Option<&'a str> {
    let wanted: &[Network] = match via {
        "tailnet" => &[Network::Tailnet],
        _ => &[Network::ThisMachine, Network::Local, Network::Other],
    };
    agent
        .candidates()
        .into_iter()
        .find(|host| wanted.contains(&Network::of(host)))
}

/// Whether linking saved an address or key for the path `via` names.
pub fn saved(agent: &Agent, via: &str) -> bool {
    match via {
        IROH => iroh_key(agent).is_some(),
        _ => address(agent, via).is_some(),
    }
}

/// How the path `via` names is written in output, as in "via local network".
pub fn label(via: &str) -> &'static str {
    match via {
        IROH => IROH,
        "tailnet" => Network::Tailnet.name(),
        _ => Network::Local.name(),
    }
}

/// The first thing to check when the path `via` names does not answer.
pub fn hint(via: &str) -> &'static str {
    match via {
        IROH => "Make sure the Agent is on and ally start runs there, and this machine is online",
        "tailnet" => "Check Tailscale runs on both machines",
        _ => "Check both machines are on the same network",
    }
}

/// Probing waits on the network for up to a second.
fn probe(agent: &Agent) -> Route {
    off_runtime(|| probe_now(agent))
}

/// Run `work`, which waits on the network. Inside the async runtime the wait is handed
/// off, so other tasks on this worker keep running meanwhile.
pub fn off_runtime<T>(work: impl FnOnce() -> T) -> T {
    use tokio::runtime::{Handle, RuntimeFlavor};

    match Handle::try_current() {
        Ok(handle) if handle.runtime_flavor() == RuntimeFlavor::MultiThread => {
            tokio::task::block_in_place(work)
        }
        _ => work(),
    }
}

fn probe_now(agent: &Agent) -> Route {
    let candidates = agent.candidates();
    let key = iroh_key(agent);
    if let ([only], None) = (candidates.as_slice(), &key) {
        return Route::to(only);
    }
    let port = agent.port.unwrap_or(SSH_PORT);
    let attempts: Vec<_> = candidates
        .iter()
        .map(|host| {
            let host = host.to_string();
            std::thread::spawn(move || answers(&host, port))
        })
        .collect();
    for (host, attempt) in candidates.iter().zip(attempts) {
        if attempt.join().unwrap_or(false) {
            return Route::to(host);
        }
    }
    match key {
        Some(key) => Route::Iroh { key },
        None => Route::to(&agent.host),
    }
}

/// Whether an address ranked above `current` answers now, so a process that outlives one
/// network, such as the menu bar helper, can move to the better path once it is back.
/// Only those better addresses are probed. It waits on the network, so call it off the
/// async runtime.
pub fn better_than(agent: &Agent, current: &Route) -> bool {
    let port = agent.port.unwrap_or(SSH_PORT);
    let attempts: Vec<_> = ahead_of(&agent.candidates(), current)
        .into_iter()
        .map(|host| {
            let host = host.to_string();
            std::thread::spawn(move || answers(&host, port))
        })
        .collect();
    attempts
        .into_iter()
        .any(|attempt| attempt.join().unwrap_or(false))
}

/// The addresses ranked above `current`: all of them when on iroh, which ranks last.
fn ahead_of<'a>(candidates: &[&'a str], current: &Route) -> Vec<&'a str> {
    match current {
        Route::Direct { host, .. } => candidates
            .iter()
            .take_while(|candidate| **candidate != host)
            .copied()
            .collect(),
        Route::Iroh { .. } => candidates.to_vec(),
    }
}

/// The Agent's iroh key, if pairing saved one that parses. Checked here so nothing
/// malformed ever reaches ssh's ProxyCommand.
fn iroh_key(agent: &Agent) -> Option<String> {
    agent
        .iroh
        .clone()
        .filter(|key| tunnel::public_key(key).is_ok())
}

/// The `▶` line every remote command starts with, saying which Agent does the work and by
/// which path, such as `▶ Running on archbox via tailnet`.
pub fn announce(action: &str, name: &str, path: &str, style: Style) -> String {
    format!(
        "{} {action} {} via {}",
        style.paint("▶", Tone::Info),
        style.paint(name, Tone::Info),
        style.paint(path, Tone::Info)
    )
}

fn answers(host: &str, port: u16) -> bool {
    let Ok(addresses) = (host, port).to_socket_addrs() else {
        return false;
    };
    addresses
        .into_iter()
        .any(|address| TcpStream::connect_timeout(&address, PROBE_TIMEOUT).is_ok())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    #[test]
    fn the_announcement_names_the_agent_and_the_path() {
        assert_eq!(
            announce("Installing on", "archbox", "iroh", Style::new(false)),
            "▶ Installing on archbox via iroh"
        );
    }

    const KEY: &str = "ae58ff8833241ac82d6ff7611046ed67b5072d142c588d0063e942d9a75502b6";

    fn agent(host: &str, addresses: &[&str], port: Option<u16>) -> Agent {
        with_iroh(host, addresses, port, None)
    }

    fn with_iroh(host: &str, addresses: &[&str], port: Option<u16>, key: Option<&str>) -> Agent {
        Agent {
            name: "archbox".to_string(),
            host: host.to_string(),
            user: "me".to_string(),
            port,
            daemon_port: None,
            identity_file: None,
            known_hosts: None,
            program: None,
            addresses: addresses.iter().map(|a| a.to_string()).collect(),
            iroh: key.map(str::to_string),
            client: None,
            specs: None,
        }
    }

    #[test]
    fn the_first_preferred_address_that_answers_wins() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let target = agent("192.0.2.1", &["127.0.0.1"], Some(port));

        assert_eq!(probe(&target), Route::to("127.0.0.1"));
    }

    #[test]
    fn only_addresses_ranked_above_the_current_path_are_better() {
        let candidates = ["10.0.0.5", "100.64.0.9"];
        assert_eq!(
            ahead_of(&candidates, &Route::to("100.64.0.9")),
            ["10.0.0.5"]
        );
        assert!(ahead_of(&candidates, &Route::to("10.0.0.5")).is_empty());
        let iroh = Route::Iroh {
            key: KEY.to_string(),
        };
        assert_eq!(ahead_of(&candidates, &iroh), candidates);
    }

    #[test]
    fn a_better_path_is_found_once_it_answers() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let target = with_iroh("127.0.0.1", &[], Some(port), Some(KEY));
        let iroh = Route::Iroh {
            key: KEY.to_string(),
        };
        assert!(better_than(&target, &iroh));
        assert!(!better_than(&target, &Route::to("127.0.0.1")));
        drop(listener);
        assert!(!better_than(&target, &iroh));
    }

    #[test]
    fn with_nothing_answering_the_pairing_address_is_used() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        let target = agent("192.0.2.1", &["127.0.0.1"], Some(port));

        assert_eq!(probe(&target), Route::to("192.0.2.1"));
    }

    #[test]
    fn a_single_address_is_used_without_probing() {
        let target = agent("192.0.2.1", &[], None);

        assert_eq!(probe(&target), Route::to("192.0.2.1"));
    }

    #[test]
    fn with_nothing_answering_iroh_is_used_when_the_agent_has_a_key() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        let target = with_iroh("127.0.0.1", &[], Some(port), Some(KEY));

        assert_eq!(
            probe(&target),
            Route::Iroh {
                key: KEY.to_string()
            }
        );
    }

    #[test]
    fn probing_works_from_every_kind_of_runtime() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let target = agent("192.0.2.1", &["127.0.0.1"], Some(port));
        let expected = Route::to("127.0.0.1");

        let threads = tokio::runtime::Builder::new_multi_thread().build().unwrap();
        assert_eq!(threads.block_on(async { probe(&target) }), expected);
        let single = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        assert_eq!(single.block_on(async { probe(&target) }), expected);
    }

    #[test]
    fn via_iroh_skips_a_direct_address_that_answers() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let target = with_iroh("127.0.0.1", &[], Some(port), Some(KEY));

        assert_eq!(
            forced(&target, "iroh").unwrap(),
            Route::Iroh {
                key: KEY.to_string()
            }
        );
    }

    #[test]
    fn via_takes_the_address_on_the_chosen_network() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let target = agent("100.64.0.9", &["127.0.0.1"], Some(port));

        assert_eq!(forced(&target, "local").unwrap(), Route::to("127.0.0.1"));
    }

    #[test]
    fn via_local_takes_a_host_name_but_not_a_tailnet_address() {
        let named = agent("archbox.local", &["100.64.0.9"], None);
        assert_eq!(address(&named, "local"), Some("archbox.local"));
        assert_eq!(address(&named, "tailnet"), Some("100.64.0.9"));
        let tailnet_only = agent("100.64.0.9", &[], None);
        assert!(!saved(&tailnet_only, "local"));
        assert!(saved(&tailnet_only, "tailnet"));
        assert!(!saved(&tailnet_only, "iroh"));
    }

    #[test]
    fn via_a_path_the_agent_lacks_says_how_to_fix_it() {
        let target = agent("192.168.1.20", &[], None);
        assert_eq!(
            forced(&target, "tailnet").unwrap_err().to_string(),
            "archbox has no tailnet address saved. Run ally link again while both machines are on it, or leave out --via"
        );
        assert_eq!(
            forced(&target, "iroh").unwrap_err().to_string(),
            "archbox has no iroh key saved. Run ally link again, or leave out --via"
        );
    }

    #[test]
    fn via_a_path_that_does_not_answer_fails_instead_of_falling_back() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        let target = with_iroh("127.0.0.1", &[], Some(port), Some(KEY));

        assert_eq!(
            forced(&target, "local").unwrap_err().to_string(),
            "archbox did not answer via local network. Check both machines are on the same network, or leave out --via"
        );
    }

    #[test]
    fn a_direct_address_still_beats_iroh() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let target = with_iroh("127.0.0.1", &[], Some(port), Some(KEY));

        assert_eq!(probe(&target), Route::to("127.0.0.1"));
    }
}
