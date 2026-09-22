use computerworld::{reference_world, World};
use cw_protocol::{ActionEnvelope, EnvironmentConfig, HttpRequest};
use serde_json::{json, Value};

fn setup(seed: u64) -> (World, String, String) {
    let mut world = World::new(reference_world(), seed).unwrap();
    let alice = world
        .environment(EnvironmentConfig::desktop("alice", "alice-mac"))
        .unwrap();
    let bob = world
        .environment(EnvironmentConfig::desktop("bob", "bob-windows"))
        .unwrap();
    (world, alice, bob)
}
fn action(
    world: &mut World,
    session: &str,
    machine: &str,
    family: &str,
    op: &str,
    payload: Value,
) -> Value {
    let result = world
        .step(
            session,
            vec![ActionEnvelope::new(family, op, machine, payload)],
        )
        .unwrap();
    assert!(result.outcomes[0].success, "{:?}", result.outcomes[0]);
    result.outcomes[0].value.clone()
}
fn http(
    world: &mut World,
    session: &str,
    machine: &str,
    method: &str,
    url: &str,
    body: Value,
) -> cw_protocol::HttpResponse {
    let req = HttpRequest::json(method, url, &body).unwrap();
    serde_json::from_value(action(
        world,
        session,
        machine,
        "http.v1",
        "request",
        serde_json::to_value(req).unwrap(),
    ))
    .unwrap()
}
fn shell(world: &mut World, session: &str, machine: &str, command: &str) -> String {
    let out = action(
        world,
        session,
        machine,
        "terminal.v1",
        "execute",
        json!({"command":command}),
    );
    assert_eq!(out["exit_code"], 0, "{out}");
    out["stdout"].as_str().unwrap().into()
}

#[test]
fn unrelated_world_runs_without_reference_services() {
    let definition = cw_protocol::WorldDefinition::from_json(include_str!(
        "../../../worlds/unrelated-lab/world.json"
    ))
    .unwrap();
    let mut world = World::new(definition, 9).unwrap();
    let actor = world
        .environment(EnvironmentConfig::terminal("researcher", "lab"))
        .unwrap();
    assert_eq!(shell(&mut world, &actor, "lab", "cat input.txt"), "7\n");
}

#[test]
fn local_files_are_isolated_and_actor_derives_new_file() {
    let (mut w, a, b) = setup(3);
    let input = shell(&mut w, &a, "alice-mac", "cat launch.txt");
    assert!(input.contains("Atlas"));
    let derived = input.replace("Status: pending", "Status: reviewed");
    action(
        &mut w,
        &a,
        "alice-mac",
        "filesystem.v1",
        "write",
        json!({"path":"status.txt","content":derived}),
    );
    assert!(shell(&mut w, &a, "alice-mac", "cat status.txt").contains("Status: reviewed"));
    let result = w
        .step(
            &b,
            vec![ActionEnvelope::new(
                "filesystem.v1",
                "read",
                "bob-windows",
                json!({"path":"status.txt"}),
            )],
        )
        .unwrap();
    assert!(!result.outcomes[0].success);
}

#[test]
fn browser_discovers_services_through_received_links() {
    let (mut w, a, _) = setup(3);
    let page = action(
        &mut w,
        &a,
        "alice-mac",
        "browser.v1",
        "navigate",
        json!({"url":"http://intranet.internal/"}),
    );
    assert!(page.to_string().contains("Northstar"));
    let mail = action(
        &mut w,
        &a,
        "alice-mac",
        "browser.v1",
        "click",
        json!({"id":"mail"}),
    );
    assert!(mail.to_string().contains("Atlas launch checklist"));
    let trace = serde_json::to_string(&w.trajectory()).unwrap();
    assert!(trace.contains("dns"));
    assert!(trace.contains("http"));
    assert!(trace.contains("intranet.internal"));
}

#[test]
fn document_mutation_crosses_network_and_is_visible_to_second_computer() {
    let (mut w, a, b) = setup(4);
    let mail = http(
        &mut w,
        &a,
        "alice-mac",
        "GET",
        "http://mail.internal/api/messages",
        Value::Null,
    );
    assert_eq!(mail.status, 200);
    assert!(String::from_utf8_lossy(&mail.body).contains("ATLAS-2026"));
    let changed = http(
        &mut w,
        &a,
        "alice-mac",
        "PUT",
        "http://docs.internal/api/documents/launch",
        json!({"body":"Confirmed ATLAS-2026 from mail; status reviewed.","revision":1}),
    );
    assert!(
        (200..300).contains(&changed.status),
        "{}",
        String::from_utf8_lossy(&changed.body)
    );
    let seen = http(
        &mut w,
        &b,
        "bob-windows",
        "GET",
        "http://docs.internal/api/documents/launch",
        Value::Null,
    );
    assert_eq!(seen.status, 200);
    assert!(String::from_utf8_lossy(&seen.body).contains("Confirmed ATLAS-2026"));
}

