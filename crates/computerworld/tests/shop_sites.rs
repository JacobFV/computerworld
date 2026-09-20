//! The storefronts (amazon, ebay, etsy, airbnb, booking, uber, doordash, ticketmaster)
//! served as HTML by the shop service and driven end to end through the agent API: the
//! semantic observation lists the search box, the nav and the product cards by the ids
//! the service documents; a search is typed and submitted, a card is followed, a
//! quantity is filled, the item is added to the cart and the order is placed.
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
        .step(session, vec![ActionEnvelope::new(channel, op, MACHINE, payload)])
        .unwrap();
    assert!(result.outcomes[0].success, "{op}: {:?}", result.outcomes[0]);
    result.outcomes[0].value.clone()
}
fn page(world: &World, session: &str) -> Value {
    world.observe(session).unwrap().channels["semantic.v1"][MACHINE].clone()
}
fn url(world: &World, session: &str) -> String {
    world.observe(session).unwrap().channels["browser.v1"][MACHINE]["url"].as_str().unwrap().to_owned()
}
/// Every element of the semantic tree, flattened.
fn elements(page: &Value) -> Vec<Value> {
    fn walk(elements: &[Value], out: &mut Vec<Value>) {
        for e in elements {
            out.push(e.clone());
            if let Some(children) = e["children"].as_array() {
                walk(children, out);
            }
        }
    }
    let mut out = Vec::new();
    walk(page["elements"].as_array().unwrap(), &mut out);
    out
}
fn by_id<'a>(all: &'a [Value], id: &str) -> &'a Value {
    all.iter().find(|e| e["id"] == id).unwrap_or_else(|| panic!("no element {id} in {all:?}"))
}
/// Whether any element's text contains `needle`: plain text carries no id in the tree.
fn says(all: &[Value], needle: &str) -> bool {
    all.iter().any(|e| e["text"].as_str().is_some_and(|t| t.contains(needle)))
}
fn has(all: &[Value], id: &str) -> bool {
    all.iter().any(|e| e["id"] == id)
}
/// The text of `#id` and everything under it, as the observation reports it.
fn text_of(all: &[Value], id: &str) -> String {
    fn gather(e: &Value, out: &mut String) {
        if let Some(t) = e["text"].as_str() {
            out.push_str(t);
            out.push(' ');
        }
        for c in e["children"].as_array().into_iter().flatten() {
            gather(c, out);
        }
    }
    let mut out = String::new();
    gather(by_id(all, id), &mut out);
    out
}

/// (host, page title, query, the product the query must find and that alice buys).
const RETAIL: &[(&str, &str, &str, &str)] = &[
    ("amazon.com", "Amazon", "monitor", "b0monitor27"),
    ("ebay.com", "eBay", "monitor", "115402998811"),
    ("etsy.com", "Etsy", "mug", "et-mug-slab"),
    ("airbnb.com", "Airbnb", "skykomish", "cabin-skykomish-aframe"),
    ("booking.com", "Booking.com", "miradouro", "lis-miradouro-hotel"),
    ("uber.com", "Uber", "airport", "ride-uberx-annex-airport"),
    ("doordash.com", "DoorDash", "margherita", "bayfront-pizza-margherita"),
];

