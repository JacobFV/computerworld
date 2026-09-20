//! Commerce: amazon.com, ebay.com, etsy.com, airbnb.com, booking.com, uber.com and doordash.com
//! (`mode: retail`), ticketmaster.com (`mode: tickets`). Pages are HTML (`view.rs`); the look is a skin.
//! Checkout is the one irreversible mutation in the world, so stock and orders are authoritative.
//! Every amount is integer cents — a float total would not survive a replay bit for bit.
use cw_protocol::{HttpRequest, HttpResponse, PageTheme, Result};
use cw_sdk::{Registry, Service, ServiceContext};
use cw_service_common as web;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;
mod view;
use view::View;
pub struct ShopService;
pub fn register(registry: &mut Registry) -> Result<()> {
    registry.register(ShopService)
}
/// Documented seed keys, checked by container type only — the crate that fills them owns the rest.
const OBJECTS: &[&str] = &["theme", "products", "carts", "orders", "favorites"];
const ARRAYS: &[&str] = &["categories", "messages"];
/// `mode` is the documented discriminant; an unlisted value is a seed typo, not a fallback.
const MODES: &[&str] = &["retail", "tickets"];
/// Orders start here when a seed does not say otherwise; storyline 5 pins amazon's first at 1001.
const FIRST_ORDER: u64 = 1001;
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct ShopState {
    pub mode: String,
    /// Which product the pages look like (`amazon`, `ebay`, ...); absent, the brand decides.
    pub skin: String,
    pub brand: String,
    pub tagline: String,
    pub currency: String,
    pub theme: PageTheme,
    pub categories: Vec<Category>,
    pub products: BTreeMap<String, Product>,
    /// actor -> product -> quantity. A cart is per person, like every other account-scoped thing.
    pub carts: BTreeMap<String, BTreeMap<String, u64>>,
    pub orders: BTreeMap<String, Order>,
    pub favorites: BTreeMap<String, Vec<String>>,
    pub messages: Vec<Message>,
    pub next_order: u64,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Category {
    pub id: String,
    pub title: String,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Product {
    pub id: String,
    pub category: String,
    pub title: String,
    pub seller: String,
    pub price_cents: u64,
    pub rating_sum: u64,
    pub rating_count: u64,
    pub stock: u64,
    pub fast_shipping: bool,
    pub bullets: Vec<String>,
    pub description: String,
    pub reviews: Vec<Review>,
    /// Tickets only: the `geo` place id, its display name and the event's tick and tiers.
    pub venue: String,
    pub venue_name: String,
    pub event_tick: u64,
    pub event_date: String,
    pub tiers: Vec<Tier>,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Tier {
    pub id: String,
    pub title: String,
    pub price_cents: u64,
    pub section: String,
    pub row_size: u64,
    pub capacity: u64,
    pub remaining: u64,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Review {
    pub id: String,
    pub author: String,
    pub stars: u64,
    pub title: String,
    pub body: String,
    pub tick: u64,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Order {
    pub id: String,
    pub buyer: String,
    pub tick: u64,
    pub date: String,
    pub items: Vec<OrderItem>,
    pub total_cents: u64,
    pub status: String,
    pub confirmation: String,
    pub seats: Vec<String>,
    pub venue: String,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct OrderItem {
    pub product: String,
    pub title: String,
    pub qty: u64,
    pub price_cents: u64,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Message {
    pub id: String,
    pub from: String,
    pub to: String,
    pub product: String,
    pub text: String,
    pub tick: u64,
}
/// Confirmation codes must be reproducible, so they are a folded FNV-1a of buyer and order id
/// rather than anything drawn from a random source.
fn confirmation(prefix: &str, buyer: &str, id: &str) -> String {
    const ALPHABET: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789";
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in buyer.bytes().chain(*b"#").chain(id.bytes()) {
        h = (h ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01b3);
    }
    let code: String = (0..6)
        .map(|i| ALPHABET[((h >> (i * 5)) & 31) as usize] as char)
        .collect();
    format!("{prefix}-{code}")
}
fn money(cents: u64) -> String {
    format!("${}.{:02}", cents / 100, cents % 100)
}
fn tokens(q: &str) -> Vec<String> {
    q.to_ascii_lowercase()
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|t| !t.is_empty())
        .map(str::to_owned)
        .collect()
}
impl Product {
    pub fn stars_tenths(&self) -> u64 {
        (self.rating_sum * 10)
            .checked_div(self.rating_count)
            .unwrap_or(0)
    }
    fn rating_line(&self) -> String {
        if self.rating_count == 0 {
            "No reviews yet".into()
        } else {
            let t = self.stars_tenths();
            format!(
                "{}.{} out of 5 · {} reviews",
                t / 10,
                t % 10,
                self.rating_count
            )
        }
    }
    fn cheapest_cents(&self) -> u64 {
        self.tiers
            .iter()
            .map(|t| t.price_cents)
            .min()
            .unwrap_or(self.price_cents)
    }
    fn available(&self) -> bool {
        if self.tiers.is_empty() {
            self.stock > 0
        } else {
            self.tiers.iter().any(|t| t.remaining > 0)
        }
    }
    fn score(&self, terms: &[String]) -> u64 {
        let title = self.title.to_ascii_lowercase();
        let body = format!(
            "{} {} {} {} {}",
            self.description,
            self.category,
            self.seller,
            self.venue_name,
            self.bullets.join(" ")
        )
        .to_ascii_lowercase();
        terms
            .iter()
            .map(|t| 8 * u64::from(title.contains(t)) + u64::from(body.contains(t)))
            .sum()
    }
}
impl ShopState {
    pub fn tickets(&self) -> bool {
        self.mode == "tickets"
    }
    fn product(&self, id: &str) -> std::result::Result<&Product, String> {
        self.products
            .get(id)
            .ok_or_else(|| "unknown product".into())
    }
    fn cart(&self, actor: &str) -> BTreeMap<String, u64> {
        self.carts.get(actor).cloned().unwrap_or_default()
    }
    fn cart_total(&self, actor: &str) -> u64 {
        self.cart(actor)
            .iter()
            .filter_map(|(id, qty)| self.products.get(id).map(|p| p.price_cents * qty))
            .sum()
    }
    /// `qty == 0` removes the line, which is how every real cart spells "delete".
    pub fn set_cart(
        &mut self,
        actor: &str,
        product: &str,
        qty: u64,
    ) -> std::result::Result<u64, String> {
        let stock = self.product(product)?.stock;
        if qty > stock {
            return Err(format!("only {stock} in stock"));
        }
        let cart = self.carts.entry(actor.to_owned()).or_default();
        if qty == 0 {
            cart.remove(product);
        } else {
            cart.insert(product.to_owned(), qty);
        }
        if cart.is_empty() {
            self.carts.remove(actor);
        }
        Ok(qty)
    }
    fn take_order_id(&mut self, prefix: &str) -> String {
        if self.next_order == 0 {
            self.next_order = FIRST_ORDER;
        }
        let mut id = format!("{prefix}{}", self.next_order);
        while self.orders.contains_key(&id) {
            self.next_order += 1;
            id = format!("{prefix}{}", self.next_order);
        }
        self.next_order += 1;
        id
    }
    /// The irreversible one: stock is checked for the whole cart first, so an order is
    /// all-or-nothing and the catalogue can never go negative.
    pub fn checkout(&mut self, actor: &str, tick: u64) -> std::result::Result<Order, String> {
        let cart = self.cart(actor);
        if cart.is_empty() {
            return Err("cart is empty".into());
        }
        for (id, qty) in &cart {
            let p = self.product(id)?;
            if p.stock < *qty {
                return Err(format!("{} is out of stock", p.title));
            }
        }
        let mut items = vec![];
        let mut total = 0u64;
        for (id, qty) in &cart {
            let p = self.products.get_mut(id).ok_or("unknown product")?;
            p.stock -= qty;
            total += p.price_cents * qty;
            items.push(OrderItem {
                product: p.id.clone(),
                title: p.title.clone(),
                qty: *qty,
                price_cents: p.price_cents,
            });
        }
        let id = self.take_order_id("");
        let order = Order {
            confirmation: confirmation("ORD", actor, &id),
            id: id.clone(),
            buyer: actor.to_owned(),
            tick,
            date: String::new(),
            items,
            total_cents: total,
            status: "Placed".into(),
            seats: vec![],
            venue: String::new(),
        };
        self.orders.insert(id, order.clone());
        self.carts.remove(actor);
        Ok(order)
    }
    /// Seats are a pure function of how many are already sold, so the allocation replays exactly.
    pub fn buy_tickets(
        &mut self,
        actor: &str,
        event: &str,
        tier: &str,
        qty: u64,
        tick: u64,
    ) -> std::result::Result<Order, String> {
        if !(1..=8).contains(&qty) {
            return Err("quantity must be 1 through 8".into());
        }
        let product = self.products.get_mut(event).ok_or("unknown event")?;
        let (title, venue) = (product.title.clone(), product.venue.clone());
        let t = product
            .tiers
            .iter_mut()
            .find(|t| t.id == tier)
            .ok_or("unknown tier")?;
        if t.remaining < qty {
            return Err(format!("{} is sold out", t.title));
        }
        let row_size = t.row_size.max(1);
        let mut seats = vec![];
        for _ in 0..qty {
            let index = t.capacity.saturating_sub(t.remaining);
            seats.push(format!(
                "{}-{}-{}",
                t.section,
                index / row_size + 1,
                index % row_size + 1
            ));
            t.remaining -= 1;
        }
        let total = t.price_cents * qty;
        let item = OrderItem {
            product: event.to_owned(),
            title: format!("{title} — {}", t.title),
            qty,
            price_cents: t.price_cents,
        };
        let id = self.take_order_id("TM-");
        let order = Order {
            confirmation: confirmation("TKT", actor, &id),
            id: id.clone(),
            buyer: actor.to_owned(),
            tick,
            date: String::new(),
            items: vec![item],
            total_cents: total,
            status: "Issued".into(),
            seats,
            venue,
        };
        self.orders.insert(id, order.clone());
        Ok(order)
    }
    pub fn review(
        &mut self,
        actor: &str,
        product: &str,
        stars: u64,
        title: &str,
        body: &str,
        tick: u64,
    ) -> std::result::Result<Review, String> {
        if !(1..=5).contains(&stars) {
            return Err("stars must be 1 through 5".into());
        }
        if title.trim().is_empty() {
            return Err("review title required".into());
        }
        let p = self.products.get_mut(product).ok_or("unknown product")?;
        let review = Review {
            id: format!("rv{}", p.reviews.len() + 1),
            author: actor.to_owned(),
            stars,
            title: title.trim().to_owned(),
            body: body.trim().to_owned(),
            tick,
        };
        p.rating_sum += stars;
        p.rating_count += 1;
        p.reviews.push(review.clone());
        Ok(review)
    }
    /// Toggles, and reports the side it landed on so a caller can render the right label.
    pub fn favorite(&mut self, actor: &str, product: &str) -> std::result::Result<bool, String> {
        self.product(product)?;
        let list = self.favorites.entry(actor.to_owned()).or_default();
        match list.iter().position(|p| p == product) {
            Some(at) => {
                list.remove(at);
                if list.is_empty() {
                    self.favorites.remove(actor);
                }
                Ok(false)
            }
            None => {
                list.push(product.to_owned());
                Ok(true)
            }
        }
    }
    pub fn message_seller(
        &mut self,
        actor: &str,
        product: &str,
        text: &str,
        tick: u64,
    ) -> std::result::Result<Message, String> {
        if text.trim().is_empty() {
            return Err("message required".into());
        }
        let seller = self.product(product)?.seller.clone();
        if seller.is_empty() {
            return Err("this listing has no seller to message".into());
        }
        let m = Message {
            id: format!("msg-{}", self.messages.len() + 1),
            from: actor.to_owned(),
            to: seller,
            product: product.to_owned(),
            text: text.trim().to_owned(),
            tick,
        };
        self.messages.push(m.clone());
        Ok(m)
    }
    pub fn search(&self, query: &str, category: &str) -> Vec<&Product> {
        let terms = tokens(query);
        let mut hits: Vec<(u64, &Product)> = self
            .products
            .values()
            .filter(|p| category.is_empty() || p.category == category)
            .map(|p| (if terms.is_empty() { 1 } else { p.score(&terms) }, p))
            .filter(|(s, _)| *s > 0)
            .collect();
        hits.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.id.cmp(&b.1.id)));
        hits.into_iter().map(|(_, p)| p).collect()
    }
    fn orders_of(&self, actor: &str) -> Vec<&Order> {
        let mut v: Vec<&Order> = self.orders.values().filter(|o| o.buyer == actor).collect();
        v.sort_by(|a, b| b.tick.cmp(&a.tick).then_with(|| a.id.cmp(&b.id)));
        v
    }
}
impl Service for ShopService {
    fn kind(&self) -> &str {
        "shop"
    }
    fn initialize(&self, initial: Value, _: &ServiceContext) -> Result<Value> {
        let gated = web::shape(initial, OBJECTS, ARRAYS)?;
        let mode = web::variant(&gated, "mode", MODES)?;
        if gated.get("skin").is_some() {
            web::variant(&gated, "skin", view::SKINS)?;
        }
        web::theme(&gated)?;
        let mut s: ShopState = web::load(&gated)?;
        s.mode = mode;
        if s.brand.is_empty() {
            s.brand = "Shop".into();
        }
        if s.currency.is_empty() {
            s.currency = "USD".into();
        }
        if s.next_order == 0 {
            s.next_order = FIRST_ORDER;
        }
        Ok(serde_json::to_value(s)?)
    }
    fn handle(
        &self,
        state: &mut Value,
        c: &ServiceContext,
        r: &HttpRequest,
    ) -> Result<HttpResponse> {
        let mut s: ShopState = web::load(state)?;
        let path = web::path(r);
        let parts: Vec<&str> = path.trim_matches('/').split('/').collect();
        let method = r.method.to_ascii_uppercase();
        if method == "GET" {
            let view = View::new(&s, &c.actor);
            return match parts.as_slice() {
                [""] => view.home(),
                ["s"] => view.results(
                    &web::query(r, "k").unwrap_or_default(),
                    &web::query(r, "c").unwrap_or_default(),
                ),
                ["dp", id] => view.detail(id),
                ["event", id] => view.event(id),
                ["cart"] => view.cart(),
                ["orders"] | ["my-tickets"] => view.orders(),
                ["orders", id] => view.order(id),
                ["favorites"] => view.favorites(),
                ["api", "products"] => HttpResponse::json(200, &s.products),
                ["api", "products", id] => web::domain(s.product(id).map(|x| json!(x))),
                ["api", "orders"] => HttpResponse::json(200, &s.orders_of(&c.actor)),
                ["api", "cart"] => HttpResponse::json(200, &s.cart(&c.actor)),
                _ => web::error(404, "route not found"),
            };
        }
        if method != "POST" {
            return web::error(405, "method not allowed");
        }
        let b = web::body(r)?;
        let text = |k: &str| web::text(&b, k);
        let outcome: std::result::Result<(Value, String), String> = match parts.as_slice() {
            ["api", "cart"] => s
                .set_cart(
                    &c.actor,
                    &text("product"),
                    web::number(&b, "qty").unwrap_or(1),
                )
                .map(|qty| {
                    (
                        json!({"product": text("product"), "qty": qty}),
                        "/cart".into(),
                    )
                }),
            ["api", "checkout"] if s.tickets() => s
                .buy_tickets(
                    &c.actor,
                    &text("event"),
                    &text("tier"),
                    web::number(&b, "qty").unwrap_or(1),
                    c.tick,
                )
                .map(|o| {
                    let url = format!("/orders/{}", o.id);
                    (json!(o), url)
                }),
            ["api", "checkout"] => s.checkout(&c.actor, c.tick).map(|o| {
                let url = format!("/orders/{}", o.id);
                (json!(o), url)
            }),
            ["api", "products", id, "reviews"] => s
                .review(
                    &c.actor,
                    id,
                    web::number(&b, "stars").unwrap_or(0),
                    &text("title"),
                    &text("body"),
                    c.tick,
                )
                .map(|rv| (json!(rv), format!("/dp/{id}"))),
            ["api", "products", id, "favorite"] => s
                .favorite(&c.actor, id)
                .map(|on| (json!({"product": id, "favorite": on}), "/favorites".into())),
            ["api", "messages"] => s
                .message_seller(&c.actor, &text("product"), &text("text"), c.tick)
                .map(|m| {
                    let url = format!("/dp/{}", m.product);
                    (json!(m), url)
                }),
            _ => return web::error(404, "route not found"),
        };
        let (value, next) = match outcome {
            Ok(v) => v,
            Err(e) => return web::domain::<Value>(Err(e)),
        };
        web::save(state, &s)?;
        // A browser form wants the page it just changed; an API client wants the record.
        let submitted = r
            .header("content-type")
            .is_some_and(|h| h.starts_with("application/x-www-form-urlencoded"));
        if !submitted {
            return HttpResponse::json(200, &value);
        }
        let view = View::new(&s, &c.actor);
        let parts: Vec<&str> = next.trim_matches('/').split('/').collect();
        match parts.as_slice() {
            ["cart"] => view.cart(),
            ["favorites"] => view.favorites(),
            ["orders", id] => view.order(id),
            ["dp", id] => view.detail(id),
            _ => HttpResponse::json(200, &value),
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn ctx() -> ServiceContext {
        ServiceContext {
            actor: "alice".into(),
            source: "alice-mac".into(),
            tick: 7,
            seed: 1,
            instance: "shop".into(),
        }
    }
    fn retail() -> Value {
        json!({
            "mode": "retail", "brand": "Testmart", "next_order": 1001,
            "theme": {"accent": "#146eb4"},
            "categories": [{"id": "electronics", "title": "Electronics"}],
            "products": {
                "mon27": {"id": "mon27", "category": "electronics", "seller": "Lumen Works",
                          "title": "Lumen 27-inch 4K USB-C Monitor", "price_cents": 42999,
                          "rating_sum": 184, "rating_count": 41, "stock": 2,
                          "bullets": ["3840x2160 at 60Hz"], "description": "A monitor."},
                "cbl": {"id": "cbl", "category": "electronics", "title": "USB-C Cable",
                        "price_cents": 1899, "stock": 0, "description": "A cable."}
            }
        })
    }
    fn post(state: &mut Value, url: &str, body: &[(&str, &str)]) -> HttpResponse {
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
        ShopService.handle(state, &ctx(), &r).unwrap()
    }
    fn get(state: &mut Value, url: &str) -> HttpResponse {
        ShopService
            .handle(state, &ctx(), &HttpRequest::get(url))
            .unwrap()
    }
    /// A page response parsed as the browser would parse it, and checked strictly.
    struct Dom(cw_web::dom::Document);
    fn dom(r: &HttpResponse) -> Dom {
        assert_eq!(r.header("content-type"), Some(cw_service_common::html::HTML_MEDIA_TYPE));
        let html = String::from_utf8(r.body.clone()).unwrap();
        cw_service_common::html::validate_strict(&html).unwrap_or_else(|e| panic!("{e}"));
        Dom(cw_web::html::parse(&html))
    }
    impl Dom {
        fn node(&self, id: &str) -> cw_web::dom::NodeId {
            *self.0.by_id(id).first().unwrap_or_else(|| panic!("no #{id}"))
        }
        fn has(&self, id: &str) -> bool {
            !self.0.by_id(id).is_empty()
        }
        fn text(&self, id: &str) -> String {
            cw_web::paint::semantics::collapse(&self.0.text_content(self.node(id)))
        }
        fn attr(&self, id: &str, name: &str) -> String {
            self.0.attr(self.node(id), name).unwrap_or_default().to_owned()
        }
        fn tag(&self, id: &str) -> String {
            self.0.tag(self.node(id)).unwrap_or_default().to_owned()
        }
        /// The `name=value` pairs a form would submit untouched, in document order.
        fn fields(&self, form: &str) -> Vec<(String, String)> {
            let root = self.node(form);
            self.0
                .descendants(root)
                .filter(|n| self.0.is(*n, "input"))
                .map(|n| (self.0.attr(n, "name").unwrap_or_default().to_owned(), self.0.attr(n, "value").unwrap_or_default().to_owned()))
                .collect()
        }
    }
    fn pairs(v: &[(&str, &str)]) -> Vec<(String, String)> {
        v.iter().map(|(k, v)| ((*k).to_owned(), (*v).to_owned())).collect()
    }
    #[test]
    fn seed_shape_is_gated_at_load() {
        assert!(ShopService.initialize(json!([]), &ctx()).is_err());
        assert!(ShopService
            .initialize(json!({"products": []}), &ctx())
            .is_err());
        assert!(ShopService
            .initialize(json!({"mode": "nonesuch"}), &ctx())
            .is_err());
        assert!(ShopService.initialize(Value::Null, &ctx()).is_ok());
    }
    #[test]
    fn catalogue_pages_render_and_do_not_mutate() {
        let mut state = ShopService.initialize(retail(), &ctx()).unwrap();
        let before = state.clone();
        for url in [
            "http://amazon.com/",
            "http://amazon.com/s?k=monitor",
            "http://amazon.com/s?c=electronics",
            "http://amazon.com/dp/mon27",
            "http://amazon.com/cart",
            "http://amazon.com/orders",
            "http://amazon.com/favorites",
        ] {
            let page = get(&mut state, url);
            assert_eq!(page.status, 200, "{url}");
            assert_eq!(page, get(&mut state, url), "{url} must be pure");
        }
        assert_eq!(before, state, "rendering must not mutate seed state");
        let hits = dom(&get(&mut state, "http://amazon.com/s?k=monitor"));
        assert_eq!(hits.text("n-mon27"), "Lumen 27-inch 4K USB-C Monitor");
        assert_eq!(hits.attr("p-mon27", "href"), "/dp/mon27");
        assert!(!hits.has("p-cbl"), "the cable does not match \"monitor\"");
        assert_eq!(hits.attr("hdr-k", "value"), "monitor");
        assert_eq!(get(&mut state, "http://amazon.com/dp/nope").status, 404);
        assert_eq!(get(&mut state, "http://amazon.com/nowhere").status, 404);
    }
    #[test]
    fn cart_checkout_creates_an_order_that_survives_a_round_trip() {
        let mut state = ShopService.initialize(retail(), &ctx()).unwrap();
        assert_eq!(
            post(
                &mut state,
                "http://amazon.com/api/cart",
                &[("product", "mon27"), ("qty", "1")]
            )
            .status,
            200
        );
        let cart = dom(&get(&mut state, "http://amazon.com/cart"));
        assert_eq!(cart.text("line-mon27-p"), "$429.99");
        assert_eq!(cart.text("subtotal"), "Subtotal (1 item): $429.99");
        assert_eq!(cart.fields("qty-mon27"), pairs(&[("product", "mon27"), ("qty", "1")]));
        assert_eq!(cart.fields("rm-mon27"), pairs(&[("product", "mon27"), ("qty", "0")]));
        assert_eq!((cart.attr("checkout", "action"), cart.attr("checkout", "method")), ("/api/checkout".into(), "post".into()));
        let placed = post(&mut state, "http://amazon.com/api/checkout", &[]);
        assert_eq!(placed.status, 200);
        assert_eq!(dom(&placed).text("lead"), "Order 1001");
        let s: ShopState = web::load(&state).unwrap();
        assert_eq!(s.orders["1001"].total_cents, 42999);
        assert_eq!(s.orders["1001"].buyer, "alice");
        assert_eq!(s.products["mon27"].stock, 1, "stock is decremented");
        assert!(s.carts.is_empty(), "checkout empties the cart");
        let orders = dom(&get(&mut state, "http://amazon.com/orders"));
        assert_eq!(orders.text("o-1001-id"), "Order 1001");
        assert_eq!(orders.attr("o-1001", "href"), "/orders/1001");
        let order = dom(&get(&mut state, "http://amazon.com/orders/1001"));
        assert_eq!(order.text("total"), "Order total: $429.99");
        assert_eq!(order.attr("again-0", "href"), "/dp/mon27");
        // The confirmation is a pure function of buyer and id, so a replay reproduces it.
        assert_eq!(
            s.orders["1001"].confirmation,
            confirmation("ORD", "alice", "1001")
        );
        let round: ShopState = serde_json::from_value(serde_json::to_value(&s).unwrap()).unwrap();
        assert_eq!(round, s);
    }
    #[test]
    fn checkout_is_refused_when_it_should_be() {
        let mut state = ShopService.initialize(retail(), &ctx()).unwrap();
        assert_eq!(
            post(&mut state, "http://amazon.com/api/checkout", &[]).status,
            400,
            "an empty cart cannot be ordered"
        );
        assert_eq!(
            post(
                &mut state,
                "http://amazon.com/api/cart",
                &[("product", "mon27"), ("qty", "9")]
            )
            .status,
            400,
            "more than stock is refused"
        );
        assert_eq!(
            post(
                &mut state,
                "http://amazon.com/api/cart",
                &[("product", "cbl"), ("qty", "1")]
            )
            .status,
            400,
            "a sold-out product cannot enter a cart"
        );
        let before = state.clone();
        assert_eq!(
            post(
                &mut state,
                "http://amazon.com/api/cart",
                &[("product", "ghost"), ("qty", "1")]
            )
            .status,
            400
        );
        assert_eq!(before, state, "a refused mutation leaves no trace");
    }
    /// A seed can hand an actor a cart line the catalogue can no longer fill (`set_cart`
    /// refuses `qty > stock`, so only a seed can). The line says how many are left rather
    /// than claiming "In stock" and letting the all-or-nothing checkout refuse the lot.
    #[test]
    fn a_cart_line_the_catalogue_cannot_fill_says_how_many_are_left() {
        let mut seed = retail();
        seed["carts"] = json!({"alice": {"mon27": 5}});
        let mut state = ShopService.initialize(seed, &ctx()).unwrap();
        let cart = dom(&get(&mut state, "http://amazon.com/cart"));
        assert!(cart.text("line-mon27-row").contains("Only 2 left"), "{}", cart.text("line-mon27-row"));
        assert_eq!(
            post(&mut state, "http://amazon.com/api/checkout", &[]).status,
            400,
            "and checkout still refuses the whole cart"
        );
    }
    #[test]
    fn reviews_favourites_and_seller_messages_mutate() {
        let mut state = ShopService.initialize(retail(), &ctx()).unwrap();
        assert_eq!(
            post(
                &mut state,
                "http://amazon.com/api/products/mon27/reviews",
                &[("stars", "4"), ("title", "Good"), ("body", "Crisp.")]
            )
            .status,
            200
        );
        let s: ShopState = web::load(&state).unwrap();
        assert_eq!(s.products["mon27"].rating_count, 42);
        assert_eq!(s.products["mon27"].rating_sum, 188);
        assert_eq!(s.products["mon27"].reviews.last().unwrap().author, "alice");
        assert_eq!(
            post(
                &mut state,
                "http://amazon.com/api/products/mon27/reviews",
                &[("stars", "9"), ("title", "Nope")]
            )
            .status,
            400,
            "stars outside 1-5 are refused"
        );
        post(
            &mut state,
            "http://amazon.com/api/products/mon27/favorite",
            &[],
        );
        let s: ShopState = web::load(&state).unwrap();
        assert_eq!(s.favorites["alice"], vec!["mon27".to_owned()]);
        post(
            &mut state,
            "http://amazon.com/api/products/mon27/favorite",
            &[],
        );
        let s: ShopState = web::load(&state).unwrap();
        assert!(s.favorites.is_empty(), "favourite toggles off");
        assert_eq!(
            post(
                &mut state,
                "http://amazon.com/api/messages",
                &[("product", "mon27"), ("text", "Ships to Seattle?")]
            )
            .status,
            200
        );
        let s: ShopState = web::load(&state).unwrap();
        assert_eq!(s.messages[0].to, "Lumen Works");
    }
    #[test]
    fn ticket_seats_are_allocated_from_remaining_without_randomness() {
        let seed = json!({
            "mode": "tickets", "brand": "Tickets", "next_order": 2210,
            "products": {"devcon-2026": {
                "id": "devcon-2026", "title": "DevCon Seattle 2026", "venue": "devcon-center",
                "venue_name": "Cascade Convention Center", "event_tick": 240,
                "event_date": "May 14, 2026", "description": "Two days of talks.",
                "tiers": [{"id": "floor", "title": "Floor", "price_cents": 24900,
                           "section": "A", "row_size": 20, "capacity": 500, "remaining": 234}]
            }}
        });
        let mut state = ShopService.initialize(seed, &ctx()).unwrap();
        let bought = post(
            &mut state,
            "http://ticketmaster.com/api/checkout",
            &[("event", "devcon-2026"), ("tier", "floor"), ("qty", "1")],
        );
        assert_eq!(bought.status, 200);
        let s: ShopState = web::load(&state).unwrap();
        assert_eq!(s.orders["TM-2210"].seats, vec!["A-14-7".to_owned()]);
        assert_eq!(s.orders["TM-2210"].total_cents, 24900);
        assert_eq!(s.products["devcon-2026"].tiers[0].remaining, 233);
        assert_eq!(dom(&bought).text("seat-0"), "Seat A-14-7");
        assert_eq!(dom(&get(&mut state, "http://ticketmaster.com/my-tickets")).text("o-TM-2210-id"), "Order TM-2210");
        let event = dom(&get(&mut state, "http://ticketmaster.com/event/devcon-2026"));
        assert_eq!(event.text("tier-floor-p"), "$249.00");
        assert_eq!(event.text("tier-floor-r"), "233 left");
        assert_eq!(event.fields("buy-floor"), pairs(&[("event", "devcon-2026"), ("tier", "floor"), ("qty", "1")]));
        assert_eq!((event.attr("buy-floor", "action"), event.attr("buy-floor", "method")), ("/api/checkout".into(), "post".into()));
        assert_eq!(event.tag("buy-floor-go"), "button");
        assert_eq!(event.attr("venue-map", "href"), "http://maps.google.com/maps/place/devcon-center");
        assert_eq!(
            post(
                &mut state,
                "http://ticketmaster.com/api/checkout",
                &[("event", "devcon-2026"), ("tier", "balcony"), ("qty", "1")]
            )
            .status,
            400
        );
    }
    /// Every id the `Page` version exposed is on the element that plays the same role.
    #[test]
    fn the_agent_ids_forms_and_links_survive_the_move_to_html() {
        let mut state = ShopService.initialize(retail(), &ctx()).unwrap();
        let home = dom(&get(&mut state, "http://amazon.com/"));
        for id in ["chrome", "wordmark", "hdr-search", "hdr-k", "hdr-search-go", "nav-basket", "nav-orders", "cats", "cat-electronics", "lead", "deals", "p-mon27", "t-mon27", "n-mon27", "r-mon27", "tags-mon27", "pr-mon27", "so-cbl", "foot"] {
            assert!(home.has(id), "home lacks #{id}");
        }
        assert_eq!((home.attr("hdr-search", "action"), home.attr("hdr-search", "method")), ("/s".into(), "get".into()));
        assert_eq!(home.attr("hdr-k", "name"), "k");
        assert_eq!(home.attr("hdr-k", "aria-label"), "Search the catalogue");
        assert_eq!(home.text("hdr-search-go"), "Search");
        assert_eq!((home.attr("nav-basket", "href"), home.attr("nav-basket", "aria-label")), ("/cart".into(), "Cart".into()));
        assert_eq!((home.attr("nav-orders", "href"), home.attr("nav-orders", "aria-label")), ("/orders".into(), "Orders".into()));
        assert_eq!(home.attr("cat-electronics", "href"), "/s?c=electronics");
        assert_eq!(home.text("pr-mon27"), "$429.99");
        assert_eq!(home.tag("p-mon27"), "a");
        let detail = dom(&get(&mut state, "http://amazon.com/dp/mon27"));
        for id in ["hero", "gallery", "shot", "strip", "shot-1", "shot-3", "buybox", "title", "rating", "price-row", "price", "stock", "bul-0", "desc-head", "desc", "seller", "rev-head", "write-go", "ask-go", "fav-go", "add-go"] {
            assert!(detail.has(id), "detail lacks #{id}");
        }
        assert_eq!(detail.text("price"), "$429.99");
        assert_eq!(detail.text("rating"), "4.4 out of 5 · 41 reviews");
        assert_eq!(detail.text("stock"), "2 in stock");
        assert_eq!((detail.attr("add", "action"), detail.attr("add", "method")), ("/api/cart".into(), "post".into()));
        assert_eq!(detail.fields("add"), pairs(&[("product", "mon27"), ("qty", "1")]));
        assert_eq!(detail.attr("add-qty", "name"), "qty");
        assert_eq!(detail.attr("fav", "action"), "/api/products/mon27/favorite");
        assert!(detail.fields("fav").is_empty());
        assert_eq!(detail.text("fav-go"), "Save to favourites");
        assert_eq!(detail.attr("ask", "action"), "/api/messages");
        assert_eq!(detail.fields("ask"), pairs(&[("product", "mon27"), ("text", "")]));
        assert_eq!(detail.attr("write", "action"), "/api/products/mon27/reviews");
        assert_eq!(detail.fields("write"), pairs(&[("stars", "5"), ("title", ""), ("body", "")]));
        // A sold-out product offers no add form; a posted review appears with its ids.
        let sold = dom(&get(&mut state, "http://amazon.com/dp/cbl"));
        assert!(!sold.has("add") && sold.text("stock") == "Sold out");
        let after = dom(&post(&mut state, "http://amazon.com/api/products/mon27/reviews", &[("stars", "4"), ("title", "Good <b>"), ("body", "Crisp & clear.")]));
        assert_eq!(after.text("rev-rv1-t"), "4★ Good <b>", "seed text is escaped, never markup");
        assert_eq!(after.text("rev-rv1-b"), "Crisp & clear.");
        assert_eq!(after.text("rev-rv1-a"), "alice · tick 7");
        let favs = dom(&post(&mut state, "http://amazon.com/api/products/mon27/favorite", &[]));
        assert!(favs.has("favs") && favs.has("p-mon27"));
        assert_eq!(dom(&get(&mut state, "http://amazon.com/dp/mon27")).text("fav-go"), "Remove favourite");
    }
    /// Every page of every skin is HTML the engine renders: strict CSS, unique ids.
    #[test]
    fn every_page_of_every_skin_passes_the_strict_validator() {
        for skin in view::SKINS {
            let mut seed = retail();
            seed["skin"] = json!(skin);
            seed["favorites"] = json!({"alice": ["mon27"]});
            seed["carts"] = json!({"alice": {"mon27": 1}});
            seed["products"]["mon27"]["fast_shipping"] = json!(true);
            seed["products"]["mon27"]["reviews"] = json!([{"id": "rv-1", "author": "bob", "stars": 4, "title": "Fine", "body": "Works.", "tick": 2}]);
            let mut state = ShopService.initialize(seed, &ctx()).unwrap();
            for path in ["/", "/s?k=monitor", "/s?c=electronics", "/s?k=zzz", "/dp/mon27", "/dp/cbl", "/cart", "/orders", "/favorites"] {
                let page = dom(&get(&mut state, &format!("http://shop.test{path}")));
                assert_eq!(page.attr("hdr-search", "action"), "/s", "{skin} {path}");
            }
            let placed = dom(&post(&mut state, "http://shop.test/api/checkout", &[]));
            assert_eq!(placed.text("lead"), "Order 1001", "{skin}");
            dom(&get(&mut state, "http://shop.test/orders"));
            dom(&get(&mut state, "http://shop.test/cart"));
        }
        // The brand picks the skin when the seed names none; an unknown skin is a seed typo.
        let mut named = retail();
        named["brand"] = json!("Booking.com");
        let s: ShopState = web::load(&ShopService.initialize(named, &ctx()).unwrap()).unwrap();
        assert_eq!(view::skin_of(&s), "booking");
        let s: ShopState = web::load(&ShopService.initialize(retail(), &ctx()).unwrap()).unwrap();
        assert_eq!(view::skin_of(&s), "plain");
        let mut typo = retail();
        typo["skin"] = json!("amazonn");
        assert!(ShopService.initialize(typo, &ctx()).is_err());
    }
    #[test]
    fn other_peoples_orders_are_not_visible() {
        let mut state = ShopService.initialize(retail(), &ctx()).unwrap();
        post(
            &mut state,
            "http://amazon.com/api/cart",
            &[("product", "mon27"), ("qty", "1")],
        );
        post(&mut state, "http://amazon.com/api/checkout", &[]);
        let bob = ServiceContext {
            actor: "bob".into(),
            ..ctx()
        };
        let page = ShopService
            .handle(
                &mut state,
                &bob,
                &HttpRequest::get("http://amazon.com/orders/1001"),
            )
            .unwrap();
        assert_eq!(page.status, 404);
    }
    #[test]
    fn non_get_post_methods_are_rejected() {
        let mut state = ShopService.initialize(retail(), &ctx()).unwrap();
        let mut r = HttpRequest::get("http://amazon.com/cart");
        r.method = "DELETE".into();
        assert_eq!(
            ShopService.handle(&mut state, &ctx(), &r).unwrap().status,
            405
        );
    }
}
