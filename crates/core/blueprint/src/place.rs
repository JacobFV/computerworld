//! Placing a service: its node, its link and its DNS records from one block.
//!
//! Stating those separately is what makes a large world unwritable by hand — the
//! reference company has ninety-three services and pays for them with a hundred
//! and two nodes, a hundred and two links and two hundred and thirty records,
//! none of which say anything the service did not already imply. A `place:` block
//! says where a service sits; the rest is derived.
//!
//! Nothing here is specific to a company: the addresses, the zone, the client a
//! service is reachable from and the TTLs all come from the blueprint, either on
//! the service or under `defaults.place`.
use crate::node::{Map, Node};
use crate::{known_keys, Error};

const PLACE_KEYS: &[&str] = &["address", "zone", "link", "dns"];
const LINK_KEYS: &[&str] = &["from", "latency_us", "loss_per_million", "bidirectional"];

/// Read `key` from the service's own `place`, else from `defaults.place`.
fn setting<'a>(place: &'a Map, defaults: &'a Node, key: &str) -> Option<&'a Node> {
    place.get(key).or_else(|| defaults.get("place")?.get(key))
}

/// The TTL and resolver a zone's records take.
fn dns_defaults<'a>(place: &'a Map, defaults: &'a Node, zone: &str) -> Option<&'a Node> {
    setting(place, defaults, "dns")?.get(zone)
}

pub fn expand(document: &mut Node, source: &str) -> Result<(), Error> {
    let defaults = document
        .get("defaults")
        .cloned()
        .unwrap_or(Node::Map(Map::new()));
    let defaults = &defaults;
    if let Some(map) = defaults.as_map() {
        known_keys(map, &["place", "build_only"], "defaults", source)?;
        if let Some(place) = defaults.get("place").and_then(Node::as_map) {
            known_keys(place, PLACE_KEYS, "defaults.place", source)?;
        }
    }
    strip_build_only(document, defaults)?;
    let Some(root) = document.as_map_mut() else {
        return Err(Error::at(source, "a blueprint is a mapping"));
    };
    // Collect first: the services are borrowed while placing writes into network.
    let mut placements = Vec::new();
    if let Some(services) = root.get_mut("services").and_then(Node::as_list_mut) {
        for service in services.iter_mut() {
            let Some(map) = service.as_map_mut() else {
                return Err(Error::at(source, "every service is a mapping"));
            };
            let Some(place) = map.remove("place") else {
                continue;
            };
            let id = map
                .get("id")
                .and_then(Node::as_str)
                .unwrap_or("<unnamed>")
                .to_string();
            let Some(node) = map.get("node").and_then(Node::as_str).map(str::to_string) else {
                return Err(Error::at(
                    source,
                    format!("service {id}: place needs a node"),
                ));
            };
            let domains: Vec<String> = map
                .get("domains")
                .and_then(Node::as_list)
                .unwrap_or(&[])
                .iter()
                .filter_map(|d| d.as_str().map(str::to_string))
                .collect();
            let Node::Map(place) = place else {
                return Err(Error::at(
                    source,
                    format!("service {id}: place is a mapping"),
                ));
            };
            known_keys(&place, PLACE_KEYS, &format!("service {id}'s place"), source)?;
            placements.push((id, node, domains, place));
        }
    }
    if placements.is_empty() {
        return Ok(());
    }

    let network = root.get_mut("network");
    let network = match network {
        Some(Node::Map(_)) => root.get_mut("network").unwrap(),
        Some(other) => {
            return Err(Error::at(
                source,
                format!("network is a mapping, not {}", other.kind()),
            ))
        }
        None => {
            root.insert("network", Node::Map(Map::new()));
            root.get_mut("network").unwrap()
        }
    };
    let network = network.as_map_mut().expect("just made a mapping");

    for (id, node_id, domains, place) in placements {
        let Some(address) = setting(&place, defaults, "address").and_then(Node::scalar_text) else {
            return Err(Error::at(
                source,
                format!("service {id}: place needs an address"),
            ));
        };
        let zone = setting(&place, defaults, "zone")
            .and_then(Node::scalar_text)
            .unwrap_or_else(|| "internet".to_string());

        let mut node = Map::new();
        node.insert("id", Node::string(&node_id));
        node.insert("address", Node::string(&address));
        node.insert("zone", Node::string(&zone));
        upsert(network, "nodes", Node::Map(node), |n| {
            n.get("id").and_then(Node::as_str).map(str::to_string)
        });

        // A link setting is looked up on the service's own `link`, then on the
        // defaults' — so a world states its client and its latency once.
        let declared = place.get("link").and_then(Node::as_map);
        let fallback = defaults
            .get("place")
            .and_then(|p| p.get("link"))
            .and_then(Node::as_map);
        if let Some(map) = declared {
            known_keys(
                map,
                LINK_KEYS,
                &format!("service {id}'s place.link"),
                source,
            )?;
        }
        let link_setting = |key: &str| {
            declared
                .and_then(|m| m.get(key))
                .or_else(|| fallback.and_then(|m| m.get(key)))
        };
        if let Some(from) = link_setting("from").and_then(Node::scalar_text) {
            let mut link = Map::new();
            link.insert("from", Node::string(&from));
            link.insert("to", Node::string(&node_id));
            link.insert(
                "bidirectional",
                link_setting("bidirectional")
                    .cloned()
                    .unwrap_or(Node::Bool(true)),
            );
            link.insert(
                "latency_us",
                link_setting("latency_us")
                    .cloned()
                    .unwrap_or(Node::Number("0".into())),
            );
            link.insert(
                "loss_per_million",
                link_setting("loss_per_million")
                    .cloned()
                    .unwrap_or(Node::Number("0".into())),
            );
            upsert(network, "links", Node::Map(link), |l| {
                let from = l.get("from")?.as_str()?;
                let to = l.get("to")?.as_str()?;
                Some(format!("{from}\u{0}{to}"))
            });
        }

        for domain in domains {
            let declared = network
                .get("dns")
                .and_then(Node::as_list)
                .unwrap_or(&[])
                .iter()
                .any(|r| {
                    r.get("name")
                        .and_then(Node::as_str)
                        .is_some_and(|n| n.eq_ignore_ascii_case(&domain))
                });
            // A record someone wrote by hand wins: an alias to another name is
            // exactly what a derived address record would destroy.
            if declared {
                continue;
            }
            let mut record = Map::new();
            record.insert("name", Node::string(&domain));
            record.insert("address", Node::string(&address));
            if let Some(defaults) = dns_defaults(&place, defaults, &zone) {
                for key in ["ttl_us", "resolver"] {
                    if let Some(value) = defaults.get(key) {
                        record.insert(key, value.clone());
                    }
                }
            }
            upsert(network, "dns", Node::Map(record), |r| {
                Some(r.get("name")?.as_str()?.to_ascii_lowercase())
            });
        }
    }

    Ok(())
}

