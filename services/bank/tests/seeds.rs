//! The shipped northwind.example seed, exercised through the crate that serves it. Storyline 5's
//! `tx-1` -> order 1001 cross-link is pinned here, so a seed edit cannot quietly break it.
use cw_protocol::HttpRequest;
use cw_sdk::{Service, ServiceContext};
use cw_service_bank::{BankService, BankState};
use serde_json::Value;
const CHASE: &str = include_str!("../../../worlds/company-2026/sites/northwind.json");
const ORDER_1001: &str = "http://amazon.com/orders/1001";
fn ctx(actor: &str) -> ServiceContext {
    ServiceContext {
        actor: actor.into(),
        source: "alice-mac".into(),
        tick: 20,
        seed: 1,
        instance: "bank".into(),
    }
}
fn load() -> Value {
    let doc: Value = serde_json::from_str(CHASE).expect("site file must be JSON");
    assert_eq!(doc["kind"], "bank");
    BankService
        .initialize(doc["initial_state"].clone(), &ctx("alice"))
        .expect("seed must load")
}
fn get(state: &mut Value, actor: &str, url: &str) -> (u16, String) {
    let r = BankService
        .handle(state, &ctx(actor), &HttpRequest::get(url))
        .unwrap();
    (r.status, String::from_utf8(r.body).unwrap())
}
#[test]
fn storyline_5_tx_1_points_at_the_order_that_caused_it() {
    let mut state = load();
    let s: BankState = serde_json::from_value(state.clone()).unwrap();
    let tx = &s.transactions["tx-1"];
    assert_eq!(tx.account, "cc-3310");
    assert_eq!(
        tx.amount_cents, -42_999,
        "the monitor cost $429.99 in cents"
    );
    assert_eq!(tx.link, ORDER_1001);
    assert!(!tx.pending);
    let (status, body) = get(
        &mut state,
        "alice",
        "http://northwind.example/accounts/cc-3310/transactions/tx-1",
    );
    assert_eq!(status, 200);
    assert!(
        body.contains(ORDER_1001),
        "the detail must render a real outbound link: {body}"
    );
    assert!(body.contains("-$429.99"), "{body}");
}
#[test]
fn storyline_4_the_devcon_ticket_shows_on_the_card() {
    let s: BankState = serde_json::from_value(load()).unwrap();
    let tx = &s.transactions["tx-5"];
    assert_eq!(tx.amount_cents, -18_900);
    assert_eq!(tx.link, "http://ticketmaster.com/orders/TM-2209");
    assert!(tx.merchant.contains("DEVCON"), "{}", tx.merchant);
}
#[test]
fn the_card_ledger_and_its_balance_agree_to_the_cent() {
    let s: BankState = serde_json::from_value(load()).unwrap();
    let card = &s.accounts["cc-3310"];
    let ledger: i64 = s
        .transactions
        .values()
        .filter(|t| t.account == "cc-3310")
        .map(|t| t.amount_cents)
        .sum();
    assert_eq!(
        card.balance_cents, ledger,
        "the card's balance is its transactions"
    );
    assert_eq!(card.available_cents, card.limit_cents + card.balance_cents);
    assert!(
        s.next_tx > s.transactions.len() as u64,
        "a new tx must not reuse a seeded id"
    );
}
#[test]
fn every_page_the_seed_advertises_resolves_for_its_owner_and_no_one_else() {
    let mut state = load();
    for url in [
        "http://northwind.example/",
        "http://northwind.example/transfers",
        "http://northwind.example/accounts/chk-4417",
        "http://northwind.example/accounts/sav-9902",
        "http://northwind.example/accounts/cc-3310",
        "http://northwind.example/accounts/cc-3310/transactions/tx-1",
        "http://northwind.example/statements/chk-4417/2026-03",
    ] {
        assert_eq!(get(&mut state, "alice", url).0, 200, "{url}");
    }
    // Only ctx.actor's accounts are addressable; bob sees a refusal, never Alice's balance.
    for url in [
        "http://northwind.example/accounts/chk-4417",
        "http://northwind.example/accounts/cc-3310/transactions/tx-1",
        "http://northwind.example/statements/chk-4417/2026-03",
    ] {
        assert_eq!(get(&mut state, "bob", url).0, 403, "{url}");
    }
    let (status, body) = get(&mut state, "bob", "http://northwind.example/");
    assert_eq!(status, 200);
    assert!(
        !body.contains("4417"),
        "another actor's overview must not list Alice's accounts"
    );
}
