//! northwind.example and paypal.com served as HTML by the bank service and driven end to
//! end through the agent API: the browser renders each with the web engine, the semantic
//! observation lists the tiles, ledger rows, inputs, buttons and forms by the ids the
//! service documents, the ledger filter is a GET whose URL is the query, a transfer and
//! a payment are POSTed from the forms, and the page that comes back shows the new balance.
use computerworld::{reference_world, World};
use cw_protocol::{ActionEnvelope, EnvironmentConfig};
use serde_json::{json, Value};

const MACHINE: &str = "alice-mac";
const ACTOR: &str = "alice";

fn world() -> (World, String) {
    let mut world = World::new(reference_world(), 42).unwrap();
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
fn act(world: &mut World, session: &str, channel: &str, op: &str, payload: Value) -> Value {
    let result = world
        .step(
            session,
            vec![ActionEnvelope::new(channel, op, MACHINE, payload)],
        )
        .unwrap();
    assert!(result.outcomes[0].success, "{op}: {:?}", result.outcomes[0]);
    result.outcomes[0].value.clone()
}
fn browser_do(world: &mut World, session: &str, op: &str, payload: Value) {
    act(world, session, "browser.v1", op, payload);
}
fn url(world: &World, session: &str) -> String {
    world.observe(session).unwrap().channels["browser.v1"][MACHINE]["url"]
        .as_str()
        .unwrap()
        .to_owned()
}
/// The semantic tree of the page on screen: its title and every element, flattened.
fn page(world: &World, session: &str) -> (String, Vec<Value>) {
    fn walk(elements: &[Value], out: &mut Vec<Value>) {
        for e in elements {
            out.push(e.clone());
            if let Some(children) = e["children"].as_array() {
                walk(children, out);
            }
        }
    }
    let page = world.observe(session).unwrap().channels["semantic.v1"][MACHINE].clone();
    let mut out = Vec::new();
    walk(page["elements"].as_array().unwrap(), &mut out);
    (page["title"].as_str().unwrap_or_default().to_owned(), out)
}
fn by_id<'a>(all: &'a [Value], id: &str) -> &'a Value {
    all.iter()
        .find(|e| e["id"] == id)
        .unwrap_or_else(|| panic!("no element {id} in {all:?}"))
}
/// Whether any element's text contains `needle`: how a person reads a balance off the page.
fn shows(all: &[Value], needle: &str) -> bool {
    all.iter()
        .any(|e| e["text"].as_str().is_some_and(|t| t.contains(needle)))
}

