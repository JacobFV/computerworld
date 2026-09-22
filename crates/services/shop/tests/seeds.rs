//! The shipped storefront seeds, exercised through the crate that serves them. Storylines 4 and 5
//! are pinned here, so a seed edit that breaks the cross-site links fails in this crate.
use cw_protocol::HttpRequest;
use cw_sdk::{Service, ServiceContext};
use cw_service_shop::{ShopService, ShopState};
use serde_json::Value;
const AMAZON: &str = include_str!("../../../../worlds/company-2026/sites/amazon.json");
const ETSY: &str = include_str!("../../../../worlds/company-2026/sites/etsy.json");
const TICKETMASTER: &str = include_str!("../../../../worlds/company-2026/sites/ticketmaster.json");
fn ctx(actor: &str) -> ServiceContext {
    ServiceContext {
        actor: actor.into(),
        source: "alice-mac".into(),
        tick: 20,
        seed: 1,
        instance: "shop".into(),
    }
}
fn load(raw: &str) -> Value {
    let doc: Value = serde_json::from_str(raw).expect("site file must be JSON");
    assert_eq!(doc["kind"], "shop");
    ShopService
        .initialize(doc["initial_state"].clone(), &ctx("alice"))
        .expect("seed must load")
}
/// A page is checked the moment it is fetched: HTML the engine renders strictly, unique ids.
fn checked(status: u16, body: &str) {
    if status == 200 && body.starts_with("<!DOCTYPE html>") {
        cw_service_common::html::validate_strict(body).unwrap_or_else(|e| panic!("{e}"));
    }
}
fn get(state: &mut Value, actor: &str, url: &str) -> (u16, String) {
    let r = ShopService
        .handle(state, &ctx(actor), &HttpRequest::get(url))
        .unwrap();
    let body = String::from_utf8(r.body).unwrap();
    checked(r.status, &body);
    (r.status, body)
}
/// The collapsed text of `#id` in a page.
fn text(body: &str, id: &str) -> String {
    let doc = cw_web::html::parse(body);
    let node = *doc.by_id(id).first().unwrap_or_else(|| panic!("no #{id}"));
    cw_web::paint::semantics::collapse(&doc.text_content(node))
}
fn attr(body: &str, id: &str, name: &str) -> String {
    let doc = cw_web::html::parse(body);
    let node = *doc.by_id(id).first().unwrap_or_else(|| panic!("no #{id}"));
    doc.attr(node, name).unwrap_or_default().to_owned()
}
fn post(state: &mut Value, actor: &str, url: &str, body: &[(&str, &str)]) -> (u16, String) {
    let mut r = HttpRequest::get(url);
    r.method = "POST".into();
    r.headers.insert(
        "content-type".into(),
        "application/x-www-form-urlencoded".into(),
    );
    r.body = url::form_urlencoded::Serializer::new(String::new())
        .extend_pairs(body)
        .finish()
        .into_bytes();
    let r = ShopService.handle(state, &ctx(actor), &r).unwrap();
    let body = String::from_utf8(r.body).unwrap();
    checked(r.status, &body);
    (r.status, body)
}
#[test]
fn storyline_5_order_1001_is_the_order_the_bank_points_at() {
    let mut state = load(AMAZON);
    let s: ShopState = serde_json::from_value(state.clone()).unwrap();
    let order = &s.orders["1001"];
    assert_eq!(order.buyer, "alice");
    assert_eq!(order.total_cents, 42_999);
    assert_eq!(order.items[0].product, "b0monitor27");
    assert_eq!(
        s.next_order, 1002,
        "the next order must not collide with 1001"
    );
    let (status, body) = get(&mut state, "alice", "http://amazon.com/orders/1001");
    assert_eq!(status, 200);
    assert_eq!(text(&body, "total"), "Order total: $429.99");
    assert_eq!(text(&body, "it-0-p"), "$429.99");
    assert!(text(&body, "meta").contains(&order.confirmation), "{body}");
    assert_eq!(attr(&body, "again-0", "href"), "/dp/b0monitor27");
    // The order is Alice's alone; anyone else gets a refusal, not someone else's receipt.
    assert_ne!(
        get(&mut state, "bob", "http://amazon.com/orders/1001").0,
        200
    );
    // Storyline 5's four-star review is in the catalogue and counted in the rating.
    let monitor = &s.products["b0monitor27"];
    assert!(monitor
        .reviews
        .iter()
        .any(|r| r.author == "alice" && r.stars == 4));
    assert_eq!(monitor.rating_count as usize, 41);
    assert_eq!(monitor.stars_tenths(), 44);
}
#[test]
fn every_retail_page_the_seeds_advertise_resolves_and_the_catalogue_is_searchable() {
    let mut state = load(AMAZON);
    for url in [
        "http://amazon.com/",
        "http://amazon.com/cart",
        "http://amazon.com/orders",
        "http://amazon.com/favorites",
        "http://amazon.com/dp/b0monitor27",
        "http://amazon.com/dp/b0dock11",
        "http://amazon.com/dp/b0kbd87",
        "http://amazon.com/dp/b0book-det",
        "http://amazon.com/dp/b0chair",
        "http://amazon.com/dp/b0cable",
    ] {
        assert_eq!(get(&mut state, "alice", url).0, 200, "{url}");
    }
    // Storyline 5: the query Alice typed really finds the product she bought.
    let (_, body) = get(
        &mut state,
        "alice",
        "http://amazon.com/s?k=27%20inch%204k%20usb-c%20monitor",
    );
    assert_eq!(
        text(&body, "n-b0monitor27"),
        "Lumen 27-inch 4K USB-C Monitor"
    );
    assert_eq!(attr(&body, "p-b0monitor27", "href"), "/dp/b0monitor27");
    assert_eq!(
        text(&body, "pr-b0monitor27"),
        "$429.99",
        "superscript cents still read as one price"
    );
    let mut state = load(ETSY);
    for url in [
        "http://etsy.com/",
        "http://etsy.com/dp/et-print-map",
        "http://etsy.com/dp/et-mug-slab",
        "http://etsy.com/dp/et-notebook",
        "http://etsy.com/dp/et-tote-block",
        "http://etsy.com/dp/et-ring-band",
        "http://etsy.com/orders/5001",
    ] {
        assert_eq!(get(&mut state, "alice", url).0, 200, "{url}");
    }
}
#[test]
fn a_seeded_cart_really_checks_out_and_decrements_the_catalogue() {
    let mut state = load(AMAZON);
    let before: ShopState = serde_json::from_value(state.clone()).unwrap();
    assert_eq!(before.carts["carol"]["b0lamp"], 1);
    let (status, _) = post(&mut state, "carol", "http://amazon.com/api/checkout", &[]);
    assert_eq!(status, 200);
    let after: ShopState = serde_json::from_value(state.clone()).unwrap();
    let order = &after.orders["1002"];
    assert_eq!(order.total_cents, 5_899 + 2 * 1_899);
    assert_eq!(
        after.products["b0lamp"].stock,
        before.products["b0lamp"].stock - 1
    );
    assert!(
        !after.carts.contains_key("carol"),
        "checkout empties the cart"
    );
    // Sold-out lines are refused rather than quietly oversold.
    let (status, _) = post(
        &mut state,
        "bob",
        "http://amazon.com/api/cart",
        &[("product", "b0kbd87"), ("qty", "1")],
    );
    assert_eq!(status, 400);
}
#[test]
fn storyline_4_carol_holds_seat_a_14_7_and_the_next_seat_follows_from_it() {
    let mut state = load(TICKETMASTER);
    let s: ShopState = serde_json::from_value(state.clone()).unwrap();
    assert_eq!(s.orders["TM-2210"].buyer, "carol");
    assert_eq!(s.orders["TM-2210"].seats, vec!["A-14-7".to_string()]);
    assert_eq!(s.orders["TM-2210"].venue, "devcon-center");
    assert_eq!(s.orders["TM-2209"].seats, vec!["A-14-6".to_string()]);
    assert_eq!(s.next_order, 2211);
    let (status, body) = get(
        &mut state,
        "carol",
        "http://ticketmaster.com/event/devcon-2026",
    );
    assert_eq!(status, 200);
    assert!(
        text(&body, "when").contains("Cascade Convention Center"),
        "{body}"
    );
    assert_eq!(
        attr(&body, "venue-map", "href"),
        "http://maps.google.com/maps/place/devcon-center"
    );
    let (status, body) = get(&mut state, "carol", "http://ticketmaster.com/my-tickets");
    assert_eq!(status, 200);
    assert_eq!(text(&body, "o-TM-2210-id"), "Order TM-2210");
    assert_eq!(attr(&body, "o-TM-2210", "href"), "/orders/TM-2210");
    let (status, body) = get(
        &mut state,
        "carol",
        "http://ticketmaster.com/orders/TM-2210",
    );
    assert_eq!(status, 200);
    assert_eq!(text(&body, "seat-0"), "Seat A-14-7");
    // Seats are a pure function of what is left, so the next buyer gets exactly the next seat.
    let (status, _) = post(
        &mut state,
        "bob",
        "http://ticketmaster.com/api/checkout",
        &[("event", "devcon-2026"), ("tier", "floor"), ("qty", "1")],
    );
    assert_eq!(status, 200);
    let s: ShopState = serde_json::from_value(state).unwrap();
    assert_eq!(s.orders["TM-2211"].seats, vec!["A-14-8".to_string()]);
}
#[test]
fn every_ticket_page_the_seed_advertises_resolves() {
    let mut state = load(TICKETMASTER);
    for url in [
        "http://ticketmaster.com/",
        "http://ticketmaster.com/my-tickets",
        "http://ticketmaster.com/event/devcon-2026",
        "http://ticketmaster.com/event/meridian-tour",
        "http://ticketmaster.com/event/psfc-rivergate",
        "http://ticketmaster.com/event/rust-meetup",
    ] {
        assert_eq!(get(&mut state, "carol", url).0, 200, "{url}");
    }
    assert_eq!(
        get(
            &mut state,
            "carol",
            "http://ticketmaster.com/orders/TM-2210"
        )
        .0,
        200
    );
}