#[test]
fn undeclared_host_network_and_ungranted_machine_are_blocked() {
    let (mut w, a, _) = setup(3);
    let bad = w
        .step(
            &a,
            vec![
                ActionEnvelope::new(
                    "http.v1",
                    "request",
                    "alice-mac",
                    serde_json::to_value(HttpRequest::get("https://example.com/")).unwrap(),
                ),
                ActionEnvelope::new(
                    "filesystem.v1",
                    "read",
                    "bob-windows",
                    json!({"path":"notes.txt"}),
                ),
            ],
        )
        .unwrap();
    assert!(bad.outcomes.iter().all(|o| !o.success));
}

#[test]
fn snapshot_branch_and_portable_restore_preserve_future_behavior() {
    let (mut w, a, _) = setup(7);
    action(
        &mut w,
        &a,
        "alice-mac",
        "browser.v1",
        "navigate",
        json!({"url":"http://intranet.internal/"}),
    );
    let checkpoint = w.snapshot();
    let portable = w.export_snapshot().unwrap();
    let mut branch = w.fork(&checkpoint).unwrap();
    action(
        &mut branch,
        &a,
        "alice-mac",
        "filesystem.v1",
        "write",
        json!({"path":"branch.txt","content":"branch only"}),
    );
    let missing = w
        .step(
            &a,
            vec![ActionEnvelope::new(
                "filesystem.v1",
                "read",
                "alice-mac",
                json!({"path":"branch.txt"}),
            )],
        )
        .unwrap();
    assert!(!missing.outcomes[0].success);
    w.import_snapshot(&portable).unwrap();
    branch.restore(&checkpoint).unwrap();
    let action = ActionEnvelope::new(
        "browser.v1",
        "click",
        "alice-mac",
        json!({"id":"public-guide"}),
    );
    assert_eq!(
        w.step(&a, vec![action.clone()]).unwrap(),
        branch.step(&a, vec![action]).unwrap()
    );
    assert_eq!(w.state_hash().unwrap(), branch.state_hash().unwrap());
}

#[test]
fn structured_observation_contains_no_service_backing_state() {
    let (mut w, a, _) = setup(3);
    let observation = serde_json::to_string(&w.observe(&a).unwrap()).unwrap();
    assert!(!observation.contains("ATLAS-2026"));
    assert!(!observation.contains("mailboxes"));
    action(
        &mut w,
        &a,
        "alice-mac",
        "browser.v1",
        "navigate",
        json!({"url":"http://mail.internal/"}),
    );
    let observation = serde_json::to_string(&w.observe(&a).unwrap()).unwrap();
    assert!(observation.contains("Atlas launch checklist"));
    assert!(!observation.contains("mailboxes"));
}

#[test]
fn git_push_then_clone_transfers_real_shared_files_between_machines() {
    let (mut w, a, b) = setup(11);
    shell(
        &mut w,
        &a,
        "alice-mac",
        "git clone http://git.internal/repos/onboarding onboarding",
    );
    shell(&mut w, &a, "alice-mac", "cd onboarding");
    shell(&mut w, &a, "alice-mac", "echo reviewed > checklist.txt");
    shell(&mut w, &a, "alice-mac", "git add checklist.txt");
    shell(&mut w, &a, "alice-mac", "git commit -m reviewed");
    shell(&mut w, &a, "alice-mac", "git push origin main");
    shell(
        &mut w,
        &b,
        "bob-windows",
        "git clone http://git.internal/repos/onboarding onboarding",
    );
    shell(&mut w, &b, "bob-windows", "cd onboarding");
    assert_eq!(
        shell(&mut w, &b, "bob-windows", "cat checklist.txt"),
        "reviewed\n"
    );
}