/// Put the DNS records in name order.
///
/// Which order a resolver is given its records in changes nothing it answers, and
/// a sorted list is the difference between adding a service moving one line of the
/// world file and appending to two hundred lines nobody can scan. This runs once,
/// after everything has been merged and placed, so a record added by the last
/// fragment lands in the same place as one added by the first.
pub fn sort_dns(document: &mut Node) {
    let Some(Node::List(records)) = document
        .as_map_mut()
        .and_then(|m| m.get_mut("network"))
        .and_then(|n| match n {
            Node::Map(map) => map.get_mut("dns"),
            _ => None,
        })
    else {
        return;
    };
    fn name(record: &Node) -> &str {
        record.get("name").and_then(Node::as_str).unwrap_or("")
    }
    records.sort_by(|a, b| name(a).cmp(name(b)));
}

/// Drop the keys a blueprint declares as its own working notes.
///
/// A fragment often carries more than the world needs — notes, or keys a generator
/// reads — which belong to the file and not to the service. Naming them once, in
/// `defaults.build_only`, keeps the stripping declarative instead of a list baked
/// into this crate.
fn strip_build_only(document: &mut Node, defaults: &Node) -> Result<(), Error> {
    let keys: Vec<String> = defaults
        .get("build_only")
        .and_then(Node::as_list)
        .unwrap_or(&[])
        .iter()
        .filter_map(|k| k.as_str().map(str::to_string))
        .collect();
    if keys.is_empty() {
        return Ok(());
    }
    let Some(root) = document.as_map_mut() else {
        return Ok(());
    };
    for list in ["services", "computers", "profiles"] {
        let Some(items) = root.get_mut(list).and_then(Node::as_list_mut) else {
            continue;
        };
        for item in items.iter_mut() {
            let Some(map) = item.as_map_mut() else {
                continue;
            };
            for key in &keys {
                map.remove(key);
            }
        }
    }
    Ok(())
}

