//! Refusing a key nobody will read.
//!
//! `WorldDefinition` deserialises with serde's defaults, which ignore what they
//! do not recognise. A misspelled `intial_files` therefore survives into the
//! world file, is dropped on the way into the engine, and the machine comes up
//! empty with nothing anywhere saying why. The schema is small enough to state,
//! so it is stated, and a blueprint that misspells a key does not build.
//!
//! The lists mirror [`cw_protocol`]. A field added there and not here refuses a
//! blueprint that uses it, which is a loud failure rather than a quiet one.
use crate::node::Node;
use crate::{known_keys, Error};

const PROFILE: &[&str] = &["id", "name", "family", "home", "case_sensitive", "shell"];
const COMPUTER: &[&str] = &[
    "id",
    "profile",
    "address",
    "user",
    "node",
    "initial_files",
    "initial_binary_files",
    "installed_apps",
    "packages",
    "presentation",
];
const SERVICE: &[&str] = &[
    "id",
    "kind",
    "node",
    "domains",
    "port",
    "tls",
    "initial_state",
];
const NETWORK: &[&str] = &["implicit_lan", "nodes", "links", "dns", "routes", "gateway"];
const NODE: &[&str] = &["id", "address", "zone"];
const LINK: &[&str] = &[
    "from",
    "to",
    "bidirectional",
    "latency_us",
    "loss_per_million",
];
const DNS: &[&str] = &["name", "address", "ttl_us", "resolver"];
const ROUTE: &[&str] = &["from", "to", "via"];
const GATEWAY: &[&str] = &[
    "sources",
    "ports",
    "schemes",
    "allowed_cidrs",
    "denied_cidrs",
    "max_response_bytes",
    "timeout_us",
    "allow_local",
    "allow_internet",
    "allow_host",
    "host_allowlist",
    "denied_pairs",
];

/// Check every key of a resolved world, source-only directives already gone.
pub fn check(world: &Node, source: &str) -> Result<(), Error> {
    each(world, "profiles", PROFILE, "a profile", source)?;
    each(world, "computers", COMPUTER, "a computer", source)?;
    each(world, "services", SERVICE, "a service", source)?;
    let Some(network) = world.get("network") else {
        return Ok(());
    };
    let Some(map) = network.as_map() else {
        return Err(Error::at(
            source,
            format!("network is a mapping, not {}", network.kind()),
        ));
    };
    known_keys(map, NETWORK, "the network", source)?;
    each(network, "nodes", NODE, "a network node", source)?;
    each(network, "links", LINK, "a network link", source)?;
    each(network, "dns", DNS, "a DNS record", source)?;
    each(network, "routes", ROUTE, "a route", source)?;
    if let Some(gateway) = network.get("gateway") {
        let Some(map) = gateway.as_map() else {
            return Err(Error::at(source, "the gateway policy is a mapping"));
        };
        known_keys(map, GATEWAY, "the gateway policy", source)?;
    }
    Ok(())
}

fn each(parent: &Node, key: &str, allowed: &[&str], what: &str, source: &str) -> Result<(), Error> {
    let Some(value) = parent.get(key) else {
        return Ok(());
    };
    let Some(items) = value.as_list() else {
        return Err(Error::at(
            source,
            format!("{key} is a list, not {}", value.kind()),
        ));
    };
    for item in items {
        let Some(map) = item.as_map() else {
            return Err(Error::at(
                source,
                format!("{what} is a mapping, not {}", item.kind()),
            ));
        };
        // Name the entry, so a message about eighty-seven services says which.
        let named = map
            .get("id")
            .or_else(|| map.get("name"))
            .and_then(Node::as_str)
            .map(|id| format!("{what} ({id})"))
            .unwrap_or_else(|| what.to_string());
        known_keys(map, allowed, &named, source)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::yaml;

    fn checked(text: &str) -> Result<(), Error> {
        check(&yaml::parse(text).unwrap(), "w.yml")
    }

    #[test]
    fn a_world_of_known_keys_passes() {
        assert!(checked(
            "computers:\n  - {id: a, profile: p, address: \"10.0.0.1\", user: ada}\nnetwork:\n  \
             nodes:\n    - {id: a, address: \"10.0.0.1\", zone: local}\n  gateway: {allow_host: false}\n"
        )
        .is_ok());
    }

    #[test]
    fn a_misspelled_computer_key_is_refused_and_names_the_computer() {
        let error = checked("computers:\n  - {id: alice-mac, intial_files: {}}\n")
            .unwrap_err()
            .to_string();
        assert!(error.contains("a computer (alice-mac)"), "{error}");
        assert!(error.contains("did you mean initial_files?"), "{error}");
    }

    #[test]
    fn a_misspelled_service_or_network_key_is_refused() {
        assert!(checked("services:\n  - {id: wiki, kind: wiki, domians: []}\n").is_err());
        assert!(checked("network:\n  nodes:\n    - {id: a, adress: x}\n").is_err());
        assert!(checked("network:\n  dns:\n    - {name: a, ttl: 1}\n").is_err());
        assert!(checked("network:\n  gateway: {allow_internets: true}\n").is_err());
        assert!(checked("network:\n  implicit_lans: true\n").is_err());
    }

    #[test]
    fn a_source_only_key_that_survived_is_refused() {
        // `copy:` and `place:` are resolved away; one reaching here means the
        // resolver skipped it, and shipping it would be worse than failing.
        assert!(checked("computers:\n  - {id: a, copy: []}\n").is_err());
        assert!(checked("services:\n  - {id: a, kind: k, place: {}}\n").is_err());
    }

    #[test]
    fn the_wrong_shape_is_refused_rather_than_ignored() {
        assert!(checked("computers: {id: a}\n").is_err());
        assert!(checked("services:\n  - just-a-string\n").is_err());
        assert!(checked("network: []\n").is_err());
    }
}
