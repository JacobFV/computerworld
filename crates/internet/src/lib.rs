//! The built-in internet: `worlds/internet/world.json`, joined into every world that does not
//! set `internet: false`.
//!
//! A world declares only what is its own — its machines, its LAN, its internal services and
//! any public site it runs — and names the internet's [`INTERNET_ATTACHMENTS`] where it
//! hangs off them. [`join`] adds the rest. What the world declares wins: a service, node or
//! DNS name the world already has is never replaced, which is what lets a world stand its
//! own `google.com` in place of the built-in one.
use cw_protocol::{NetworkLink, Result, SimError, WorldDefinition, INTERNET_ATTACHMENTS};
use std::collections::BTreeSet;
use std::sync::OnceLock;

/// The router a world's machines uplink through when the world does not say how.
pub const UPLINK: &str = INTERNET_ATTACHMENTS[0];
/// The uplink a world gets when it names none: a home or office line to the ISP.
const UPLINK_LATENCY_US: u64 = 3000;

/// The internet as its blueprint resolves, parsed once.
pub fn definition() -> &'static WorldDefinition {
    static PARSED: OnceLock<WorldDefinition> = OnceLock::new();
    PARSED.get_or_init(|| {
        WorldDefinition::from_json(include_str!("../../../worlds/internet/world.json"))
            .expect("the packaged internet is a valid world")
    })
}

/// Join the internet into `world`, unless it set `internet: false`. Joining twice changes
/// nothing, so a definition taken from a running world can be booted again.
pub fn join(world: &mut WorldDefinition) -> Result<()> {
    if !world.internet {
        return Ok(());
    }
    let internet = definition();
    let ids: BTreeSet<String> = world.services.iter().map(|s| s.id.clone()).collect();
    let domains: BTreeSet<String> = world
        .services
        .iter()
        .flat_map(|s| s.domains.iter().map(|d| d.to_ascii_lowercase()))
        .collect();
    for service in &internet.services {
        let taken = ids.contains(&service.id)
            || service
                .domains
                .iter()
                .any(|d| domains.contains(&d.to_ascii_lowercase()));
        if !taken {
            world.services.push(service.clone());
        }
    }
    let network = &mut world.network;
    // Asked of the world's own links, before the internet's (which all touch the router) land.
    let uplinked = network
        .links
        .iter()
        .any(|l| l.from == UPLINK || l.to == UPLINK);
    let nodes: BTreeSet<String> = network.nodes.iter().map(|n| n.id.clone()).collect();
    for node in &internet.network.nodes {
        if !nodes.contains(&node.id) {
            network.nodes.push(node.clone());
        }
    }
    // The internet's links go ahead of the world's. A path is the fewest hops, ties broken
    // by link order, so this keeps traffic on the backbone rather than letting it transit
    // whatever server a world happens to have linked to a point of presence.
    let own = std::mem::take(&mut network.links);
    network.links = internet
        .network
        .links
        .iter()
        .filter(|l| !own.contains(l))
        .cloned()
        .collect();
    network.links.extend(own);
    // A domain the world serves itself is the world's to resolve, not the internet's.
    let names: BTreeSet<String> = network
        .dns
        .iter()
        .map(|r| r.name.trim_end_matches('.').to_ascii_lowercase())
        .chain(domains)
        .collect();
    for record in &internet.network.dns {
        if !names.contains(&record.name.trim_end_matches('.').to_ascii_lowercase()) {
            network.dns.push(record.clone());
        }
    }
    if !uplinked {
        for computer in &world.computers {
            network.links.push(NetworkLink {
                from: computer.node_id().to_owned(),
                to: UPLINK.to_owned(),
                bidirectional: true,
                latency_us: UPLINK_LATENCY_US,
                loss_per_million: 0,
            });
        }
    }
    world
        .validate()
        .map_err(|e| SimError::invalid(format!("world {} with the internet joined: {e}", world.id)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lab(internet: bool) -> WorldDefinition {
        let mut world =
            WorldDefinition::from_json(include_str!("../../../worlds/unrelated-lab/world.json"))
                .unwrap();
        world.internet = internet;
        // The lab sits on a documentation address the backbone uses; move it onto the LAN.
        world.computers[0].address = "10.0.0.5".into();
        world
    }

    #[test]
    fn the_attachments_are_the_internets_own_nodes() {
        let nodes: BTreeSet<&str> = definition()
            .network
            .nodes
            .iter()
            .map(|n| n.id.as_str())
            .collect();
        for attachment in INTERNET_ATTACHMENTS {
            assert!(
                nodes.contains(attachment),
                "{attachment} is not in the internet"
            );
        }
        assert!(!definition().internet, "the internet does not join itself");
    }

    #[test]
    fn a_world_joins_the_internet_through_an_uplink_and_can_opt_out() {
        let mut offline = lab(false);
        let before = offline.clone();
        join(&mut offline).unwrap();
        assert_eq!(
            offline, before,
            "internet: false is exactly what the world declares"
        );

        let mut online = lab(true);
        join(&mut online).unwrap();
        assert_eq!(
            online.services.len(),
            before.services.len() + definition().services.len()
        );
        let node = online.computers[0].node_id().to_owned();
        assert!(online
            .network
            .links
            .iter()
            .any(|l| l.from == node && l.to == UPLINK));
        let again = online.clone();
        join(&mut online).unwrap();
        assert_eq!(online, again, "joining twice changes nothing");
    }

    #[test]
    fn what_a_world_declares_wins() {
        let mut world = lab(true);
        let mut own = definition().services[0].clone();
        own.id = "my-own".into();
        own.node = world.computers[0].node_id().to_owned();
        own.search_entries.clear();
        world.services.push(own.clone());
        join(&mut world).unwrap();
        assert!(!world
            .services
            .iter()
            .any(|s| s.id == definition().services[0].id));
        assert_eq!(
            world
                .services
                .iter()
                .filter(|s| s.domains == own.domains)
                .count(),
            1
        );
    }
}
