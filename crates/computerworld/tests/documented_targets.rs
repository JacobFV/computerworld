//! The published list of shell targets has to be the real one. A doc that drifts from
//! the router is worse than no doc, so this reads the document and checks it.
use computerworld::{reference_world, World};
use cw_protocol::{ActionEnvelope, EnvironmentConfig};
use serde_json::json;
use std::collections::BTreeSet;

const DOC: &str = include_str!("../../../docs/action-families.md");

/// Every `shell:*` target the document names, with its placeholders resolved to a
/// concrete example so it can actually be dispatched.
fn documented() -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for line in DOC.lines() {
        for token in line.split('`') {
            if !token.starts_with("shell:") {
                continue;
            }
            for part in token.split("\\|") {
                let part = part.trim();
                if part.is_empty() {
                    continue;
                }
                let target = if part.starts_with("shell:") {
                    part.to_owned()
                } else {
                    // `shell:power:lock` / `:off` continuation forms.
                    continue;
                };
                out.insert(target);
            }
        }
    }
    out
}

#[test]
fn every_documented_shell_target_is_one_the_router_recognises() {
    let mut world = World::new(reference_world(), 3).unwrap();
    let session = world
        .environment(EnvironmentConfig::desktop("alice", "alice-mac"))
        .unwrap();
    let mut unknown = Vec::new();
    // A target nobody wrote a handler for, dispatched first so the detector below is
    // known to fire. Without it an assertion over an always-empty list proves nothing.
    let canary = dispatch(&mut world, &session, "shell:no-such-control-exists");
    assert!(
        canary.as_deref().is_some_and(never_routed),
        "the unknown-target detector caught nothing for a made-up target: {canary:?}"
    );
    for target in documented() {
        // Placeholders stand for a shape, not a literal; substitute a real one.
        let concrete = target
            .replace("<kind>", "files")
            .replace("<name>", "quick")
            .replace("<switch>", "wifi")
            .replace("<level>", "brightness")
            .replace("<pct>", "50")
            .replace("<char>", "a")
            .replace("<key>", "Enter")
            .replace("<i>", "0")
            .replace("<n>", "0")
            .replace("<path>", "/");
        // `shell:*` is how the prose writes "the shell namespace", not a control.
        if concrete.contains('<') || concrete.ends_with('*') {
            continue;
        }
        // A control may legitimately refuse in this state — no page to bookmark, no
        // window to close. What it must not do is be unrecognised.
        if let Some(refusal) = dispatch(&mut world, &session, &concrete) {
            if never_routed(&refusal) {
                unknown.push(format!("{concrete} -> {refusal}"));
            }
        }
    }
    // Targets that are shape-only or need state this fixture lacks are expected to
    // refuse; what this catches is a documented target no handler has ever heard of.
    assert!(
        unknown.is_empty(),
        "documented targets the router does not recognise:\n{unknown:#?}"
    );
}

/// Press one shell target and hand back the refusal it gave, if it refused at all.
fn dispatch(world: &mut World, session: &str, target: &str) -> Option<String> {
    let result = world
        .step(
            session,
            vec![ActionEnvelope::new(
                "application.v1",
                "shell",
                "alice-mac",
                json!({ "target": target }),
            )],
        )
        .unwrap();
    let error = result.outcomes[0].error.as_ref()?;
    Some(format!(
        "{}/{}: {}",
        error.code,
        error.reason.as_deref().unwrap_or("-"),
        error.message
    ))
}

/// The refusal the router gives when no handler claims a target at all, as opposed to
/// a handler that claimed it and said "not in this state".
fn never_routed(refusal: &str) -> bool {
    refusal.contains(cw_protocol::reason::UNKNOWN_SHELL_TARGET)
}

#[test]
fn the_document_names_every_target_family_the_router_handles() {
    // The reverse direction: a target the router grew but nobody wrote down.
    let source = include_str!("../../environment/src/desktop_extensions.rs");
    let shell = include_str!("../../environment/src/lib.rs");
    let mut missing = Vec::new();
    let mut found = Vec::new();
    for text in [source, shell] {
        for piece in text.split("strip_prefix(\"shell:").skip(1) {
            let Some(name) = piece.split('"').next() else {
                continue;
            };
            let family = format!("shell:{name}");
            if family.len() <= 7 {
                continue;
            }
            found.push(family.clone());
            if !DOC.contains(&family) {
                missing.push(family);
            }
        }
    }
    // A test that examines nothing passes for the wrong reason.
    found.sort();
    found.dedup();
    assert!(
        found.len() >= 10,
        "only found {} target families to check; the extraction has broken: {found:#?}",
        found.len()
    );
    missing.sort();
    missing.dedup();
    assert!(
        missing.is_empty(),
        "handled but undocumented target families:\n{missing:#?}"
    );
}
