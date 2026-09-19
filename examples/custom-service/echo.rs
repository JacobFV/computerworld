//! A service plugin independent of any browser, operating system or reference world.
use cw_protocol::{HttpRequest, HttpResponse, Result};
use cw_sdk::{Registry, Service, ServiceContext};
use serde_json::{json, Value};

struct Counter;
impl Service for Counter {
    fn kind(&self) -> &str {
        "example.counter"
    }
    fn initialize(&self, initial: Value, _: &ServiceContext) -> Result<Value> {
        Ok(json!({"count": initial.get("count").and_then(Value::as_u64).unwrap_or(0)}))
    }
    fn handle(
        &self,
        state: &mut Value,
        context: &ServiceContext,
        request: &HttpRequest,
    ) -> Result<HttpResponse> {
        if request.method == "POST" {
            let count = state["count"].as_u64().unwrap_or(0);
            state["count"] = json!(count
                .checked_add(1)
                .ok_or_else(|| cw_protocol::SimError::invalid("counter overflow"))?);
        } else if request.method != "GET" {
            return Ok(HttpResponse::text(405, "use GET or POST"));
        }
        HttpResponse::json(
            200,
            &json!({"count":state["count"],"requested_by":context.actor}),
        )
    }
}
fn main() -> Result<()> {
    let mut registry = Registry::new();
    registry.register(Counter)?;
    let mut definition =
        cw_protocol::WorldDefinition::from_json(include_str!("../custom-world/world.json"))?;
    definition.network.nodes.push(cw_protocol::NetworkNode {
        id: "counter-node".into(),
        address: "192.0.2.20".into(),
        zone: cw_protocol::NetworkZone::Local,
    });
    definition.network.links.push(cw_protocol::NetworkLink {
        from: "lab".into(),
        to: "counter-node".into(),
        bidirectional: true,
        latency_us: 20,
        loss_per_million: 0,
    });
    definition.network.dns.push(cw_protocol::DnsRecord {
        name: "counter.internal".into(),
        address: "192.0.2.20".into(),
        ttl_us: 1_000_000,
        resolver: None,
    });
    definition.services.push(cw_protocol::ServiceDefinition {
        id: "counter".into(),
        kind: "example.counter".into(),
        node: "counter-node".into(),
        domains: vec!["counter.internal".into()],
        port: 80,
        tls: true,
        initial_state: json!({"count":4}),
    });
    let mut world = computerworld::World::with_registry(definition, 7, registry)?;
    let actor = world.environment(cw_protocol::EnvironmentConfig::desktop("researcher", "lab"))?;
    let request = HttpRequest::json("POST", "http://counter.internal/", &json!({}))?;
    let result = world.step(
        &actor,
        vec![cw_protocol::ActionEnvelope::new(
            "http.v1",
            "request",
            "lab",
            serde_json::to_value(request)?,
        )],
    )?;
    assert!(result.outcomes[0].success);
    let response: HttpResponse = serde_json::from_value(result.outcomes[0].value.clone())?;
    assert_eq!(serde_json::from_slice::<Value>(&response.body)?["count"], 5);
    println!("{}", String::from_utf8_lossy(&response.body));
    Ok(())
}
