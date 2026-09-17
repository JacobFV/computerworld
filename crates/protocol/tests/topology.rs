use cw_protocol::*;
use proptest::prelude::*;
use serde_json::json;
fn definition() -> WorldDefinition {
    serde_json::from_value(json!({"id":"arbitrary-world","profiles":[{"id":"custom","family":"custom"}],"computers":[{"id":"alpha","profile":"custom","address":"10.0.0.1","user":"one"},{"id":"beta","profile":"custom","address":"10.0.0.2","user":"two"}],"network":{"nodes":[{"id":"service","address":"10.0.1.1","zone":"internet"}],"links":[{"from":"alpha","to":"service"},{"from":"beta","to":"service"}],"dns":[{"name":"api.example.test","address":"10.0.1.1"}]},"services":[{"id":"arbitrary-instance","kind":"third-party","node":"service","domains":["api.example.test"]}]})).unwrap()
}
#[test]
fn arbitrary_modules_are_valid_schema() {
    definition().validate().unwrap()
}
#[test]
fn computer_node_ownership_is_unique() {
    let mut w = definition();
    w.computers[1].node = "alpha".into();
    assert!(w.validate().is_err())
}
#[test]
fn service_dns_cannot_identify_another_node() {
    let mut w = definition();
    w.network.dns[0].address = "10.0.0.1".into();
    assert!(w.validate().is_err())
}
#[test]
fn route_requires_existing_nodes() {
    let mut w = definition();
    w.network.routes.push(Route {
        from: "alpha".into(),
        to: "service".into(),
        via: Some("missing".into()),
    });
    assert!(w.validate().is_err())
}
#[test]
fn dns_resolver_is_a_real_node() {
    let mut w = definition();
    w.network.dns[0].resolver = Some("missing".into());
    assert!(w.validate().is_err())
}
#[test]
fn dns_aliases_are_supported() {
    let mut w = definition();
    w.network.dns.push(DnsRecord {
        name: "alias.test".into(),
        address: "api.example.test".into(),
        ttl_us: 100,
        resolver: None,
    });
    w.validate().unwrap()
}
#[test]
fn version_blocks_incompatible_definitions() {
    let mut w = definition();
    w.schema_version += 1;
    assert!(w.validate().is_err())
}
#[test]
fn unknown_optional_fields_are_forward_compatible() {
    let mut v = serde_json::to_value(definition()).unwrap();
    v["future_optional_hint"] = json!(true);
    let parsed: WorldDefinition = serde_json::from_value(v).unwrap();
    parsed.validate().unwrap()
}
proptest! {
 #[test]fn duplicate_ids_always_fail(id in "[a-z][a-z0-9]{0,20}") {let mut w=definition();w.computers[0].id=id.clone();w.computers[1].id=id;prop_assert!(w.validate().is_err());}
 #[test]fn reachable_definitions_roundtrip(seed in any::<u32>()) {let mut w=definition();w.metadata=json!({"arbitrary_seed":seed});let round:WorldDefinition=serde_json::from_slice(&serde_json::to_vec(&w).unwrap()).unwrap();prop_assert_eq!(&round,&w);prop_assert!(round.validate().is_ok());}
 #[test]fn malformed_addresses_are_rejected(suffix in "[a-z]{1,16}") {let mut w=definition();w.computers[0].address=format!("not-an-ip-{suffix}");prop_assert!(w.validate().is_err());}
}

#[test]
fn duplicate_page_targets_fail() {
    let mut page = Page::new("ambiguous");
    page.elements = vec![
        PageElement::Text {
            id: "same".into(),
            text: "first".into(),
        },
        PageElement::Link {
            id: "same".into(),
            text: "next".into(),
            url: "/next".into(),
        },
    ];
    assert!(page.validate().is_err());
}
#[test]
fn page_versions_are_checked() {
    let mut page = Page::new("future");
    page.version = 42;
    assert!(page.validate().is_err());
}