/// Replace the entry of `network.<key>` with the same identity, or append.
fn upsert(network: &mut Map, key: &str, item: Node, identity: impl Fn(&Node) -> Option<String>) {
    if !network.contains_key(key) {
        network.insert(key, Node::List(Vec::new()));
    }
    let Some(Node::List(items)) = network.get_mut(key) else {
        return;
    };
    let id = identity(&item);
    match items.iter_mut().find(|existing| identity(existing) == id) {
        Some(existing) => *existing = item,
        None => items.push(item),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::yaml;

    fn placed(text: &str) -> Node {
        let mut document = yaml::parse(text).unwrap();
        expand(&mut document, "t.yml").unwrap();
        sort_dns(&mut document);
        document.as_map_mut().map(|m| m.remove("defaults"));
        document
    }

    const WORLD: &str = "
defaults:
  place:
    zone: internet
    link: {from: app-server, latency_us: 1500}
    dns:
      internet: {ttl_us: 300000000, resolver: dns-public}
      local: {ttl_us: 60000000, resolver: app-server}
services:
  - id: shop
    kind: shop
    node: shop
    domains: [shop.example, www.shop.example]
    place:
      address: 203.0.113.44
  - id: wiki
    kind: wiki
    node: wiki
    domains: [wiki.internal]
    place:
      address: 10.0.1.9
      zone: local
";

    #[test]
    fn a_placed_service_gets_its_node_link_and_records() {
        let world = placed(WORLD);
        let network = world.get("network").unwrap();
        let nodes = network.get("nodes").unwrap().as_list().unwrap();
        assert_eq!(nodes.len(), 2);
        assert_eq!(
            nodes[0].get("address").unwrap().as_str(),
            Some("203.0.113.44")
        );
        assert_eq!(nodes[1].get("zone").unwrap().as_str(), Some("local"));
        let links = network.get("links").unwrap().as_list().unwrap();
        assert_eq!(links.len(), 2);
        assert_eq!(links[0].get("from").unwrap().as_str(), Some("app-server"));
        assert_eq!(
            links[0].get("latency_us").unwrap(),
            &Node::Number("1500".into())
        );
        let dns = network.get("dns").unwrap().as_list().unwrap();
        assert_eq!(dns.len(), 3);
    }

    #[test]
    fn records_come_out_in_name_order() {
        let world = placed(WORLD);
        let names: Vec<_> = world
            .get("network")
            .unwrap()
            .get("dns")
            .unwrap()
            .as_list()
            .unwrap()
            .iter()
            .map(|r| r.get("name").unwrap().as_str().unwrap())
            .collect();
        assert_eq!(names, ["shop.example", "wiki.internal", "www.shop.example"]);
    }

    #[test]
    fn a_zone_chooses_its_own_ttl_and_resolver() {
        let world = placed(WORLD);
        let dns = world
            .get("network")
            .unwrap()
            .get("dns")
            .unwrap()
            .as_list()
            .unwrap();
        let wiki = dns
            .iter()
            .find(|r| r.get("name").unwrap().as_str() == Some("wiki.internal"));
        assert_eq!(
            wiki.unwrap().get("resolver").unwrap().as_str(),
            Some("app-server")
        );
        let shop = dns
            .iter()
            .find(|r| r.get("name").unwrap().as_str() == Some("shop.example"));
        assert_eq!(
            shop.unwrap().get("ttl_us").unwrap(),
            &Node::Number("300000000".into())
        );
    }

    #[test]
    fn a_declared_record_is_left_alone() {
        let world = placed(
            "network:\n  dns:\n    - {name: shop.example, address: other.example}\nservices:\n  \
             - id: shop\n    node: shop\n    domains: [shop.example]\n    place: {address: 203.0.113.44}\n",
        );
        let dns = world
            .get("network")
            .unwrap()
            .get("dns")
            .unwrap()
            .as_list()
            .unwrap();
        assert_eq!(dns.len(), 1);
        assert_eq!(
            dns[0].get("address").unwrap().as_str(),
            Some("other.example")
        );
    }

    #[test]
    fn placing_twice_replaces_rather_than_appends() {
        let world = placed(
            "network:\n  nodes:\n    - {id: shop, address: 1.1.1.1, zone: local}\nservices:\n  \
             - id: shop\n    node: shop\n    place: {address: 203.0.113.44}\n",
        );
        let nodes = world
            .get("network")
            .unwrap()
            .get("nodes")
            .unwrap()
            .as_list()
            .unwrap();
        assert_eq!(nodes.len(), 1);
        assert_eq!(
            nodes[0].get("address").unwrap().as_str(),
            Some("203.0.113.44")
        );
    }

    #[test]
    fn the_place_block_does_not_survive_into_the_world() {
        let world = placed(WORLD);
        let shop = &world.get("services").unwrap().as_list().unwrap()[0];
        assert!(shop.get("place").is_none());
    }

    #[test]
    fn a_place_without_an_address_is_refused() {
        let mut document =
            yaml::parse("services:\n  - id: a\n    node: a\n    place: {zone: local}\n").unwrap();
        let error = expand(&mut document, "t.yml").unwrap_err().to_string();
        assert!(error.contains("needs an address"), "{error}");
    }

    #[test]
    fn an_unknown_place_key_is_refused() {
        let mut document =
            yaml::parse("services:\n  - id: a\n    node: a\n    place: {addres: 1.1.1.1}\n")
                .unwrap();
        assert!(expand(&mut document, "t.yml").is_err());
    }
}