#[test]
fn northwind_accounts_are_read_filtered_and_a_transfer_is_made_through_the_agent_api() {
    let (mut world, session) = world();
    browser_do(
        &mut world,
        &session,
        "navigate",
        json!({"url":"http://northwind.example/"}),
    );
    let (title, all) = page(&world, &session);
    assert_eq!(title, "Northwind Bank");
    assert_eq!(by_id(&all, "nav-home")["kind"], "link");
    assert_eq!(
        by_id(&all, "nav-pay")["url"],
        "http://northwind.example/transfers"
    );
    let tile = by_id(&all, "a-chk-4417");
    assert_eq!(tile["kind"], "link");
    assert_eq!(tile["url"], "http://northwind.example/accounts/chk-4417");
    assert!(
        tile["text"].as_str().unwrap().contains("Everyday Checking"),
        "{tile:?}"
    );
    assert!(shows(&all, "$8,124.55"));
    // The eight newest ledger rows are links; the seed's newest is tx-14.
    assert_eq!(
        by_id(&all, "t-tx-14")["url"],
        "http://northwind.example/accounts/chk-4417/transactions/tx-14"
    );

    // The tile is one link to the account; the ledger filter is a GET form.
    browser_do(&mut world, &session, "click", json!({"id":"a-chk-4417"}));
    assert_eq!(
        url(&world, &session),
        "http://northwind.example/accounts/chk-4417"
    );
    let (title, all) = page(&world, &session);
    assert!(title.starts_with("Everyday Checking"), "{title}");
    assert_eq!(by_id(&all, "filter")["kind"], "form");
    assert_eq!(by_id(&all, "filter-q")["kind"], "input");
    assert_eq!(by_id(&all, "filter-q")["label"], "Search merchant or memo");
    assert_eq!(by_id(&all, "filter-go")["kind"], "button");
    browser_do(
        &mut world,
        &session,
        "fill",
        json!({"id":"filter-q","value":"payroll"}),
    );
    browser_do(&mut world, &session, "click", json!({"id":"filter-go"}));
    assert_eq!(
        url(&world, &session),
        "http://northwind.example/accounts/chk-4417?category=&q=payroll"
    );
    let (_, all) = page(&world, &session);
    assert!(
        shows(&all, "1 transaction"),
        "the count reads as prose, not as a placeholder"
    );
    assert_eq!(by_id(&all, "filter-q")["value"], "payroll");

    // A ledger row is one link to the transaction; the card charge links out to the order.
    browser_do(
        &mut world,
        &session,
        "navigate",
        json!({"url":"http://northwind.example/accounts/cc-3310"}),
    );
    browser_do(&mut world, &session, "click", json!({"id":"t-tx-1"}));
    assert_eq!(
        url(&world, &session),
        "http://northwind.example/accounts/cc-3310/transactions/tx-1"
    );
    let (_, all) = page(&world, &session);
    assert!(shows(&all, "-$429.99"));
    assert_eq!(
        by_id(&all, "origin")["url"],
        "http://amazon.com/orders/1001"
    );
    browser_do(&mut world, &session, "click", json!({"id":"back"}));
    assert_eq!(
        url(&world, &session),
        "http://northwind.example/accounts/cc-3310"
    );

    // Pay & transfer: fill the three fields, press the button, read the new balance.
    browser_do(&mut world, &session, "click", json!({"id":"nav-pay"}));
    assert_eq!(url(&world, &session), "http://northwind.example/transfers");
    let (_, all) = page(&world, &session);
    for (id, label) in [
        ("xfer-from", "From account id"),
        ("xfer-to", "To account id"),
        ("xfer-amount", "Amount in cents"),
    ] {
        assert_eq!(by_id(&all, id)["kind"], "input");
        assert_eq!(by_id(&all, id)["label"], label);
    }
    assert_eq!(by_id(&all, "xfer")["kind"], "form");
    assert_eq!(by_id(&all, "xfer-go")["text"], "Transfer");
    browser_do(
        &mut world,
        &session,
        "fill",
        json!({"id":"xfer-from","value":"chk-4417"}),
    );
    browser_do(
        &mut world,
        &session,
        "fill",
        json!({"id":"xfer-to","value":"sav-9902"}),
    );
    browser_do(
        &mut world,
        &session,
        "fill",
        json!({"id":"xfer-amount","value":"12455"}),
    );
    browser_do(&mut world, &session, "click", json!({"id":"xfer-go"}));
    let (title, all) = page(&world, &session);
    assert!(title.starts_with("Everyday Checking"), "{title}");
    assert!(
        shows(&all, "$8,000.00"),
        "the debited balance is on the page that comes back"
    );
    browser_do(
        &mut world,
        &session,
        "navigate",
        json!({"url":"http://northwind.example/"}),
    );
    let (_, all) = page(&world, &session);
    assert!(
        shows(&all, "$14,624.55"),
        "the savings tile shows the credited leg"
    );

    // A bill is paid from the second form, and a new payee is added from the third.
    browser_do(
        &mut world,
        &session,
        "navigate",
        json!({"url":"http://northwind.example/transfers"}),
    );
    browser_do(
        &mut world,
        &session,
        "fill",
        json!({"id":"pay-account","value":"chk-4417"}),
    );
    browser_do(
        &mut world,
        &session,
        "fill",
        json!({"id":"pay-payee","value":"cascade-power"}),
    );
    browser_do(
        &mut world,
        &session,
        "fill",
        json!({"id":"pay-amount","value":"10000"}),
    );
    browser_do(&mut world, &session, "click", json!({"id":"pay-go"}));
    let (_, all) = page(&world, &session);
    assert!(shows(&all, "$7,900.00"));
    assert!(shows(&all, "Cascade Power"));
    browser_do(
        &mut world,
        &session,
        "navigate",
        json!({"url":"http://northwind.example/transfers"}),
    );
    browser_do(
        &mut world,
        &session,
        "fill",
        json!({"id":"addpayee-name","value":"Rainier Fibre"}),
    );
    browser_do(
        &mut world,
        &session,
        "fill",
        json!({"id":"addpayee-hint","value":"...7710"}),
    );
    browser_do(&mut world, &session, "click", json!({"id":"addpayee-go"}));
    let (_, all) = page(&world, &session);
    assert!(shows(&all, "rainier-fibre"));
}

