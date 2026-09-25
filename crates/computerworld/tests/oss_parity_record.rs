//! Records what the world's own services answer to the API requests the oss-web
//! framework-parity fixtures make (crates/web/engine/tests/framework-parity/
//! `<name>.api.json`), so Chromium's dump (`scripts/web-parity/dump.mjs` on the
//! fixture's `<name>.site.json`) and the engine's run (`tests/framework_parity.rs`)
//! both see exactly the bytes the app's backend returns in `worlds/oss-web`.
//!
//! Each recording lists `{method, url, body?}`; this fills in each one's `response`
//! (status, the headers a page depends on, and the body as text) from a fresh world,
//! in the listed order, through `http.v1` as the page's own requests reach the
//! service. When a dump reports requests missing from a recording, add them to its
//! list and run:
//!
//!     cargo test -p computerworld --features oss-web --test oss_parity_record -- --ignored
#![cfg(feature = "oss-web")]

use computerworld::World;
use cw_protocol::{ActionEnvelope, EnvironmentConfig, WorldDefinition};
use serde_json::{json, Value};

const MACHINE: &str = "workstation";

fn fixtures_dir() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../web/engine/tests/framework-parity")
}

fn world() -> (World, String) {
    let text = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../worlds/oss-web/world.json"),
    )
    .unwrap();
    let mut world = World::new(WorldDefinition::from_json(&text).unwrap(), 42).unwrap();
    let session = world
        .environment(EnvironmentConfig {
            actor: "ada".into(),
            machines: vec![MACHINE.into()],
            actions: vec!["http.v1".into()],
            observations: vec![],
            action_budget: 1 << 20,
        })
        .unwrap();
    (world, session)
}

/// The headers a page's fetch can observe and CORS needs.
const KEPT: &[&str] = &[
    "content-type",
    "access-control-allow-origin",
    "access-control-allow-credentials",
    "access-control-allow-headers",
    "access-control-allow-methods",
    "access-control-expose-headers",
    "x-total-count",
];

#[test]
#[ignore]
fn record() {
    let mut names: Vec<_> = std::fs::read_dir(fixtures_dir())
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.ends_with(".api.json"))
        .collect();
    names.sort();
    for name in names {
        let path = fixtures_dir().join(&name);
        let mut rec: Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        let (mut world, session) = world();
        for r in rec["requests"].as_array_mut().unwrap() {
            let mut req = json!({
                "method": r["method"],
                "url": r["url"],
                "headers": {"accept": "application/json, text/plain, */*", "origin": "http://example.test"},
                "body": [],
            });
            if let Some(b) = r["body"].as_str() {
                req["headers"]["content-type"] = json!("application/json");
                req["body"] = json!(b.as_bytes());
            }
            let out = world
                .step(
                    &session,
                    vec![ActionEnvelope::new("http.v1", "request", MACHINE, req)],
                )
                .unwrap();
            assert!(
                out.outcomes[0].success,
                "{name}: {r}: {:?}",
                out.outcomes[0]
            );
            let v = &out.outcomes[0].value;
            let bytes: Vec<u8> = serde_json::from_value(v["body"].clone()).unwrap();
            let mut headers = serde_json::Map::new();
            for (k, h) in v["headers"].as_object().into_iter().flatten() {
                if KEPT.contains(&k.to_ascii_lowercase().as_str()) {
                    headers.insert(k.to_ascii_lowercase(), h.clone());
                }
            }
            // The page's origin, whichever site the fixture opens.
            headers.insert("access-control-allow-origin".into(), json!("*"));
            // Text as text; anything else (an avatar image) as its bytes.
            r["response"] = match String::from_utf8(bytes) {
                Ok(text) => json!({"status": v["status"], "headers": headers, "text": text}),
                Err(e) => {
                    json!({"status": v["status"], "headers": headers, "bytes": e.into_bytes()})
                }
            };
        }
        std::fs::write(&path, serde_json::to_string_pretty(&rec).unwrap() + "\n").unwrap();
        eprintln!(
            "{name}: {} responses recorded",
            rec["requests"].as_array().unwrap().len()
        );
    }
}
