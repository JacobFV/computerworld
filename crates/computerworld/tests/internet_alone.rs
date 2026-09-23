//! The built-in internet stands on its own: in a world with none of the reference company in
//! it, every site boots and every site's front page answers a machine that asks for it.
use computerworld::{World, WorldDefinition};
use cw_protocol::{ActionEnvelope, EnvironmentConfig, HttpRequest, HttpResponse};
use serde_json::json;

#[test]
fn every_site_answers_in_a_world_with_no_company() {
    let mut definition =
        WorldDefinition::from_json(include_str!("../../../worlds/unrelated-lab/world.json"))
            .unwrap();
    definition.internet = true;
    definition.network.gateway.allow_internet = true;
    // The lab sits on a documentation address the backbone uses; put it on a LAN.
    definition.computers[0].address = "10.0.0.5".into();
    let machine = definition.computers[0].id.clone();
    let actor = definition.computers[0].user.clone();
    let mut world = World::new(definition, 3).expect("the internet boots without the company");
    let session = world
        .environment(EnvironmentConfig {
            actor,
            machines: vec![machine.clone()],
            actions: vec!["http.v1".into()],
            observations: vec!["semantic.v1".into()],
            action_budget: 1 << 20,
        })
        .unwrap();
    let hosts: Vec<(String, String)> = world
        .definition()
        .services
        .iter()
        .filter_map(|s| Some((s.id.clone(), s.domains.first()?.clone())))
        .collect();
    assert_eq!(
        hosts.len(),
        computerworld::internet::definition().services.len()
    );
    let mut broken = Vec::new();
    for (id, host) in hosts {
        let request = HttpRequest::json("GET", format!("http://{host}/"), &json!({})).unwrap();
        let result = world
            .step(
                &session,
                vec![ActionEnvelope::new(
                    "http.v1",
                    "request",
                    &machine,
                    serde_json::to_value(request).unwrap(),
                )],
            )
            .unwrap();
        let outcome = &result.outcomes[0];
        match serde_json::from_value::<HttpResponse>(outcome.value.clone()) {
            Ok(r) if outcome.success && r.status < 500 => {}
            Ok(r) => broken.push(format!("{id}: {}", r.status)),
            Err(_) => broken.push(format!("{id}: {}", outcome.value)),
        }
    }
    assert!(
        broken.is_empty(),
        "sites that fail on their own: {broken:#?}"
    );
}
