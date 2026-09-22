//! The worked example in the documentation has to keep working.
mod host;
use host::Host;
use std::collections::BTreeMap;

fn built(supplied: &BTreeMap<String, String>) -> cw_blueprint::Resolved {
    let files = Host::at("examples/worlds/agent-desktop");
    cw_blueprint::resolve("world.yml", &files, supplied)
        .unwrap_or_else(|e| panic!("resolving the example blueprint: {e}"))
}

#[test]
fn the_example_blueprint_rebuilds_the_example_world_byte_for_byte() {
    let built = built(&BTreeMap::new()).to_world_json();
    let checked_in = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../examples/worlds/agent-desktop.json"),
    )
    .unwrap();
    assert_eq!(built, checked_in, "run: scripts/build-content.sh");
}

#[test]
fn an_input_changes_the_world_it_builds() {
    let supplied = [("AGENT_USER".to_string(), "noor".to_string())]
        .into_iter()
        .collect();
    let definition = built(&supplied).definition().unwrap();
    assert_eq!(definition.computers[0].user, "noor");
    // The default is what the checked-in world was built with.
    assert_eq!(
        built(&BTreeMap::new()).definition().unwrap().computers[0].user,
        "ada"
    );
}

#[test]
fn the_copied_directory_is_what_the_machine_starts_with() {
    let definition = built(&BTreeMap::new()).definition().unwrap();
    let files = &definition.computers[0].initial_files;
    assert_eq!(
        files.keys().map(String::as_str).collect::<Vec<_>>(),
        [
            "Documents/expenses.csv",
            "Documents/handbook.txt",
            "Documents/report.md",
            "notes.txt"
        ]
    );
    assert!(files["Documents/handbook.txt"].contains("wiki.internal"));
}

#[test]
fn placing_the_service_derived_its_node_link_and_record() {
    let definition = built(&BTreeMap::new()).definition().unwrap();
    let node = definition
        .network
        .nodes
        .iter()
        .find(|n| n.id == "intranet-server")
        .unwrap();
    assert_eq!(node.address, "10.0.1.10");
    assert_eq!(definition.network.links.len(), 1);
    assert_eq!(definition.network.links[0].latency_us, 10);
    let record = &definition.network.dns[0];
    assert_eq!(record.name, "wiki.internal");
    assert_eq!(record.address, "10.0.1.10");
    assert_eq!(record.resolver.as_deref(), Some("intranet-server"));
}
