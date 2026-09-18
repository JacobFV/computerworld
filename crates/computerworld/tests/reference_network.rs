//! WP-0 gate for the simulated internet: the reference world's network section is well formed,
//! hermetic and reachable before any site service exists to be bound to it.
use computerworld::reference_world;
use cw_network::{Network, NetworkError};
use cw_protocol::HttpRequest;
use std::collections::BTreeSet;
use std::net::IpAddr;

/// IANA ranges reserved for documentation, plus the office LAN. No simulated host may name a
/// prefix that could ever belong to a real one.
const ALLOWED_PREFIXES: [&str; 4] = ["203.0.113.", "198.51.100.", "192.0.2.", "10.0."];

fn documented(address: &str) -> bool {
    ALLOWED_PREFIXES.iter().any(|p| address.starts_with(p))
}

#[test]
fn world_validates_with_hermetic_addressing_and_resolvable_aliases() {
    let world = reference_world();
    world.validate().unwrap();
    assert!(
        world.network.routes.is_empty(),
        "a single route rule blackholes every destination it does not match"
    );
    assert!(
        world.network.gateway.allow_internet && !world.network.gateway.allow_host,
        "the simulated internet is reachable and the real one is not"
    );
    let mut addresses = BTreeSet::new();
    for node in &world.network.nodes {
        assert!(
            documented(&node.address),
            "{} uses {}",
            node.id,
            node.address
        );
        assert!(addresses.insert(node.address.clone()), "{}", node.address);
    }
    let mut names = BTreeSet::new();
    for record in &world.network.dns {
        assert!(
            names.insert(record.name.to_ascii_lowercase()),
            "duplicate DNS name {}",
            record.name
        );
    }
    for record in &world.network.dns {
        // A record: the address must belong to a declared node, or nothing listens there ever.
        // CNAME: the target must itself be a declared name, or the alias is a dead end.
        if record.address.parse::<IpAddr>().is_ok() {
            assert!(
                addresses.contains(&record.address),
                "{} points at undeclared address {}",
                record.name,
                record.address
            );
        } else {
            assert!(
                names.contains(&record.address.to_ascii_lowercase()),
                "{} aliases unknown name {}",
                record.name,
                record.address
            );
        }
    }
}

#[test]
fn every_declared_name_routes_from_alice_mac() {
    let world = reference_world();
    let mut network = Network::with_seed(&world, 7).unwrap();
    for record in &world.network.dns {
        let request = HttpRequest::get(format!("http://{}/", record.name));
        match network.prepare_http("alice-mac", &request, 0) {
            // Bound already, or reachable and still waiting for the package that owns the site.
            Ok(_) | Err(NetworkError::Refused(_)) => {}
            Err(e) => panic!("{} is unreachable from alice-mac: {e}", record.name),
        }
    }
}
