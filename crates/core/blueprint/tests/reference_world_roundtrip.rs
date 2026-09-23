//! The checked-in world files have to come back out of the parser as the exact
//! bytes that went in. `worlds/company-2026/world.json` is 3.6 MB of generated
//! JSON whose hash the determinism corpus pins, so a writer that differs from
//! JavaScript's `JSON.stringify` by one space would rewrite the whole file.
use cw_blueprint::json::{self, Format, Step};
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

fn repo_path(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .join(relative)
}

/// Report the first differing byte with its surroundings; a bare `assert_eq!`
/// on two megabytes of JSON says nothing at all.
fn assert_same_bytes(expected: &str, actual: &str, what: &str) {
    if expected == actual {
        return;
    }
    let (left, right) = (expected.as_bytes(), actual.as_bytes());
    let at = left
        .iter()
        .zip(right)
        .position(|(a, b)| a != b)
        .unwrap_or(left.len().min(right.len()));
    let from = at.saturating_sub(60);
    let window = |bytes: &[u8]| {
        String::from_utf8_lossy(&bytes[from.min(bytes.len())..(at + 60).min(bytes.len())])
            .into_owned()
    };
    panic!(
        "{what} differs at byte {at} ({} bytes on disk, {} written)\n\
         on disk: {:?}\n\
         written: {:?}",
        left.len(),
        right.len(),
        window(left),
        window(right),
    );
}

#[test]
fn the_reference_world_round_trips_byte_for_byte() {
    round_trips_with_its_sites("worlds/company-2026");
}

#[test]
fn the_internet_round_trips_byte_for_byte() {
    round_trips_with_its_sites("worlds/internet");
}

fn round_trips_with_its_sites(root: &str) {
    let path = repo_path(&format!("{root}/world.json"));
    let text = fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let world = json::parse(&text).unwrap_or_else(|e| panic!("{}: {e}", path.display()));

    // Every service that came from its own `services/<id>/service.json` is written on one line.
    let services_dir = repo_path(&format!("{root}/services"));
    let sites: BTreeSet<String> = fs::read_dir(&services_dir)
        .unwrap_or_else(|e| panic!("{}: {e}", services_dir.display()))
        .map(|entry| entry.expect("a directory entry").path())
        .filter(|dir| dir.join("service.json").is_file())
        .filter_map(|dir| dir.file_name()?.to_str().map(str::to_string))
        .collect();
    assert!(
        !sites.is_empty(),
        "no services found in {}",
        services_dir.display()
    );

    let services = world
        .get("services")
        .and_then(|node| node.as_list())
        .expect("the world has a list of services");
    let from_a_site_file = |path: &[Step<'_>]| match path {
        [Step::Key("services"), Step::Index(index)] => services
            .get(*index)
            .and_then(|service| service.get("id"))
            .and_then(|id| id.as_str())
            .is_some_and(|id| sites.contains(id)),
        [Step::Key("internet_overlays"), Step::Key(_)] => true,
        _ => false,
    };

    let written = json::write(&world, Format::PrettyExcept(&from_a_site_file)) + "\n";
    assert_same_bytes(&text, &written, "world.json");
}

#[test]
fn the_example_world_round_trips_byte_for_byte() {
    let path = repo_path("worlds/agent-desktop/world.json");
    let text = fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let world = json::parse(&text).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let written = json::write(&world, Format::Pretty) + "\n";
    assert_same_bytes(&text, &written, "agent-desktop.json");
}