/// Every shipped storefront, every page kind, through the strict validator (in `get`),
/// with the ids an agent drives the site by.
#[test]
fn every_shipped_storefront_serves_strict_html_with_the_agent_ids() {
    for site in [
        "amazon",
        "ebay",
        "etsy",
        "airbnb",
        "booking",
        "uber",
        "doordash",
        "ticketmaster",
    ] {
        let path = format!(
            "{}/../../../worlds/company-2026/sites/{site}.json",
            env!("CARGO_MANIFEST_DIR")
        );
        let mut state = load(&std::fs::read_to_string(path).unwrap());
        let s: ShopState = serde_json::from_value(state.clone()).unwrap();
        let host = format!("http://{site}.com");
        let (status, home) = get(&mut state, "alice", &format!("{host}/"));
        assert_eq!(status, 200, "{site}");
        assert!(
            home.contains(&format!("class=\"skin-{site} ")),
            "{site} wears its own skin"
        );
        for id in [
            "chrome",
            "wordmark",
            "hdr-search",
            "hdr-k",
            "hdr-search-go",
            "nav-basket",
            "cats",
            "lead",
            "deals",
            "foot",
        ] {
            assert!(
                !cw_web::html::parse(&home).by_id(id).is_empty(),
                "{site} home lacks #{id}"
            );
        }
        // A box office keeps its orders behind `nav-basket` ("My tickets"); a store has both.
        assert_eq!(
            cw_web::html::parse(&home).by_id("nav-orders").is_empty(),
            s.tickets(),
            "{site}: nav-orders belongs to a store, not a box office"
        );
        assert_eq!(attr(&home, "hdr-search", "action"), "/s");
        for c in &s.categories {
            assert_eq!(
                attr(&home, &format!("cat-{}", c.id), "href"),
                format!("/s?c={}", c.id)
            );
            assert_eq!(
                get(&mut state, "alice", &format!("{host}/s?c={}", c.id)).0,
                200
            );
        }
        for (id, product) in &s.products {
            let page = if s.tickets() { "event" } else { "dp" };
            assert_eq!(
                attr(&home, &format!("p-{id}"), "href"),
                format!("/{page}/{id}"),
                "{site}"
            );
            let (status, body) = get(&mut state, "alice", &format!("{host}/{page}/{id}"));
            assert_eq!(status, 200, "{site} {id}");
            assert_eq!(text(&body, "title"), product.title);
        }
        for actor in ["alice", "bob", "carol"] {
            for page in ["cart", "orders", "favorites", "s?k=a"] {
                assert_eq!(
                    get(&mut state, actor, &format!("{host}/{page}")).0,
                    200,
                    "{site} {page}"
                );
            }
        }
        for (id, order) in &s.orders {
            let (status, body) = get(&mut state, &order.buyer, &format!("{host}/orders/{id}"));
            assert_eq!(status, 200, "{site} order {id}");
            assert_eq!(text(&body, "lead"), format!("Order {id}"));
        }
    }
}

