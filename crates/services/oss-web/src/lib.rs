//! Real open-source web apps, packaged for the `node-app` service kind
//! (`cw-service-node-app`) and registered by [`register`]. Each package under
//! `packages/<id>/` is an app's own build output, produced from its upstream at a
//! pinned commit by `scripts/oss-web/build.sh`, and a checked-in `manifest.json`;
//! docs/oss-webapps.md says which apps, from where, under which licence, and what
//! (little) was changed.
use std::collections::BTreeMap;

pub use cw_service_node_app::{NodeApp, Package};

mod embedded {
    include!(concat!(env!("OUT_DIR"), "/packages.rs"));
}

/// The ids of the packages this build carries, sorted.
pub fn ids() -> Vec<&'static str> {
    let mut ids: Vec<&str> = embedded::FILES.iter().map(|(p, _, _)| *p).collect();
    ids.dedup();
    ids
}

/// Every package, parsed.
pub fn packages() -> cw_protocol::Result<Vec<Package>> {
    let mut by_id: BTreeMap<&str, BTreeMap<String, &'static [u8]>> = BTreeMap::new();
    for (package, path, bytes) in embedded::FILES {
        by_id
            .entry(package)
            .or_default()
            .insert((*path).to_owned(), *bytes);
    }
    by_id.into_values().map(Package::new).collect()
}

/// Registers the `node-app` kind with every package.
pub fn register(registry: &mut cw_sdk::Registry) -> cw_protocol::Result<()> {
    cw_service_node_app::register(registry, packages()?)
}

#[cfg(test)]
mod tests {
    #[test]
    fn every_package_parses_and_names_itself() {
        let packages = super::packages().unwrap();
        let ids: Vec<&str> = packages.iter().map(|p| p.manifest.id.as_str()).collect();
        assert_eq!(ids, super::ids());
        assert_eq!(
            ids,
            [
                "conduit-react",
                "conduit-vue",
                "json-server",
                "react-admin",
                "realworld-api",
                "todomvc"
            ]
        );
    }
}
