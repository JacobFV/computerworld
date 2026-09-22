//! An HTML site served by the static-site service, driven end to end through the
//! agent API: the browser renders it with the web engine, the semantic observation
//! lists its headings, text, links and controls, and links, forms and typing work.
use computerworld::{reference_world, World};
use cw_protocol::{ActionEnvelope, EnvironmentConfig, ServiceDefinition};
use serde_json::{json, Value};

const MACHINE: &str = "alice-mac";
const ACTOR: &str = "alice";

fn world() -> (World, String) {
    let mut definition = reference_world();
    definition.services.push(ServiceDefinition {
        id: "html-site".into(),
        kind: "static-site".into(),
        node: "app-server".into(),
        domains: vec!["html.example".into()],
        port: 8090,
        tls: false,
        initial_state: json!({"files":{
            "/index.html": "<!DOCTYPE html><html><head><title>Atlas HTML</title><link rel=stylesheet href=/site.css></head><body><h1>Atlas in HTML</h1><p>Served as files.</p><a id=docs href=/docs/>Read the docs</a><form id=search action=/docs/><label for=q>Search</label><input id=q name=q><button id=go>Go</button></form></body></html>",
            "/site.css": "h1 { color: #112233 }",
            "/docs/index.html": "<title>Docs</title><h2>Documentation</h2><p>You found the docs.</p><a href=/>Home</a>"
        }}),
    });
    let mut world = World::new(definition, 42).unwrap();
    let session = world
        .environment(EnvironmentConfig {
            actor: ACTOR.into(),
            machines: vec![MACHINE.into()],
            actions: vec!["browser.v1".into(), "keyboard.v1".into()],
            observations: vec!["semantic.v1".into(), "browser.v1".into()],
            action_budget: 1 << 20,
        })
        .unwrap();
    (world, session)
}
fn act(world: &mut World, session: &str, op: &str, payload: Value) -> Value {
    let result = world
        .step(
            session,
            vec![ActionEnvelope::new("browser.v1", op, MACHINE, payload)],
        )
        .unwrap();
    assert!(result.outcomes[0].success, "{op}: {:?}", result.outcomes[0]);
    result.outcomes[0].value.clone()
}
fn page(world: &World, session: &str) -> Value {
    world.observe(session).unwrap().channels["semantic.v1"][MACHINE].clone()
}
fn texts(page: &Value) -> Vec<String> {
    fn walk(elements: &[Value], out: &mut Vec<String>) {
        for e in elements {
            for key in ["text", "label"] {
                if let Some(s) = e[key].as_str() {
                    out.push(s.to_owned());
                }
            }
            if let Some(children) = e["children"].as_array() {
                walk(children, out);
            }
        }
    }
    let mut out = Vec::new();
    walk(page["elements"].as_array().unwrap(), &mut out);
    out
}

#[test]
fn an_html_site_is_rendered_read_and_driven_through_the_agent_api() {
    let (mut world, session) = world();
    act(
        &mut world,
        &session,
        "navigate",
        json!({"url":"http://html.example:8090/"}),
    );
    let home = page(&world, &session);
    assert_eq!(home["title"], "Atlas HTML");
    let words = texts(&home);
    assert!(words.iter().any(|w| w == "Atlas in HTML"), "{words:?}");
    assert!(words.iter().any(|w| w == "Served as files."), "{words:?}");
    let link = home["elements"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["kind"] == "link")
        .expect("a link");
    assert_eq!(link["id"], "docs");
    assert_eq!(link["url"], "http://html.example:8090/docs/");
    let browser = world.observe(&session).unwrap().channels["browser.v1"][MACHINE].clone();
    assert_eq!(browser["url"], "http://html.example:8090/");
    assert_eq!(browser["fields"]["q"], "");
    assert_eq!(browser["page"]["title"], "Atlas HTML");

    act(&mut world, &session, "click", json!({"id":"docs"}));
    let docs = page(&world, &session);
    assert_eq!(docs["title"], "Docs");
    assert!(texts(&docs).iter().any(|w| w == "You found the docs."));
    act(&mut world, &session, "back", json!({}));
    assert_eq!(page(&world, &session)["title"], "Atlas HTML");

    act(
        &mut world,
        &session,
        "fill",
        json!({"id":"q","value":"maps"}),
    );
    act(&mut world, &session, "key", json!({"key":"Enter"}));
    let browser = world.observe(&session).unwrap().channels["browser.v1"][MACHINE].clone();
    assert_eq!(browser["url"], "http://html.example:8090/docs/?q=maps");
    assert_eq!(page(&world, &session)["title"], "Docs");

    // Typing goes into the focused control and shows in the observation.
    act(&mut world, &session, "back", json!({}));
    act(&mut world, &session, "click", json!({"id":"q"}));
    let result = world
        .step(
            &session,
            vec![ActionEnvelope::new(
                "keyboard.v1",
                "type",
                MACHINE,
                json!({"text":"hello"}),
            )],
        )
        .unwrap();
    assert!(result.outcomes[0].success, "{:?}", result.outcomes[0]);
    let home = page(&world, &session);
    let input = home["elements"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|e| e["children"].as_array().cloned().unwrap_or_default())
        .find(|e| e["id"] == "q")
        .expect("the input");
    assert_eq!(
        (input["label"].as_str(), input["value"].as_str()),
        (Some("Search"), Some("hello"))
    );
}