#[test]
fn chat_messages_are_shared_by_authorized_users() {
    let (mut w, a, b) = setup(11);
    let sent = http(
        &mut w,
        &a,
        "alice-mac",
        "POST",
        "http://chat.internal/api/channels/general/messages",
        json!({"text":"Atlas document is reviewed."}),
    );
    assert!(
        (200..300).contains(&sent.status),
        "{}",
        String::from_utf8_lossy(&sent.body)
    );
    let seen = http(
        &mut w,
        &b,
        "bob-windows",
        "GET",
        "http://chat.internal/api/channels/general/messages",
        Value::Null,
    );
    assert_eq!(seen.status, 200);
    assert!(String::from_utf8_lossy(&seen.body).contains("Atlas document is reviewed."));
}

#[test]
fn seeds_choose_substantive_initialized_document_content() {
    let mut variants = std::collections::BTreeSet::new();
    for seed in 0..12 {
        let (mut w, a, _) = setup(seed);
        let response = http(
            &mut w,
            &a,
            "alice-mac",
            "GET",
            "http://docs.internal/api/documents/handbook",
            Value::Null,
        );
        assert_eq!(response.status, 200);
        variants.insert(response.body.clone());
        let (mut same, actor, _) = setup(seed);
        assert_eq!(
            http(
                &mut same,
                &actor,
                "alice-mac",
                "GET",
                "http://docs.internal/api/documents/handbook",
                Value::Null
            )
            .body,
            response.body
        );
    }
    assert!(
        variants.len() > 1,
        "seeds must change initialized task-relevant facts"
    );
}

#[test]
fn recording_replays_same_actor_inputs_and_events() {
    let (mut w, a, _) = setup(42);
    let initial = w.snapshot();
    action(
        &mut w,
        &a,
        "alice-mac",
        "browser.v1",
        "navigate",
        json!({"url":"http://intranet.internal/"}),
    );
    action(
        &mut w,
        &a,
        "alice-mac",
        "browser.v1",
        "click",
        json!({"id":"mail"}),
    );
    let expected_hash = w.state_hash().unwrap();
    let expected_events = w.trajectory();
    let journal = w.interfaces().action_journal().clone();
    w.interfaces_mut().replay(&initial, &journal).unwrap();
    assert_eq!(w.state_hash().unwrap(), expected_hash);
    assert_eq!(w.trajectory(), expected_events);
}

#[cfg(feature = "render")]
#[test]
fn scene_hit_testing_drives_browser_and_rendering_is_deterministic() {
    let (mut w, a, _) = setup(19);
    action(
        &mut w,
        &a,
        "alice-mac",
        "browser.v1",
        "navigate",
        json!({"url":"http://intranet.internal/"}),
    );
    let hash = w.state_hash().unwrap();
    let scene = w.scene(&a, 800, 600).unwrap();
    let node = scene
        .nodes
        .iter()
        .find(|n| n.interaction.as_deref() == Some("mail"))
        .unwrap();
    let (x, y) = node.transform.point(node.bounds.x + 3, node.bounds.y + 3);
    assert_eq!(
        scene.hit_test(x, y).unwrap().interaction.as_deref(),
        Some("mail")
    );
    let first = w.render(&a, 800, 600).unwrap();
    let second = w.render(&a, 800, 600).unwrap();
    assert_eq!(first.rgba, second.rgba);
    assert_eq!(
        w.state_hash().unwrap(),
        hash,
        "render/observe must not mutate semantics"
    );
    let clicked = action(
        &mut w,
        &a,
        "alice-mac",
        "pointer.v1",
        "click",
        json!({"x":x,"y":y,"width":800,"height":600}),
    );
    assert!(clicked.to_string().contains("Atlas launch checklist"));
    assert_ne!(w.render(&a, 800, 600).unwrap().rgba, first.rgba);
}

#[test]
fn browser_form_mutation_requires_other_client_refresh() {
    let (mut w, a, b) = setup(8);
    action(
        &mut w,
        &b,
        "bob-windows",
        "browser.v1",
        "navigate",
        json!({"url":"http://docs.internal/documents/launch"}),
    );
    action(
        &mut w,
        &a,
        "alice-mac",
        "browser.v1",
        "navigate",
        json!({"url":"http://mail.internal/"}),
    );
    let page = w.observe(&a).unwrap().channels["semantic.v1"]["alice-mac"].clone();
    let link = page["elements"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["url"] == "http://docs.internal/documents/launch")
        .unwrap()["id"]
        .clone();
    action(
        &mut w,
        &a,
        "alice-mac",
        "browser.v1",
        "click",
        json!({"id":link}),
    );
    action(
        &mut w,
        &a,
        "alice-mac",
        "browser.v1",
        "fill",
        json!({"id":"edit-body","value":"Browser-only confirmation ATLAS-2026"}),
    );
    action(
        &mut w,
        &a,
        "alice-mac",
        "browser.v1",
        "submit",
        json!({"id":"edit"}),
    );
    assert!(!serde_json::to_string(&w.observe(&b).unwrap())
        .unwrap()
        .contains("Browser-only confirmation"));
    let refreshed = action(&mut w, &b, "bob-windows", "browser.v1", "reload", json!({}));
    assert!(refreshed
        .to_string()
        .contains("Browser-only confirmation ATLAS-2026"));
}