#[test]
fn every_retail_storefront_is_searched_and_an_order_is_placed_through_the_agent_api() {
    let (mut world, session) = world();
    for (host, title, query, product) in RETAIL {
        let home = format!("http://{host}/");
        act(&mut world, &session, "browser.v1", "navigate", json!({"url": home}));
        let seen = page(&world, &session);
        assert_eq!(seen["title"], *title, "{host}");
        let all = elements(&seen);
        // The chrome an agent steers by: the search form, the basket and the orders.
        assert_eq!(by_id(&all, "hdr-search")["kind"], "form", "{host}");
        let q = by_id(&all, "hdr-k");
        assert_eq!(q["kind"], "input");
        assert_eq!(q["label"], "Search the catalogue");
        assert_eq!(by_id(&all, "hdr-search-go")["kind"], "button");
        let basket = by_id(&all, "nav-basket");
        assert_eq!((basket["kind"].as_str(), basket["url"].as_str()), (Some("link"), Some(format!("http://{host}/cart").as_str())));
        assert_eq!(by_id(&all, "nav-orders")["url"], format!("http://{host}/orders"));
        let card = by_id(&all, &format!("p-{product}"));
        assert_eq!(card["kind"], "link", "{host}: the whole card is one link");
        assert_eq!(card["url"], format!("http://{host}/dp/{product}"));
        assert!(all.iter().any(|e| e["kind"] == "link" && e["id"].as_str().is_some_and(|id| id.starts_with("cat-"))), "{host}");

        // Type into the box and press Enter: a GET form, so the URL is the query.
        act(&mut world, &session, "browser.v1", "click", json!({"id": "hdr-k"}));
        act(&mut world, &session, "keyboard.v1", "type", json!({"text": query}));
        act(&mut world, &session, "browser.v1", "key", json!({"key": "Enter"}));
        assert_eq!(url(&world, &session), format!("http://{host}/s?k={query}"));
        let all = elements(&page(&world, &session));
        assert_eq!(by_id(&all, "hdr-k")["value"], *query);
        assert!(text_of(&all, "lead").contains(&format!("Results for \"{query}\"")), "{host}: {}", text_of(&all, "lead"));
        assert!(has(&all, &format!("p-{product}")), "{host}: {query} finds {product}");

        // Follow the card to the product, set a quantity, add it, and place the order.
        act(&mut world, &session, "browser.v1", "click", json!({"id": format!("p-{product}")}));
        assert_eq!(url(&world, &session), format!("http://{host}/dp/{product}"));
        let all = elements(&page(&world, &session));
        assert_eq!(by_id(&all, "title")["kind"], "heading", "{host}");
        assert_eq!(by_id(&all, "add-qty")["kind"], "input");
        assert_eq!(by_id(&all, "add-qty")["label"], "Quantity");
        assert_eq!(by_id(&all, "add-go")["kind"], "button");
        assert_eq!(by_id(&all, "fav-go")["kind"], "button");
        act(&mut world, &session, "browser.v1", "fill", json!({"id": "add-qty", "value": "2"}));
        act(&mut world, &session, "browser.v1", "click", json!({"id": "add-go"}));
        let all = elements(&page(&world, &session));
        assert_eq!(by_id(&all, &format!("qty-{product}-n"))["value"], "2", "{host}: the cart shows the line");
        assert_eq!(by_id(&all, &format!("rm-{product}-go"))["kind"], "button");
        // Alice arrives with seeded lines on some storefronts (ebay holds two, etsy one);
        // each Remove is a one-button form, so clearing them leaves the order exactly the
        // two units she just chose, whatever the seed happens to hold.
        let others: Vec<String> = all
            .iter()
            .filter_map(|e| e["id"].as_str())
            .filter(|id| id.starts_with("rm-") && id.ends_with("-go") && *id != format!("rm-{product}-go"))
            .map(str::to_owned)
            .collect();
        for id in others {
            act(&mut world, &session, "browser.v1", "click", json!({"id": id}));
        }
        let all = elements(&page(&world, &session));
        assert!(says(&all, "Subtotal (2 items)"), "{host}");
        act(&mut world, &session, "browser.v1", "click", json!({"id": "checkout-go"}));
        let all = elements(&page(&world, &session));
        let lead = text_of(&all, "lead");
        assert!(lead.starts_with("Order "), "{host}: {lead}");
        assert!(says(&all, "Order total: $"), "{host}");
        let order = lead.trim().trim_start_matches("Order ").to_owned();

        // The order is on the orders page, as a link to its receipt.
        act(&mut world, &session, "browser.v1", "navigate", json!({"url": format!("http://{host}/orders")}));
        let all = elements(&page(&world, &session));
        let row = by_id(&all, &format!("o-{order}"));
        assert_eq!(row["kind"], "link");
        assert_eq!(row["url"], format!("http://{host}/orders/{order}"));
        act(&mut world, &session, "browser.v1", "click", json!({"id": format!("o-{order}")}));
        assert_eq!(url(&world, &session), format!("http://{host}/orders/{order}"));
    }
}

#[test]
fn ticketmaster_sells_a_seat_through_the_agent_api() {
    let (mut world, session) = world();
    act(&mut world, &session, "browser.v1", "navigate", json!({"url": "http://ticketmaster.com/"}));
    let seen = page(&world, &session);
    assert_eq!(seen["title"], "Ticketmaster");
    let all = elements(&seen);
    assert_eq!(by_id(&all, "nav-basket")["url"], "http://ticketmaster.com/my-tickets");
    assert_eq!(by_id(&all, "cat-conference")["url"], "http://ticketmaster.com/s?c=conference");
    act(&mut world, &session, "browser.v1", "click", json!({"id": "cat-conference"}));
    assert_eq!(url(&world, &session), "http://ticketmaster.com/s?c=conference");
    act(&mut world, &session, "browser.v1", "click", json!({"id": "p-devcon-2026"}));
    assert_eq!(url(&world, &session), "http://ticketmaster.com/event/devcon-2026");
    let all = elements(&page(&world, &session));
    assert!(says(&all, "2026-04-14 · Cascade Convention Center"));
    assert_eq!(by_id(&all, "venue-map")["url"], "http://maps.google.com/maps/place/devcon-center");
    assert_eq!(by_id(&all, "buy-floor-qty")["label"], "Tickets");
    act(&mut world, &session, "browser.v1", "fill", json!({"id": "buy-floor-qty", "value": "2"}));
    act(&mut world, &session, "browser.v1", "click", json!({"id": "buy-floor-go"}));
    let all = elements(&page(&world, &session));
    assert!(text_of(&all, "lead").starts_with("Order TM-2211"), "{}", text_of(&all, "lead"));
    // Carol holds A-14-7 (storyline 4); the next two seats follow from it.
    assert!(says(&all, "Seat A-14-8") && says(&all, "Seat A-14-9"));
    act(&mut world, &session, "browser.v1", "navigate", json!({"url": "http://ticketmaster.com/my-tickets"}));
    let all = elements(&page(&world, &session));
    assert_eq!(by_id(&all, "o-TM-2211")["url"], "http://ticketmaster.com/orders/TM-2211");
}