/// A seeded cart line the catalogue cannot fill is a dead end: `set_cart` refuses
/// `qty > stock`, so no agent could ever have produced such a line, and `/api/checkout`
/// is all-or-nothing, so one bad line refuses the whole basket. Every shipped cart must
/// therefore be one its own catalogue can still fill.
#[test]
fn every_seeded_cart_is_one_the_catalogue_can_still_fill() {
    for site in [
        "amazon",
        "ebay",
        "etsy",
        "airbnb",
        "booking",
        "uber",
        "doordash",
        "ticketmaster",
    ] {
        let path = format!(
            "{}/../../../worlds/company-2026/sites/{site}.json",
            env!("CARGO_MANIFEST_DIR")
        );
        let raw = std::fs::read_to_string(path).unwrap();
        let s: ShopState = serde_json::from_value(load(&raw)).unwrap();
        for (actor, cart) in &s.carts {
            for (id, qty) in cart {
                let stock = s.products[id].stock;
                assert!(
                    *qty <= stock,
                    "{site}: {actor}'s cart holds {qty} of {id}, of which only {stock} remain"
                );
            }
            let mut state = load(&raw);
            let (status, _) = post(
                &mut state,
                actor,
                &format!("http://{site}.com/api/checkout"),
                &[],
            );
            assert_eq!(
                status, 200,
                "{site}: {actor}'s seeded cart must be orderable as it stands"
            );
        }
    }
}