#[test]
fn removing_service_link_changes_navigation_to_visible_error() {
    let mut definition = reference_world();
    definition
        .network
        .links
        .retain(|link| link.from != "mail-node" && link.to != "mail-node");
    let mut w = World::new(definition, 8).unwrap();
    let a = w
        .environment(EnvironmentConfig::desktop("alice", "alice-mac"))
        .unwrap();
    let result = w
        .step(
            &a,
            vec![ActionEnvelope::new(
                "browser.v1",
                "navigate",
                "alice-mac",
                json!({"url":"http://mail.internal/"}),
            )],
        )
        .unwrap();
    assert!(!result.outcomes[0].success);
    assert!(!serde_json::to_string(&w.observe(&a).unwrap())
        .unwrap()
        .contains("ATLAS-2026"));
    assert!(w.trajectory().iter().any(|e| e.kind == "http.rejected"));
}

#[test]
fn computer_process_identifiers_are_local_namespaces() {
    let (mut w, a, b) = setup(5);
    let alice = action(
        &mut w,
        &a,
        "alice-mac",
        "terminal.v1",
        "execute",
        json!({"command":"echo alice"}),
    );
    let bob = action(
        &mut w,
        &b,
        "bob-windows",
        "terminal.v1",
        "execute",
        json!({"command":"echo bob"}),
    );
    assert_eq!(
        alice["pid"], bob["pid"],
        "new computers allocate independent local process IDs"
    );
    assert_eq!(alice["stdout"], "alice\n");
    assert_eq!(bob["stdout"], "bob\n");
    let alice_next = action(
        &mut w,
        &a,
        "alice-mac",
        "terminal.v1",
        "execute",
        json!({"command":"echo next"}),
    );
    assert!(alice_next["pid"].as_u64().unwrap() > alice["pid"].as_u64().unwrap());
}

#[test]
fn admin_debugs_service_process_then_second_computer_reconnects() {
    let (mut w, a, _) = setup(5);
    let admin = w
        .environment(EnvironmentConfig::terminal("admin", "app-server"))
        .unwrap();
    shell(&mut w, &admin, "app-server", "systemctl start intranet");
    assert!(shell(&mut w, &admin, "app-server", "systemctl status intranet").contains("active"));
    shell(&mut w, &admin, "app-server", "systemctl stop intranet");
    let failed = w
        .step(
            &a,
            vec![ActionEnvelope::new(
                "browser.v1",
                "navigate",
                "alice-mac",
                json!({"url":"http://intranet.internal/"}),
            )],
        )
        .unwrap();
    assert!(!failed.outcomes[0].success);
    shell(&mut w, &admin, "app-server", "systemctl start intranet");
    let page = action(
        &mut w,
        &a,
        "alice-mac",
        "browser.v1",
        "navigate",
        json!({"url":"http://intranet.internal/"}),
    );
    assert!(page.to_string().contains("Northstar Workshop"));
}

