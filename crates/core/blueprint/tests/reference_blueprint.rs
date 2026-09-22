//! The reference company's blueprint must rebuild the world file it ships with.
//!
//! `worlds/company-2026/world.json` is embedded in every native and Wasm build and
//! its bytes are pinned by the determinism corpus, so "the blueprint resolves to
//! something equivalent" is not enough — it has to resolve to the same bytes.
mod host;
use host::Host;
use std::collections::BTreeMap;

#[test]
fn the_reference_blueprint_rebuilds_the_world_byte_for_byte() {
    let files = Host::at("worlds/company-2026");
    let resolved = cw_blueprint::resolve("world.yml", &files, &BTreeMap::new())
        .unwrap_or_else(|e| panic!("resolving the reference blueprint: {e}"));
    let built = resolved.to_world_json();
    let checked_in = std::fs::read_to_string(files.root.join("world.json")).unwrap();
    if built != checked_in {
        let at = built
            .bytes()
            .zip(checked_in.bytes())
            .position(|(a, b)| a != b)
            .unwrap_or(built.len().min(checked_in.len()));
        let window = |text: &str| {
            let start = at.saturating_sub(80);
            text[start..text.len().min(at + 80)]
                .escape_debug()
                .to_string()
        };
        panic!(
            "the blueprint no longer rebuilds world.json (first difference at byte {at})\n\
             built:      {}\nchecked in: {}\n\nrun: scripts/build-content.sh",
            window(&built),
            window(&checked_in),
        );
    }
}

#[test]
fn the_resolved_world_is_the_one_the_engine_validates() {
    let files = Host::at("worlds/company-2026");
    let resolved = cw_blueprint::resolve("world.yml", &files, &BTreeMap::new()).unwrap();
    let definition = resolved.definition().unwrap();
    assert_eq!(definition.id, "northstar-company-2026");
    assert_eq!(definition.computers.len(), 5);
    assert_eq!(definition.services.len(), 93);
    // The seeded home folders are the point of `copy:`; a desktop that came back
    // with two files would mean the directories had quietly stopped being read.
    let desktop = definition
        .computers
        .iter()
        .find(|c| c.id == "carol-ubuntu")
        .unwrap();
    assert_eq!(desktop.initial_files.len(), 37);
    assert!(desktop.initial_files.contains_key("notes.txt"));
    assert!(desktop
        .initial_files
        .keys()
        .any(|p| p.starts_with("Documents/")));
}

#[test]
fn a_blueprint_says_which_files_it_was_built_from() {
    let files = Host::at("worlds/company-2026");
    let resolved = cw_blueprint::resolve("world.yml", &files, &BTreeMap::new()).unwrap();
    assert!(resolved.read.contains("world.yml"));
    assert!(resolved.read.contains("sites/airbnb.json"));
    assert!(resolved.read.contains("index/google-search.json"));
    assert!(resolved.read.contains("home/ubuntu/notes.txt"));
    assert!(resolved
        .read
        .iter()
        .any(|p| p.starts_with("home/all/Documents/")));
    assert!(
        resolved.inputs.is_empty(),
        "the reference world declares no inputs"
    );
}
