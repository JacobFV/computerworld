//! The address bar is an omnibox, end to end through the agent API: what is typed
//! into the browser chrome and what `browser.v1 navigate` is given go through the
//! same resolution, so `github.com` is a site, `deterministic simulation` is a
//! search, and a name the world's DNS does not know becomes a search rather than an
//! error page.
use computerworld::{reference_world, World};
use cw_protocol::{ActionEnvelope, EnvironmentConfig};
use serde_json::{json, Value};

const MACHINE: &str = "alice-mac";

fn world() -> (World, String) {
    let mut definition = reference_world();
    definition.metadata["desktop_themes"] = json!({ MACHINE: "virtual-macos-golden-gate" });
    // A single-label intranet name, the way a company's search domain gives one: it is
    // the case the omnibox can only settle by asking the world's DNS.
    definition.network.dns.push(cw_protocol::DnsRecord {
        name: "intranet".into(),
        address: "intranet.internal".into(),
        ttl_us: 60_000_000,
        resolver: Some("app-server".into()),
    });
    let mut world = World::new(definition, 42).unwrap();
    let mut config = EnvironmentConfig::desktop("alice", MACHINE);
    config.observations = vec!["semantic.v1".into(), "browser.v1".into()];
    let session = world.environment(config).unwrap();
    (world, session)
}
fn act(world: &mut World, session: &str, family: &str, op: &str, payload: Value) {
    let result = world
        .step(
            session,
            vec![ActionEnvelope::new(family, op, MACHINE, payload)],
        )
        .unwrap();
    assert!(result.outcomes[0].success, "{op}: {:?}", result.outcomes[0]);
}
fn click(world: &mut World, session: &str, target: &str) {
    let scene = world.scene(session, 960, 640).unwrap();
    let node = scene
        .nodes
        .iter()
        .find(|node| node.interaction.as_deref() == Some(target))
        .unwrap_or_else(|| panic!("missing interaction {target}"));
    let bounds = node.transform.bounds(node.bounds);
    let x = bounds.x + (bounds.width / 2) as i32;
    let y = bounds.y + (bounds.height / 2) as i32;
    act(
        world,
        session,
        "pointer.v1",
        "click",
        json!({"x":x,"y":y,"width":960,"height":640}),
    );
}
/// Click the address bar, type `text`, press Enter, and report where the tab landed.
fn type_into_address_bar(world: &mut World, session: &str, text: &str) -> (String, String) {
    click(world, session, "window:0:content:shell:address");
    act(
        world,
        session,
        "keyboard.v1",
        "type",
        json!({ "text": text }),
    );
    act(world, session, "keyboard.v1", "key", json!({"key":"Enter"}));
    landed(world, session)
}
fn landed(world: &World, session: &str) -> (String, String) {
    let observation = world.observe(session).unwrap();
    let browser = &observation.channels["browser.v1"][MACHINE];
    let title = observation.channels["semantic.v1"][MACHINE]["title"]
        .as_str()
        .unwrap_or_default()
        .to_owned();
    (
        browser["url"].as_str().unwrap_or_default().to_owned(),
        title,
    )
}

#[test]
fn the_address_bar_takes_a_host_a_path_and_a_query() {
    let (mut world, session) = world();
    click(&mut world, &session, "shell:launch:browser");

    // A bare host: no scheme typed, https assumed, and the chrome shows the real URL.
    let (url, title) = type_into_address_bar(&mut world, &session, "github.com");
    assert_eq!(url, "https://github.com/");
    assert!(title.contains("GitHub"), "{title}");

    // A host with a path lands on the repository itself.
    let (url, title) = type_into_address_bar(&mut world, &session, "github.com/northstar/atlas");
    assert_eq!(url, "https://github.com/northstar/atlas");
    assert!(
        title.starts_with("northstar/atlas: Deterministic simulation runtime"),
        "{title}"
    );

    // Two words are not a host: they are a search on the default engine.
    let (url, title) = type_into_address_bar(&mut world, &session, "deterministic simulation");
    assert_eq!(url, "https://google.com/search?q=deterministic+simulation");
    assert_eq!(title, "deterministic simulation - Google");

    // A name nobody registered is a search too, not "this site can't be reached".
    let (url, title) = type_into_address_bar(&mut world, &session, "atlas launch checklist.txt");
    assert_eq!(
        url,
        "https://google.com/search?q=atlas+launch+checklist.txt"
    );
    assert!(title.ends_with(" - Google"), "{title}");

    // A leading `?` searches for something that would otherwise have been visited.
    let (url, _) = type_into_address_bar(&mut world, &session, "?github.com");
    assert_eq!(url, "https://google.com/search?q=github.com");
}

#[test]
fn the_agent_facing_navigate_action_resolves_the_same_way() {
    let (mut world, session) = world();
    for (typed, landed_at) in [
        ("github.com", "https://github.com/"),
        (
            "github.com/northstar/atlas",
            "https://github.com/northstar/atlas",
        ),
        // An explicit scheme is honoured exactly as given.
        ("http://intranet.internal/", "http://intranet.internal/"),
        // A single label the world resolves...
        ("intranet", "https://intranet/"),
        // ...and one it does not.
        ("unregistered", "https://google.com/search?q=unregistered"),
        (
            "deterministic simulation",
            "https://google.com/search?q=deterministic+simulation",
        ),
        (
            "nowhere.invalid",
            "https://google.com/search?q=nowhere.invalid",
        ),
    ] {
        act(
            &mut world,
            &session,
            "browser.v1",
            "navigate",
            json!({ "url": typed }),
        );
        assert_eq!(landed(&world, &session).0, landed_at, "typed {typed}");
    }
}

#[test]
fn a_url_the_browser_will_not_serve_is_still_an_error() {
    let (mut world, session) = world();
    let result = world
        .step(
            &session,
            vec![ActionEnvelope::new(
                "browser.v1",
                "navigate",
                MACHINE,
                json!({"url":"file:///etc/passwd"}),
            )],
        )
        .unwrap();
    assert!(!result.outcomes[0].success, "{:?}", result.outcomes[0]);
}

#[test]
fn a_world_can_name_its_own_search_engine() {
    let mut definition = reference_world();
    definition.metadata["search_engine"] = json!("https://duckduckgo.com/search?q=%s");
    let mut world = World::new(definition, 42).unwrap();
    let mut config = EnvironmentConfig::desktop("alice", MACHINE);
    config.observations = vec!["semantic.v1".into(), "browser.v1".into()];
    let session = world.environment(config).unwrap();
    act(
        &mut world,
        &session,
        "browser.v1",
        "navigate",
        json!({"url":"deterministic simulation"}),
    );
    let (url, title) = landed(&world, &session);
    assert_eq!(
        url,
        "https://duckduckgo.com/search?q=deterministic+simulation"
    );
    assert_eq!(title, "deterministic simulation - DuckDuckGo");
}