#[cfg(feature = "render")]
#[test]
fn native_image_is_fetched_over_network_and_survives_portable_checkpoint() {
    use cw_protocol::{HttpResponse, Page, PageElement, Result};
    use cw_sdk::{Service, ServiceContext};
    struct ImageSite;
    impl Service for ImageSite {
        fn kind(&self) -> &str {
            "test.image-site"
        }
        fn handle(
            &self,
            _: &mut Value,
            _: &ServiceContext,
            request: &HttpRequest,
        ) -> Result<HttpResponse> {
            if request.url.ends_with("/asset.rgba") {
                let mut response = HttpResponse::json(
                    200,
                    &json!({"width":2,"height":2,"rgba":[255,0,0,255,0,255,0,255,0,0,255,255,255,255,0,255]}),
                )?;
                response.headers.insert(
                    "content-type".into(),
                    "application/vnd.computerworld.rgba+json".into(),
                );
                return Ok(response);
            }
            let mut page = Page::new("Network image");
            page.elements.push(PageElement::Image {
                id: "test-image".into(),
                source: "/asset.rgba".into(),
                alt: "Four colored pixels".into(),
                width: 20,
                height: 20,
                style: None,
                action: None,
            });
            HttpResponse::page(&page)
        }
    }
    let mut definition = cw_protocol::WorldDefinition::from_json(include_str!(
        "../../../worlds/unrelated-lab/world.json"
    ))
    .unwrap();
    definition.network.nodes.push(cw_protocol::NetworkNode {
        id: "image-node".into(),
        address: "192.0.2.20".into(),
        zone: cw_protocol::NetworkZone::Local,
    });
    definition.network.links.push(cw_protocol::NetworkLink {
        from: "lab".into(),
        to: "image-node".into(),
        bidirectional: true,
        latency_us: 7,
        loss_per_million: 0,
    });
    definition.network.dns.push(cw_protocol::DnsRecord {
        name: "images.internal".into(),
        address: "192.0.2.20".into(),
        ttl_us: 1_000_000,
        resolver: None,
    });
    definition.services.push(cw_protocol::ServiceDefinition {
        id: "images".into(),
        kind: "test.image-site".into(),
        node: "image-node".into(),
        domains: vec!["images.internal".into()],
        port: 80,
        tls: true,
        initial_state: Value::Null,
    });
    let mut registry = cw_sdk::Registry::new();
    registry.register(ImageSite).unwrap();
    let mut world = World::with_registry(definition, 2, registry).unwrap();
    let actor = world
        .environment(EnvironmentConfig::desktop("researcher", "lab"))
        .unwrap();
    action(
        &mut world,
        &actor,
        "lab",
        "browser.v1",
        "navigate",
        json!({"url":"http://images.internal/"}),
    );
    let requests: Vec<_> = world
        .trajectory()
        .into_iter()
        .filter(|e| e.kind == "http.submitted")
        .collect();
    assert_eq!(
        requests.len(),
        2,
        "page and image each pass through the causal HTTP path"
    );
    assert!(requests
        .iter()
        .any(|e| e.data["request"]["url"] == "http://images.internal/asset.rgba"));
    let scene = world.scene(&actor, 320, 240).unwrap();
    let node = scene
        .nodes
        .iter()
        .find(|n| matches!(n.primitive, cw_scene::Primitive::Image { .. }))
        .expect("received asset produces image primitive");
    let cw_scene::Primitive::Image {
        width,
        height,
        rgba,
    } = &node.primitive
    else {
        unreachable!()
    };
    assert_eq!((*width, *height), (2, 2));
    assert_eq!(&rgba[..4], &[255, 0, 0, 255]);
    let (x, y) = node.transform.point(node.bounds.x + 1, node.bounds.y + 1);
    let frame = world.render(&actor, 320, 240).unwrap();
    assert_eq!(frame.pixel(x as u32, y as u32), Some([255, 0, 0, 255]));
    let portable = world.export_snapshot().unwrap();
    let hash = world.state_hash().unwrap();
    world.reset(999).unwrap();
    world.import_snapshot(&portable).unwrap();
    assert_eq!(world.state_hash().unwrap(), hash);
    assert_eq!(world.scene(&actor, 320, 240).unwrap(), scene);
    assert_eq!(world.render(&actor, 320, 240).unwrap().rgba, frame.rgba);
    assert_eq!(
        world
            .trajectory()
            .iter()
            .filter(|e| e.kind == "http.submitted")
            .count(),
        2,
        "restore/render must use checkpointed asset, not refetch"
    );
}

#[test]
fn package_removal_and_installation_control_application_launch() {
    let mut definition = cw_protocol::WorldDefinition::from_json(include_str!(
        "../../../worlds/unrelated-lab/world.json"
    ))
    .unwrap();
    definition.computers[0].packages.push("editor".into());
    let mut w = World::new(definition, 1).unwrap();
    let actor = w
        .environment(EnvironmentConfig::desktop("researcher", "lab"))
        .unwrap();
    action(
        &mut w,
        &actor,
        "lab",
        "application.v1",
        "launch",
        json!({"kind":"editor","argument":"input.txt"}),
    );
    shell(&mut w, &actor, "lab", "apt remove editor");
    let denied = w
        .step(
            &actor,
            vec![ActionEnvelope::new(
                "application.v1",
                "launch",
                "lab",
                json!({"kind":"editor","argument":"input.txt"}),
            )],
        )
        .unwrap();
    assert!(
        !denied.outcomes[0].success,
        "removed application must not launch"
    );
    shell(&mut w, &actor, "lab", "apt install editor");
    action(
        &mut w,
        &actor,
        "lab",
        "application.v1",
        "launch",
        json!({"kind":"editor","argument":"input.txt"}),
    );
    let unrelated = w
        .step(
            &actor,
            vec![ActionEnvelope::new(
                "application.v1",
                "launch",
                "lab",
                json!({"kind":"browser"}),
            )],
        )
        .unwrap();
    assert!(
        !unrelated.outcomes[0].success,
        "uninstalled builtin application must not launch"
    );
}