#[test]
fn paypal_shows_the_balance_and_activity_and_sends_a_payment_through_the_agent_api() {
    let (mut world, session) = world();
    browser_do(
        &mut world,
        &session,
        "navigate",
        json!({"url":"http://paypal.com/"}),
    );
    let (title, all) = page(&world, &session);
    assert_eq!(title, "PayPal");
    assert_eq!(by_id(&all, "nav-home")["text"], "Home");
    assert_eq!(by_id(&all, "nav-pay")["text"], "Send and Request");
    assert_eq!(by_id(&all, "nav-pay")["url"], "http://paypal.com/transfers");
    assert_eq!(
        by_id(&all, "a-pp-alice")["url"],
        "http://paypal.com/accounts/pp-alice"
    );
    assert!(shows(&all, "$483.12"));
    assert!(
        all.iter()
            .any(|e| e["kind"] == "link"
                && e["id"].as_str().is_some_and(|id| id.starts_with("t-tx-")))
    );
    assert!(
        all.iter().all(|e| e["id"] != "a-pp-bob"),
        "only alice's wallet is listed"
    );

    // Send: the round button lands on the form it names, which pays a contact.
    browser_do(&mut world, &session, "click", json!({"id":"quick-send"}));
    assert_eq!(
        url(&world, &session),
        "http://paypal.com/transfers#pay-card"
    );
    let (_, all) = page(&world, &session);
    assert_eq!(by_id(&all, "pay")["kind"], "form");
    assert_eq!(by_id(&all, "pay-payee")["label"], "Payee id");
    assert!(shows(&all, "bob-tanaka"));
    browser_do(
        &mut world,
        &session,
        "fill",
        json!({"id":"pay-account","value":"pp-alice"}),
    );
    browser_do(
        &mut world,
        &session,
        "fill",
        json!({"id":"pay-payee","value":"bob-tanaka"}),
    );
    browser_do(
        &mut world,
        &session,
        "fill",
        json!({"id":"pay-amount","value":"8312"}),
    );
    browser_do(&mut world, &session, "click", json!({"id":"pay-go"}));
    let (title, all) = page(&world, &session);
    assert!(title.starts_with("PayPal Balance"), "{title}");
    assert!(
        shows(&all, "$400.00"),
        "the wallet balance after the payment"
    );
    assert!(shows(&all, "Bob Tanaka"));

    // The activity tab lists the payment; its row opens the receipt.
    browser_do(&mut world, &session, "click", json!({"id":"nav-activity"}));
    assert_eq!(url(&world, &session), "http://paypal.com/accounts/pp-alice");
    let (_, all) = page(&world, &session);
    let row = all
        .iter()
        .find(|e| {
            e["kind"] == "link" && e["text"].as_str().is_some_and(|t| t.contains("Bob Tanaka"))
        })
        .expect("the payment is in the activity list");
    let id = row["id"].as_str().unwrap().to_owned();
    browser_do(&mut world, &session, "click", json!({"id": id}));
    let (_, all) = page(&world, &session);
    assert!(shows(&all, "-$83.12"));
    assert!(shows(&all, "Payment to Bob Tanaka @bobtanaka"));
}
