//! The built-in internet: `worlds/internet/world.json`, joined into every world that does not
//! set `internet: false`.
//!
//! A world declares only what is its own — its machines, its LAN, its internal services and
//! any public site it runs — and names the internet's [`INTERNET_ATTACHMENTS`] where it
//! hangs off them. [`join`] adds the rest. What the world declares wins: a service, node or
//! DNS name the world already has is never replaced, which is what lets a world stand its
//! own `google.com` in place of the built-in one. A world's `internet_overlays` add its own
//! people and content to the sites it shares — see [`merge`].
use cw_protocol::{
    NetworkLink, Result, ServiceDefinition, SimError, SiteOverlay, WorldDefinition,
    INTERNET_ATTACHMENTS,
};
use serde_json::Value;
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

/// Layer `overlay` onto `base`, as a JSON merge patch does: objects merge key by key, and
/// anything else — a sentence, a number, an array — is replaced by the overlay's. An overlay
/// adds a mailbox or a repository as a new key, and restates a list whole, so a paragraph,
/// a comment or a revision keeps its place in the order it had.
pub fn merge(base: &mut Value, overlay: &Value) {
    match (base, overlay) {
        (Value::Object(base), Value::Object(overlay)) => {
            for (key, value) in overlay {
                match base.get_mut(key) {
                    Some(existing) => merge(existing, value),
                    None => {
                        base.insert(key.clone(), value.clone());
                    }
                }
            }
        }
        (base, overlay) => *base = overlay.clone(),
    }
}

/// One of the internet's sites with a world's overlay applied.
pub fn overlaid(site: &ServiceDefinition, overlay: &SiteOverlay) -> ServiceDefinition {
    let mut site = site.clone();
    if site.initial_state.is_null() {
        site.initial_state = Value::Object(Default::default());
    }
    merge(&mut site.initial_state, &overlay.initial_state);
    site.search_entries
        .extend(overlay.search_entries.iter().cloned());
    site.domains.extend(overlay.domains.iter().cloned());
    site
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
    let mut overlays = std::mem::take(&mut world.internet_overlays);
    for service in &internet.services {
        let taken = ids.contains(&service.id)
            || service
                .domains
                .iter()
                .any(|d| domains.contains(&d.to_ascii_lowercase()));
        if !taken {
            world.services.push(match overlays.remove(&service.id) {
                Some(overlay) => overlaid(service, &overlay),
                None => service.clone(),
            });
        }
    }
    // An overlay is spent once applied, so joining twice cannot apply it twice.
    if let Some(site) = overlays.keys().next() {
        return Err(SimError::invalid(format!(
            "world {} overlays {site}, which is not an internet site it joins",
            world.id
        )));
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

    /// The internet every world shares carries none of the reference company's story: its
    /// people, company and product live in the company's `internet_overlays`.
    #[test]
    fn the_internet_is_neutral() {
        let text = include_str!("../../../worlds/internet/world.json").to_ascii_lowercase();
        let anywhere = [
            "northstar",
            "atlas",
            "alicechen",
            "bmartinez",
            "praman",
            "nakamura",
            "guide.example",
            ".internal",
        ];
        let words = [
            "alice", "bob", "carol", "priya", "raman", "nguyen", "okafor",
        ];
        let letter = |c: Option<char>| c.is_some_and(|c| c.is_ascii_alphabetic());
        let mut found = Vec::new();
        for term in anywhere {
            if let Some(at) = text.find(term) {
                found.push(&text[at.saturating_sub(40)..(at + 40).min(text.len())]);
            }
        }
        for word in words {
            for (at, _) in text.match_indices(word) {
                if !letter(text[..at].chars().next_back())
                    && !letter(text[at + word.len()..].chars().next())
                {
                    found.push(&text[at.saturating_sub(40)..(at + 40).min(text.len())]);
                    break;
                }
            }
        }
        assert!(
            found.is_empty(),
            "the internet mentions the reference company: {found:#?}"
        );
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
    fn an_overlay_adds_to_a_site_and_is_applied_once() {
        let site = &definition().services[0];
        let mut world = lab(true);
        world.internet_overlays.insert(
            site.id.clone(),
            SiteOverlay {
                initial_state: serde_json::json!({"overlay_note": "ours"}),
                search_entries: vec![serde_json::json!({"url": "http://x.test/", "title": "X"})],
                domains: vec!["ours.x.test".into()],
            },
        );
        join(&mut world).unwrap();
        let joined = world.services.iter().find(|s| s.id == site.id).unwrap();
        assert_eq!(joined.initial_state["overlay_note"], "ours");
        assert_eq!(joined.search_entries.len(), site.search_entries.len() + 1);
        assert_eq!(
            joined.domains.last().map(String::as_str),
            Some("ours.x.test")
        );
        assert!(world.internet_overlays.is_empty());
        let again = world.clone();
        join(&mut world).unwrap();
        assert_eq!(world, again);

        let mut stray = lab(true);
        stray
            .internet_overlays
            .insert("no-such-site".into(), SiteOverlay::default());
        assert!(join(&mut stray).is_err());
    }

    #[test]
    fn merge_adds_keys_and_restates_values_and_lists() {
        let mut base = serde_json::json!({"a": {"x": 1, "list": [1, 3]}, "s": "old"});
        merge(
            &mut base,
            &serde_json::json!({"a": {"y": 2, "list": [1, 2, 3]}, "s": "new"}),
        );
        assert_eq!(
            base,
            serde_json::json!({"a": {"x": 1, "y": 2, "list": [1, 2, 3]}, "s": "new"})
        );
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
