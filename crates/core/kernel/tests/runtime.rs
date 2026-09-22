use cw_kernel::{Runtime, Snapshot};
use cw_protocol::*;
use cw_sdk::*;
use serde_json::{json, Value};

struct Counter;
impl Service for Counter {
    fn kind(&self) -> &str {
        "counter"
    }
    fn initialize(&self, _: Value, c: &ServiceContext) -> Result<Value> {
        Ok(json!({"count":0,"seed":c.seed}))
    }
    fn handle(&self, s: &mut Value, _: &ServiceContext, r: &HttpRequest) -> Result<HttpResponse> {
        if r.method == "POST" {
            s["count"] = json!(s["count"].as_u64().unwrap() + 1);
        }
        if r.url.ends_with("/fail") {
            s["count"] = json!(999);
            return Err(SimError::invalid("transaction failed"));
        }
        HttpResponse::json(200, s)
    }
}
struct Relay;
impl Service for Relay {
    fn kind(&self) -> &str {
        "relay"
    }
    fn handle(&self, _: &mut Value, _: &ServiceContext, _: &HttpRequest) -> Result<HttpResponse> {
        Ok(HttpResponse::text(202, "queued"))
    }
    fn handle_with_effects(
        &self,
        _: &mut Value,
        _: &ServiceContext,
        _: &HttpRequest,
    ) -> Result<ServiceTransition> {
        Ok(ServiceTransition {
            response: HttpResponse::text(202, "queued"),
            effects: vec![
                ServiceEffect::Http {
                    request: HttpRequest::json("POST", "http://counter.test/", &json!({}))?,
                    reply_token: "reply".into(),
                },
                ServiceEffect::Schedule {
                    delay_us: 20,
                    token: "timer".into(),
                    data: json!(7),
                },
            ],
        })
    }
    fn on_effect(
        &self,
        s: &mut Value,
        _: &ServiceContext,
        token: &str,
        result: &ServiceEffectResult,
    ) -> Result<Vec<ServiceEffect>> {
        s[token] = serde_json::to_value(result)?;
        Ok(vec![])
    }
}
fn definition() -> WorldDefinition {
    serde_json::from_value(json!({"id":"test","profiles":[{"id":"linux","family":"linux"}],"computers":[{"id":"a","profile":"linux","address":"10.0.0.1","user":"alice","initial_files":{"/home/alice/note":"one"}},{"id":"b","profile":"linux","address":"10.0.0.2","user":"bob"}],"network":{"nodes":[{"id":"server","address":"10.0.0.3"}],"links":[{"from":"a","to":"server","latency_us":5},{"from":"b","to":"server","latency_us":5}]},"services":[{"id":"counter","kind":"counter","node":"server","domains":["counter.test"],"initial_state":{}},{"id":"relay","kind":"relay","node":"server","port":8080,"domains":["relay.test"],"initial_state":{}}]})).unwrap()
}
fn world(seed: u64) -> Runtime {
    let mut registry = Registry::new();
    registry.register(Counter).unwrap();
    registry.register(Relay).unwrap();
    Runtime::new(definition(), seed, registry).unwrap()
}
#[test]
fn machines_network_persistence_and_isolation() {
    let mut r = world(5);
    let response = r
        .http(
            "a",
            "alice",
            HttpRequest::json("POST", "http://counter.test/", &json!({})).unwrap(),
        )
        .unwrap();
    assert_eq!(response.status, 200);
    let response = r
        .http("b", "bob", HttpRequest::get("http://counter.test/"))
        .unwrap();
    assert_eq!(
        serde_json::from_slice::<Value>(&response.body).unwrap()["count"],
        1
    );
    assert_eq!(r.read_file("a", "note").unwrap(), b"one");
    assert!(r.read_file("b", "note").is_err());
    r.execute("a", "alice", "echo changed > note").unwrap();
    assert_eq!(r.read_file("a", "note").unwrap(), b"changed\n");
    assert!(r.read_file("b", "note").is_err());
    assert!(!r.network().traces().is_empty());
    assert!(r.events().iter().any(|e| e.kind == "http.completed"));
}
#[test]
fn pending_snapshot_includes_service_commit_and_response_delivery() {
    let mut r = world(7);
    let id = r
        .submit_http(
            "a",
            "alice",
            HttpRequest::json("POST", "http://counter.test/", &json!({})).unwrap(),
        )
        .unwrap();
    let before = r.snapshot();
    assert_eq!(r.service_state("counter").unwrap()["count"], 0);
    r.advance(5).unwrap();
    assert_eq!(r.service_state("counter").unwrap()["count"], 1);
    assert!(r.take_response(&id).unwrap().is_none());
    let in_flight = Snapshot::from_json(&r.snapshot().to_json().unwrap()).unwrap();
    r.advance(100).unwrap();
    let response = r.take_response(&id).unwrap().unwrap();
    let hash = r.state_hash().unwrap();
    r.restore(&in_flight).unwrap();
    r.advance(100).unwrap();
    assert_eq!(r.take_response(&id).unwrap().unwrap(), response);
    assert_eq!(r.state_hash().unwrap(), hash);
    r.restore(&before).unwrap();
    assert_eq!(r.service_state("counter").unwrap()["count"], 0);
    r.advance(105).unwrap();
    assert_eq!(r.take_response(&id).unwrap().unwrap(), response);
}
#[test]
fn fork_reset_and_seed_initialization() {
    let mut a = world(8);
    let snap = a.snapshot();
    let mut b = a.fork(&snap).unwrap();
    a.write_file("a", "alice", "note", b"branch").unwrap();
    assert_eq!(b.read_file("a", "note").unwrap(), b"one");
    a.reset(8).unwrap();
    assert_eq!(a.state_hash().unwrap(), b.state_hash().unwrap());
    b.reset(9).unwrap();
    assert_ne!(
        a.service_state("counter").unwrap(),
        b.service_state("counter").unwrap()
    );
}
#[test]
fn failed_service_transition_rolls_back() {
    let mut r = world(2);
    assert!(r
        .http("a", "alice", HttpRequest::get("http://counter.test/fail"))
        .is_err());
    assert_eq!(r.service_state("counter").unwrap()["count"], 0);
}
#[test]
fn service_effects_traverse_network_and_resume_timers() {
    let mut r = world(2);
    r.http("a", "alice", HttpRequest::get("http://relay.test:8080/"))
        .unwrap();
    let snap = r.snapshot();
    r.advance(100).unwrap();
    assert_eq!(r.service_state("counter").unwrap()["count"], 1);
    assert_eq!(r.service_state("relay").unwrap()["timer"]["kind"], "timer");
    assert!(r.service_state("relay").unwrap()["reply"].is_object());
    let hash = r.state_hash().unwrap();
    r.restore(&snap).unwrap();
    r.advance(100).unwrap();
    assert_eq!(r.state_hash().unwrap(), hash);
}
#[test]
fn deterministic_commands_and_blocked_host() {
    let mut a = world(88);
    let mut b = world(88);
    for r in [&mut a, &mut b] {
        r.execute("a", "alice", "echo hi > out").unwrap();
        r.http("b", "bob", HttpRequest::get("http://counter.test/"))
            .unwrap();
        assert!(r
            .http("a", "alice", HttpRequest::get("https://example.com/"))
            .is_err());
    }
    assert_eq!(a.state_hash().unwrap(), b.state_hash().unwrap());
    assert_eq!(a.events(), b.events());
}
#[test]
fn restore_rejects_other_blueprint() {
    let a = world(1);
    let mut d = definition();
    d.id = "different".into();
    let mut registry = Registry::new();
    registry.register(Counter).unwrap();
    registry.register(Relay).unwrap();
    let mut b = Runtime::new(d, 1, registry).unwrap();
    let hash = b.state_hash().unwrap();
    assert!(b.restore(&a.snapshot()).is_err());
    assert_eq!(b.state_hash().unwrap(), hash);
}
#[test]
fn explicit_host_policy_and_stale_completion() {
    let mut d = definition();
    d.network.gateway.allow_host = true;
    d.network.gateway.host_allowlist = vec!["public.example".into()];
    d.network.gateway.allowed_cidrs = vec!["203.0.113.0/24".into()];
    d.network.gateway.sources = vec!["a".into()];
    d.network.gateway.schemes = vec!["https".into()];
    d.network.gateway.ports = vec![443];
    let mut registry = Registry::new();
    registry.register(Counter).unwrap();
    registry.register(Relay).unwrap();
    let mut r = Runtime::new(d, 8, registry).unwrap();
    let initial = r.snapshot();
    assert!(r
        .begin_external(
            "b",
            "bob",
            HttpRequest::get("https://public.example/"),
            vec!["203.0.113.8".into()]
        )
        .is_err());
    let live = r
        .begin_external(
            "a",
            "alice",
            HttpRequest::get("https://public.example/"),
            vec!["203.0.113.8".into()],
        )
        .unwrap();
    assert!(r.try_snapshot().is_err());
    assert!(r.fork(&r.snapshot()).is_err());
    assert!(r.take_response(&live.id).unwrap().is_none());
    r.restore(&initial).unwrap();
    assert!(r
        .complete_external(&live, Ok(HttpResponse::text(200, "stale")))
        .is_err());
    let fresh = r
        .begin_external(
            "a",
            "alice",
            HttpRequest::get("https://public.example/"),
            vec!["203.0.113.8".into()],
        )
        .unwrap();
    assert_eq!(live.id, fresh.id);
    assert_ne!(live.generation, fresh.generation);
    assert!(r
        .complete_external(&live, Ok(HttpResponse::text(200, "stale")))
        .is_err());
    r.complete_external(&fresh, Ok(HttpResponse::text(200, "recorded")))
        .unwrap();
    assert_eq!(
        r.take_response(&fresh.id).unwrap().unwrap().body,
        b"recorded"
    );
    assert!(r.try_snapshot().is_ok());
    assert!(r.events().iter().any(|e| e.kind == "external.completed"));
}
#[test]
fn colocated_process_controls_reachability_and_restart_preserves_state() {
    let mut d = definition();
    d.computers.push(
        serde_json::from_value(
            json!({"id":"server","profile":"linux","address":"10.0.0.3","user":"admin"}),
        )
        .unwrap(),
    );
    let mut registry = Registry::new();
    registry.register(Counter).unwrap();
    registry.register(Relay).unwrap();
    let mut r = Runtime::new(d, 9, registry).unwrap();
    assert_eq!(
        r.execute("server", "admin", "systemctl status counter")
            .unwrap()
            .exit_code,
        0
    );
    r.http(
        "a",
        "alice",
        HttpRequest::json("POST", "http://counter.test/", &json!({})).unwrap(),
    )
    .unwrap();
    assert_eq!(
        r.execute("server", "admin", "systemctl stop counter")
            .unwrap()
            .exit_code,
        0
    );
    assert!(r
        .http("a", "alice", HttpRequest::get("http://counter.test/"))
        .is_err());
    assert_eq!(
        r.execute("server", "admin", "systemctl start counter")
            .unwrap()
            .exit_code,
        0
    );
    let response = r
        .http("a", "alice", HttpRequest::get("http://counter.test/"))
        .unwrap();
    assert_eq!(
        serde_json::from_slice::<Value>(&response.body).unwrap()["count"],
        1
    );
}
#[test]
fn process_deadlines_are_independent_of_advance_partition() {
    let mut one = world(7);
    let mut split = world(7);
    let a = one.execute("a", "alice", "sleep 0.000025 &").unwrap();
    let b = split.execute("a", "alice", "sleep 0.000025 &").unwrap();
    assert_eq!(a.exit_code, 0);
    assert_eq!(a.pid, b.pid);
    one.advance(100).unwrap();
    split.advance(50).unwrap();
    split.advance(50).unwrap();
    assert_eq!(
        one.computer("a")
            .unwrap()
            .processes
            .get(a.pid)
            .unwrap()
            .ended,
        Some(25)
    );
    assert_eq!(one.state_hash().unwrap(), split.state_hash().unwrap());
    let before = one.tick();
    let result = one.execute("a", "alice", "sleep 0.001").unwrap();
    assert_eq!(result.exit_code, 0);
    assert_eq!(one.tick(), before + 1000);
}
#[test]
fn queued_request_cannot_reach_stopped_service() {
    let mut d = definition();
    d.computers.push(
        serde_json::from_value(
            json!({"id":"server","profile":"linux","address":"10.0.0.3","user":"admin"}),
        )
        .unwrap(),
    );
    let mut registry = Registry::new();
    registry.register(Counter).unwrap();
    registry.register(Relay).unwrap();
    let mut r = Runtime::new(d, 7, registry).unwrap();
    let id = r
        .submit_http(
            "a",
            "alice",
            HttpRequest::json("POST", "http://counter.test/", &json!({})).unwrap(),
        )
        .unwrap();
    r.execute("server", "admin", "systemctl stop counter")
        .unwrap();
    r.advance(100).unwrap();
    assert!(r.take_response(&id).is_err());
    assert_eq!(r.service_state("counter").unwrap()["count"], 0);
}
#[test]
fn rejected_external_completions_release_tokens_and_record_only_validated_failures() {
    let mut d = definition();
    d.network.gateway.allow_host = true;
    d.network.gateway.host_allowlist = vec!["public.example".into()];
    d.network.gateway.allowed_cidrs = vec!["203.0.113.0/24".into()];
    d.network.gateway.sources = vec!["a".into()];
    d.network.gateway.schemes = vec!["https".into()];
    d.network.gateway.ports = vec![443];
    d.network.gateway.timeout_us = 10;
    d.network.gateway.max_response_bytes = 4;
    let mut registry = Registry::new();
    registry.register(Counter).unwrap();
    registry.register(Relay).unwrap();
    let mut runtime = Runtime::new(d, 8, registry).unwrap();
    for (elapsed, response, expected_code) in [
        (11, HttpResponse::text(200, "late"), "timeout"),
        (0, HttpResponse::text(200, "oversized body"), "denied"),
        (0, HttpResponse::text(600, "bad"), "invalid"),
        (0, HttpResponse::text(99, "bad"), "invalid"),
    ] {
        let effect = runtime
            .begin_external(
                "a",
                "alice",
                HttpRequest::get("https://public.example/"),
                vec!["203.0.113.8".into()],
            )
            .unwrap();
        assert!(runtime.try_snapshot().is_err());
        runtime.advance(elapsed).unwrap();
        runtime.complete_external(&effect, Ok(response)).unwrap();
        let checkpoint = runtime.try_snapshot().unwrap();
        assert_eq!(
            runtime.take_response(&effect.id).unwrap_err().code,
            expected_code
        );
        // Portable restore retains the validated failure without a live adapter token.
        runtime
            .restore(&Snapshot::from_json(&checkpoint.to_json().unwrap()).unwrap())
            .unwrap();
        assert_eq!(
            runtime.take_response(&effect.id).unwrap_err().code,
            expected_code
        );
        let event = runtime.events().last().unwrap();
        assert_eq!(event.kind, "external.completed");
        assert_eq!(event.data["response"]["Err"]["code"], expected_code);
        assert!(!serde_json::to_string(&event.data)
            .unwrap()
            .contains("oversized body"));
        assert!(runtime
            .complete_external(&effect, Ok(HttpResponse::text(200, "dup")))
            .is_err());
    }
}