#[test]
fn live_devices_preserve_state_revoke_grants_and_restore_topology() {
    use computerworld::{NetworkLink, NetworkNode, NetworkZone};
    let (mut world, alice, _) = setup(77);
    shell(
        &mut world,
        &alice,
        "alice-mac",
        "echo retained > /home/alice/retained.txt",
    );
    let before = world.snapshot();
    let mut phone = world.definition().computers[0].clone();
    phone.id = "phone".into();
    phone.node = "phone".into();
    phone.address = "10.0.0.99".into();
    let node = NetworkNode {
        id: phone.id.clone(),
        address: phone.address.clone(),
        zone: NetworkZone::Local,
    };
    let link = NetworkLink {
        from: "phone".into(),
        to: "alice-mac".into(),
        bidirectional: true,
        latency_us: 10,
        loss_per_million: 0,
    };
    world
        .add_computer(phone.clone(), node.clone(), vec![link.clone()])
        .unwrap();
    let phone_session = world
        .environment(EnvironmentConfig::desktop("alice", "phone"))
        .unwrap();
    assert_eq!(
        world
            .runtime()
            .read_file("alice-mac", "/home/alice/retained.txt")
            .unwrap(),
        b"retained\n"
    );
    assert!(world
        .runtime()
        .read_file("phone", "/home/alice/retained.txt")
        .is_err());
    let response = http(
        &mut world,
        &phone_session,
        "phone",
        "GET",
        "http://intranet.internal/",
        Value::Null,
    );
    assert_eq!(response.status, 200);
    let active = world.snapshot();
    let portable = world.export_snapshot().unwrap();
    let active_hash = world.state_hash().unwrap();
    assert!(world.add_computer(phone, node, vec![link]).is_err());
    assert_eq!(
        world.state_hash().unwrap(),
        active_hash,
        "failed edit must be atomic"
    );
    world.remove_computer("phone").unwrap();
    assert!(world.validate_session(&phone_session).is_err());
    assert!(world.runtime().computer("phone").is_err());
    assert!(!world
        .runtime()
        .network()
        .config
        .nodes
        .iter()
        .any(|n| n.id == "phone"));
    world.restore(&active).unwrap();
    assert_eq!(world.state_hash().unwrap(), active_hash);
    assert!(world.validate_session(&phone_session).is_ok());
    let mut receiver = World::new(reference_world(), 77).unwrap();
    receiver.import_snapshot(&portable).unwrap();
    assert_eq!(receiver.state_hash().unwrap(), active_hash);
    let fork = receiver.fork(&active).unwrap();
    assert_eq!(fork.state_hash().unwrap(), active_hash);
    receiver.restore(&before).unwrap();
    assert!(receiver.runtime().computer("phone").is_err());
    world.reset(77).unwrap();
    assert!(world.runtime().computer("phone").is_err());
    assert!(world.validate_session(&phone_session).is_err());
    assert!(world.validate_session(&alice).is_ok());
    assert!(world
        .runtime()
        .read_file("alice-mac", "/home/alice/retained.txt")
        .is_err());
}

#[test]
fn removing_service_host_is_atomic_and_multimachine_grants_shrink() {
    let (mut world, _, _) = setup(7);
    let hash = world.state_hash().unwrap();
    assert!(world.remove_computer("git-server").is_err());
    assert_eq!(world.state_hash().unwrap(), hash);
    let mut config = EnvironmentConfig::desktop("alice", "alice-mac");
    config.machines.push("bob-windows".into());
    let session = world.environment(config).unwrap();
    world.remove_computer("alice-mac").unwrap();
    let config = &world.interfaces().session(&session).unwrap().config;
    assert_eq!(config.machines, vec!["bob-windows"]);
    world.observe(&session).unwrap();
    let outcome = world
        .step(
            &session,
            vec![ActionEnvelope::new(
                "terminal.v1",
                "execute",
                "alice-mac",
                json!({"command":"pwd"}),
            )],
        )
        .unwrap();
    assert!(!outcome.outcomes[0].success);
}
