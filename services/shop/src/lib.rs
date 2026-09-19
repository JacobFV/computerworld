//! Commerce: amazon.com and etsy.com (`mode: retail`), ticketmaster.com (`mode: tickets`).
//! Checkout is the one irreversible mutation in the world, so stock and orders are authoritative.
//! Every amount is integer cents — a float total would not survive a replay bit for bit.
use cw_protocol::{HttpRequest, HttpResponse, PageAction, PageElement, PageTheme, Result};
use cw_sdk::{Registry, Service, ServiceContext};
use cw_service_common as web;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;
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
/// Resolved palette: the seed theme with renderer-neutral fallbacks, so a partial theme still
/// produces a page that reads correctly.
struct Palette {
    accent: String,
    ink: String,
    muted: String,
    surface: String,
}
fn palette(t: &PageTheme) -> Palette {
    Palette {
        accent: t.accent.clone().unwrap_or_else(|| "#146eb4".into()),
        ink: t.ink.clone().unwrap_or_else(|| "#0f1111".into()),
        muted: t.muted.clone().unwrap_or_else(|| "#565959".into()),
        surface: t.surface.clone().unwrap_or_else(|| "#eaeded".into()),
    }
}
fn act(method: &str, url: &str, fields: &[(&str, &str)]) -> PageAction {
    PageAction {
        method: method.into(),
        url: url.into(),
        fields: fields
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect(),
    }
}
fn input(id: &str, label: &str, value: &str) -> PageElement {
    PageElement::Input {
        id: id.into(),
        label: label.into(),
        value: value.into(),
        placeholder: String::new(),
    }
}
/// A form whose drawn controls are exactly the fields it submits.
fn form_el(
    id: &str,
    action: PageAction,
    mut children: Vec<PageElement>,
    submit: &str,
) -> PageElement {
    children.push(PageElement::Button {
        id: format!("{id}-go"),
        text: submit.into(),
        action: action.clone(),
        style: None,
    });
    PageElement::Form {
        id: id.into(),
        action,
        children,
    }
}
fn price_badge(id: &str, cents: u64, p: &Palette) -> PageElement {
    web::badge(
        id,
        money(cents),
        web::style()
            .bold()
            .size(16)
            .color("#ffffff")
            .background(p.accent.clone())
            .radius(4)
            .padding(6),
    )
}
fn thumb(id: &str, label: &str, height: u32, p: &Palette) -> PageElement {
    web::thumbnail(
        id,
        label,
        web::style()
            .height(height)
            .background(p.surface.clone())
            .color(p.muted.clone())
            .radius(6)
            .align("center"),
    )
}
/// Brand bar, catalogue search and the account routes — present on every page so the nav is real.
fn chrome(s: &ShopState, p: &Palette, query: &str) -> Vec<PageElement> {
    let search = form_el(
        "hdr-search",
        act("GET", "/s", &[("k", "$hdr-k")]),
        vec![input("hdr-k", "Search the catalogue", query)],
        "Search",
    );
    let (basket, basket_url) = if s.tickets() {
        ("My tickets", "/my-tickets")
    } else {
        ("Cart", "/cart")
    };
    vec![
        web::styled_row(
            "chrome",
            16,
            "center",
            web::style().background(p.ink.clone()).padding(12).radius(6),
            vec![
                web::styled(
                    "wordmark",
                    &s.brand,
                    web::style().size(22).bold().color("#ffffff").width(200),
                ),
                search,
                web::link("nav-basket", basket, basket_url),
                web::link("nav-orders", "Orders", "/orders"),
            ],
        ),
        web::spacer("chrome-gap", 12),
    ]
}
fn footer(id: &str, p: &Palette) -> Vec<PageElement> {
    vec![
        web::spacer(&format!("{id}-gap"), 16),
        web::divider(&format!("{id}-rule")),
        web::styled(
            id,
            "Simulated storefront in a training world. No real order is ever placed and no real \
             payment system is contacted.",
            web::style().size(12).color(p.muted.clone()),
        ),
    ]
}
fn product_card(s: &ShopState, product: &Product, p: &Palette) -> PageElement {
    let id = &product.id;
    let url = if s.tickets() {
        format!("/event/{id}")
    } else {
        format!("/dp/{id}")
    };
    let mut body = vec![
        thumb(&format!("t-{id}"), &product.title, 120, p),
        web::styled(
            &format!("n-{id}"),
            &product.title,
            web::style().size(15).medium().color(p.ink.clone()),
        ),
    ];
    if s.tickets() {
        body.push(web::styled(
            &format!("v-{id}"),
            format!("{} · {}", product.venue_name, product.event_date),
            web::style().size(13).color(p.muted.clone()),
        ));
    } else {
        body.push(web::styled(
            &format!("r-{id}"),
            product.rating_line(),
            web::style().size(13).color(p.muted.clone()),
        ));
    }
    let mut tags = vec![price_badge(
        &format!("pr-{id}"),
        product.cheapest_cents(),
        p,
    )];
    if product.fast_shipping {
        tags.push(web::badge(
            &format!("pf-{id}"),
            "Two-day",
            web::style()
                .size(12)
                .color(p.accent.clone())
                .border(p.accent.clone())
                .radius(4)
                .padding(4),
        ));
    }
    if !product.available() {
        tags.push(web::badge(
            &format!("so-{id}"),
            "Sold out",
            web::style()
                .size(12)
                .color("#8a1c1c")
                .background("#fbe9e7")
                .radius(4)
                .padding(4),
        ));
    }
    body.push(web::row(&format!("tags-{id}"), 8, "center", tags));
    web::card_action(
        &format!("p-{id}"),
        web::style()
            .background("#ffffff")
            .border("#d5d9d9")
            .radius(8)
            .padding(12),
        web::visit(url),
        body,
    )
}
fn home(s: &ShopState, p: &Palette) -> Result<HttpResponse> {
    let mut e = chrome(s, p, "");
    if !s.categories.is_empty() {
        e.push(web::row(
            "cats",
            14,
            "center",
            s.categories
                .iter()
                .map(|c| web::link(&format!("cat-{}", c.id), &c.title, format!("/s?c={}", c.id)))
                .collect(),
        ));
        e.push(web::spacer("cats-gap", 12));
    }
    e.push(web::styled(
        "lead",
        if s.tickets() {
            "On sale now"
        } else {
            "Today's picks"
        },
        web::style().size(24).bold().color(p.ink.clone()),
    ));
    e.push(web::spacer("lead-gap", 10));
    e.push(web::grid(
        "deals",
        3,
        14,
        s.products.values().map(|x| product_card(s, x, p)).collect(),
    ));
    e.extend(footer("foot", p));
    web::themed_page(&s.brand, s.theme.clone(), e)
}
fn results(s: &ShopState, p: &Palette, query: &str, category: &str) -> Result<HttpResponse> {
    let hits = s.search(query, category);
    let title = match (query.is_empty(), category.is_empty()) {
        (true, false) => format!("Browsing {category}"),
        (true, true) => "Everything".to_owned(),
        _ => format!("Results for \"{query}\""),
    };
    let mut e = chrome(s, p, query);
    e.push(web::styled(
        "lead",
        format!("{title} — {} item(s)", hits.len()),
        web::style().size(20).bold().color(p.ink.clone()),
    ));
    e.push(web::spacer("lead-gap", 10));
    if hits.is_empty() {
        e.push(web::styled(
            "empty",
            "Nothing matched. Try a broader word.",
            web::style().color(p.muted.clone()),
        ));
    } else {
        e.push(web::grid(
            "hits",
            3,
            14,
            hits.iter().map(|x| product_card(s, x, p)).collect(),
        ));
    }
    e.extend(footer("foot", p));
    web::themed_page(&format!("{} — {}", title, s.brand), s.theme.clone(), e)
}
fn detail(s: &ShopState, p: &Palette, actor: &str, id: &str) -> Result<HttpResponse> {
    let Some(product) = s.products.get(id) else {
        return web::error(404, "product not found");
    };
    let favorited = s
        .favorites
        .get(actor)
        .is_some_and(|f| f.iter().any(|x| x == id));
    let mut right = vec![
        web::styled(
            "title",
            &product.title,
            web::style().size(26).bold().color(p.ink.clone()),
        ),
        web::styled(
            "rating",
            product.rating_line(),
            web::style().size(14).color(p.muted.clone()),
        ),
        web::row(
            "price-row",
            10,
            "center",
            vec![
                price_badge("price", product.price_cents, p),
                web::badge(
                    "stock",
                    if product.stock > 0 {
                        format!("{} in stock", product.stock)
                    } else {
                        "Sold out".into()
                    },
                    web::style()
                        .size(12)
                        .color(if product.stock > 0 {
                            p.muted.clone()
                        } else {
                            "#8a1c1c".into()
                        })
                        .radius(4)
                        .padding(4),
                ),
            ],
        ),
    ];
    for (i, b) in product.bullets.iter().enumerate() {
        right.push(web::styled(
            &format!("bul-{i}"),
            format!("• {b}"),
            web::style().size(14).color(p.ink.clone()),
        ));
    }
    right.push(web::spacer("buy-gap", 8));
    if product.stock > 0 {
        right.push(form_el(
            "add",
            act("POST", "/api/cart", &[("product", id), ("qty", "$add-qty")]),
            vec![input("add-qty", "Quantity", "1")],
            "Add to cart",
        ));
    }
    right.push(form_el(
        "fav",
        act("POST", &format!("/api/products/{id}/favorite"), &[]),
        vec![],
        if favorited {
            "Remove favourite"
        } else {
            "Save to favourites"
        },
    ));
    let mut e = chrome(s, p, "");
    e.push(web::row(
        "hero",
        24,
        "start",
        vec![
            web::card(
                "gallery",
                web::style()
                    .background("#ffffff")
                    .border("#d5d9d9")
                    .radius(8)
                    .padding(12)
                    .width(360),
                vec![
                    thumb("shot", &product.title, 260, p),
                    web::row(
                        "strip",
                        8,
                        "center",
                        (1..4)
                            .map(|i| thumb(&format!("shot-{i}"), &format!("View {i}"), 56, p))
                            .collect(),
                    ),
                ],
            ),
            web::card(
                "buybox",
                web::style()
                    .background("#ffffff")
                    .border("#d5d9d9")
                    .radius(8)
                    .padding(16),
                right,
            ),
        ],
    ));
    e.push(web::spacer("desc-gap", 16));
    e.push(web::styled(
        "desc-head",
        "About this item",
        web::style().size(18).bold().color(p.ink.clone()),
    ));
    e.push(web::styled(
        "desc",
        &product.description,
        web::style().size(14).color(p.ink.clone()),
    ));
    if !product.seller.is_empty() {
        e.push(web::spacer("seller-gap", 12));
        e.push(web::styled(
            "seller",
            format!("Sold by {}", product.seller),
            web::style().size(14).color(p.muted.clone()),
        ));
        e.push(form_el(
            "ask",
            act(
                "POST",
                "/api/messages",
                &[("product", id), ("text", "$ask-text")],
            ),
            vec![input("ask-text", "Message the seller", "")],
            "Send message",
        ));
    }
    e.push(web::spacer("rev-gap", 16));
    e.push(web::styled(
        "rev-head",
        format!("{} review(s)", product.reviews.len()),
        web::style().size(18).bold().color(p.ink.clone()),
    ));
    for r in &product.reviews {
        e.push(web::card(
            &format!("rev-{}", r.id),
            web::style()
                .background("#ffffff")
                .border("#e3e6e6")
                .radius(6)
                .padding(10),
            vec![
                web::styled(
                    &format!("rev-{}-t", r.id),
                    format!("{}★ {}", r.stars, r.title),
                    web::style().size(15).medium().color(p.ink.clone()),
                ),
                web::styled(
                    &format!("rev-{}-b", r.id),
                    &r.body,
                    web::style().size(13).color(p.ink.clone()),
                ),
                web::styled(
                    &format!("rev-{}-a", r.id),
                    format!("{} · tick {}", r.author, r.tick),
                    web::style().size(12).color(p.muted.clone()),
                ),
            ],
        ));
    }
    e.push(form_el(
        "write",
        act(
            "POST",
            &format!("/api/products/{id}/reviews"),
            &[
                ("stars", "$write-stars"),
                ("title", "$write-title"),
                ("body", "$write-body"),
            ],
        ),
        vec![
            input("write-stars", "Stars (1-5)", "5"),
            input("write-title", "Headline", ""),
            input("write-body", "Your review", ""),
        ],
        "Post review",
    ));
    e.extend(footer("foot", p));
    web::themed_page(
        &format!("{} — {}", product.title, s.brand),
        s.theme.clone(),
        e,
    )
}
fn event(s: &ShopState, p: &Palette, id: &str) -> Result<HttpResponse> {
    let Some(product) = s.products.get(id) else {
        return web::error(404, "event not found");
    };
    let mut e = chrome(s, p, "");
    e.push(web::styled(
        "title",
        &product.title,
        web::style().size(26).bold().color(p.ink.clone()),
    ));
    e.push(web::styled(
        "when",
        format!(
            "{} · {} · tick {}",
            product.event_date, product.venue_name, product.event_tick
        ),
        web::style().size(14).color(p.muted.clone()),
    ));
    e.push(thumb("stage", &product.venue_name, 160, p));
    e.push(web::styled(
        "desc",
        &product.description,
        web::style().size(14).color(p.ink.clone()),
    ));
    e.push(web::spacer("tier-gap", 14));
    for t in &product.tiers {
        let mut row = vec![
            web::styled(
                &format!("tier-{}-t", t.id),
                &t.title,
                web::style().size(16).medium().color(p.ink.clone()).flex(2),
            ),
            price_badge(&format!("tier-{}-p", t.id), t.price_cents, p),
            web::badge(
                &format!("tier-{}-r", t.id),
                format!("{} left", t.remaining),
                web::style().size(12).color(p.muted.clone()).padding(4),
            ),
        ];
        if t.remaining > 0 {
            row.push(form_el(
                &format!("buy-{}", t.id),
                act(
                    "POST",
                    "/api/checkout",
                    &[
                        ("event", id),
                        ("tier", &t.id),
                        ("qty", &format!("$buy-{}-qty", t.id)),
                    ],
                ),
                vec![input(&format!("buy-{}-qty", t.id), "Tickets", "1")],
                "Buy",
            ));
        }
        e.push(web::card(
            &format!("tier-{}", t.id),
            web::style()
                .background("#ffffff")
                .border("#d5d9d9")
                .radius(8)
                .padding(12),
            vec![web::row(&format!("tier-{}-row", t.id), 12, "center", row)],
        ));
    }
    if !product.venue.is_empty() {
        e.push(web::link(
            "venue-map",
            format!("Directions to {}", product.venue_name),
            format!("http://maps.google.com/maps/place/{}", product.venue),
        ));
    }
    e.extend(footer("foot", p));
    web::themed_page(
        &format!("{} — {}", product.title, s.brand),
        s.theme.clone(),
        e,
    )
}
fn cart_page(s: &ShopState, p: &Palette, actor: &str) -> Result<HttpResponse> {
    let cart = s.cart(actor);
    let mut e = chrome(s, p, "");
    e.push(web::styled(
        "lead",
        "Shopping cart",
        web::style().size(24).bold().color(p.ink.clone()),
    ));
    if cart.is_empty() {
        e.push(web::styled(
            "empty",
            "Your cart is empty.",
            web::style().color(p.muted.clone()),
        ));
    }
    for (id, qty) in &cart {
        let Some(product) = s.products.get(id) else {
            continue;
        };
        e.push(web::card(
            &format!("line-{id}"),
            web::style()
                .background("#ffffff")
                .border("#d5d9d9")
                .radius(8)
                .padding(12),
            vec![web::row(
                &format!("line-{id}-row"),
                14,
                "center",
                vec![
                    thumb(&format!("line-{id}-t"), &product.title, 72, p),
                    web::styled(
                        &format!("line-{id}-n"),
                        &product.title,
                        web::style().size(15).medium().color(p.ink.clone()).flex(3),
                    ),
                    price_badge(&format!("line-{id}-p"), product.price_cents * qty, p),
                    form_el(
                        &format!("qty-{id}"),
                        act(
                            "POST",
                            "/api/cart",
                            &[("product", id), ("qty", &format!("$qty-{id}-n"))],
                        ),
                        vec![input(&format!("qty-{id}-n"), "Qty", &qty.to_string())],
                        "Update",
                    ),
                    form_el(
                        &format!("rm-{id}"),
                        act("POST", "/api/cart", &[("product", id), ("qty", "0")]),
                        vec![],
                        "Remove",
                    ),
                ],
            )],
        ));
    }
    e.push(web::spacer("sum-gap", 12));
    e.push(web::styled(
        "subtotal",
        format!("Subtotal: {}", money(s.cart_total(actor))),
        web::style().size(20).bold().color(p.ink.clone()),
    ));
    if !cart.is_empty() {
        e.push(form_el(
            "checkout",
            act("POST", "/api/checkout", &[]),
            vec![],
            "Place your order",
        ));
    }
    e.extend(footer("foot", p));
    web::themed_page(&format!("Cart — {}", s.brand), s.theme.clone(), e)
}
fn order_list(s: &ShopState, p: &Palette, actor: &str) -> Result<HttpResponse> {
    let orders = s.orders_of(actor);
    let mut e = chrome(s, p, "");
    e.push(web::styled(
        "lead",
        if s.tickets() {
            "My tickets"
        } else {
            "Your orders"
        },
        web::style().size(24).bold().color(p.ink.clone()),
    ));
    if orders.is_empty() {
        e.push(web::styled(
            "empty",
            "No orders yet.",
            web::style().color(p.muted.clone()),
        ));
    }
    for o in orders {
        e.push(web::card_action(
            &format!("o-{}", o.id),
            web::style()
                .background("#ffffff")
                .border("#d5d9d9")
                .radius(8)
                .padding(12),
            web::visit(format!("/orders/{}", o.id)),
            vec![web::row(
                &format!("o-{}-row", o.id),
                14,
                "center",
                vec![
                    web::styled(
                        &format!("o-{}-id", o.id),
                        format!("Order {}", o.id),
                        web::style().size(15).bold().color(p.ink.clone()).width(160),
                    ),
                    web::styled(
                        &format!("o-{}-t", o.id),
                        o.items
                            .iter()
                            .map(|i| i.title.as_str())
                            .collect::<Vec<_>>()
                            .join(", "),
                        web::style()
                            .size(14)
                            .color(p.ink.clone())
                            .flex(3)
                            .one_line(),
                    ),
                    web::badge(
                        &format!("o-{}-s", o.id),
                        &o.status,
                        web::style().size(12).color(p.muted.clone()).padding(4),
                    ),
                    price_badge(&format!("o-{}-p", o.id), o.total_cents, p),
                ],
            )],
        ));
    }
    e.extend(footer("foot", p));
    web::themed_page(&format!("Orders — {}", s.brand), s.theme.clone(), e)
}
fn order_detail(s: &ShopState, p: &Palette, actor: &str, id: &str) -> Result<HttpResponse> {
    let Some(o) = s.orders.get(id).filter(|o| o.buyer == actor) else {
        return web::error(404, "order not found");
    };
    let mut e = chrome(s, p, "");
    e.push(web::styled(
        "lead",
        format!("Order {}", o.id),
        web::style().size(24).bold().color(p.ink.clone()),
    ));
    e.push(web::styled(
        "meta",
        format!(
            "{} · confirmation {} · {}",
            if o.date.is_empty() {
                format!("tick {}", o.tick)
            } else {
                o.date.clone()
            },
            o.confirmation,
            o.status
        ),
        web::style().size(14).color(p.muted.clone()),
    ));
    for (i, item) in o.items.iter().enumerate() {
        e.push(web::card(
            &format!("it-{i}"),
            web::style()
                .background("#ffffff")
                .border("#d5d9d9")
                .radius(8)
                .padding(12),
            vec![web::row(
                &format!("it-{i}-row"),
                14,
                "center",
                vec![
                    web::styled(
                        &format!("it-{i}-n"),
                        &item.title,
                        web::style().size(15).medium().color(p.ink.clone()).flex(3),
                    ),
                    web::styled(
                        &format!("it-{i}-q"),
                        format!("x{}", item.qty),
                        web::style().size(14).color(p.muted.clone()).width(60),
                    ),
                    price_badge(&format!("it-{i}-p"), item.price_cents * item.qty, p),
                ],
            )],
        ));
    }
    if !o.seats.is_empty() {
        e.push(web::row(
            "seats",
            8,
            "center",
            o.seats
                .iter()
                .enumerate()
                .map(|(i, seat)| {
                    web::badge(
                        &format!("seat-{i}"),
                        format!("Seat {seat}"),
                        web::style()
                            .size(13)
                            .bold()
                            .color("#ffffff")
                            .background(p.accent.clone())
                            .radius(4)
                            .padding(6),
                    )
                })
                .collect(),
        ));
    }
    e.push(web::divider("total-rule"));
    e.push(web::styled(
        "total",
        format!("Order total: {}", money(o.total_cents)),
        web::style().size(20).bold().color(p.ink.clone()),
    ));
    for (i, item) in o.items.iter().enumerate() {
        if s.products.contains_key(&item.product) {
            let url = if s.tickets() {
                format!("/event/{}", item.product)
            } else {
                format!("/dp/{}", item.product)
            };
            e.push(web::link(
                &format!("again-{i}"),
                format!("View {}", item.title),
                url,
            ));
        }
    }
    e.extend(footer("foot", p));
    web::themed_page(&format!("Order {} — {}", o.id, s.brand), s.theme.clone(), e)
}
fn favorites(s: &ShopState, p: &Palette, actor: &str) -> Result<HttpResponse> {
    let list = s.favorites.get(actor).cloned().unwrap_or_default();
    let mut e = chrome(s, p, "");
    e.push(web::styled(
        "lead",
        "Favourites",
        web::style().size(24).bold().color(p.ink.clone()),
    ));
    if list.is_empty() {
        e.push(web::styled(
            "empty",
            "Nothing saved yet.",
            web::style().color(p.muted.clone()),
        ));
    } else {
        e.push(web::grid(
            "favs",
            3,
            14,
            list.iter()
                .filter_map(|id| s.products.get(id))
                .map(|x| product_card(s, x, p))
                .collect(),
        ));
    }
    e.extend(footer("foot", p));
    web::themed_page(&format!("Favourites — {}", s.brand), s.theme.clone(), e)
}
impl Service for ShopService {
    fn kind(&self) -> &str {
        "shop"
    }
    fn initialize(&self, initial: Value, _: &ServiceContext) -> Result<Value> {
        let gated = web::shape(initial, OBJECTS, ARRAYS)?;
        let mode = web::variant(&gated, "mode", MODES)?;
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
        let p = palette(&s.theme);
        let path = web::path(r);
        let parts: Vec<&str> = path.trim_matches('/').split('/').collect();
        let method = r.method.to_ascii_uppercase();
        if method == "GET" {
            return match parts.as_slice() {
                [""] => home(&s, &p),
                ["s"] => results(
                    &s,
                    &p,
                    &web::query(r, "k").unwrap_or_default(),
                    &web::query(r, "c").unwrap_or_default(),
                ),
                ["dp", id] => detail(&s, &p, &c.actor, id),
                ["event", id] => event(&s, &p, id),
                ["cart"] => cart_page(&s, &p, &c.actor),
                ["orders"] | ["my-tickets"] => order_list(&s, &p, &c.actor),
                ["orders", id] => order_detail(&s, &p, &c.actor, id),
                ["favorites"] => favorites(&s, &p, &c.actor),
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
        let p = palette(&s.theme);
        let parts: Vec<&str> = next.trim_matches('/').split('/').collect();
        match parts.as_slice() {
            ["cart"] => cart_page(&s, &p, &c.actor),
            ["favorites"] => favorites(&s, &p, &c.actor),
            ["orders", id] => order_detail(&s, &p, &c.actor, id),
            ["dp", id] => detail(&s, &p, &c.actor, id),
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
    fn text(r: &HttpResponse) -> String {
        String::from_utf8(r.body.clone()).unwrap()
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
        assert!(text(&get(&mut state, "http://amazon.com/s?k=monitor")).contains("Lumen"));
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
        assert!(text(&get(&mut state, "http://amazon.com/cart")).contains("429.99"));
        let placed = post(&mut state, "http://amazon.com/api/checkout", &[]);
        assert_eq!(placed.status, 200);
        assert!(text(&placed).contains("Order 1001"));
        let s: ShopState = web::load(&state).unwrap();
        assert_eq!(s.orders["1001"].total_cents, 42999);
        assert_eq!(s.orders["1001"].buyer, "alice");
        assert_eq!(s.products["mon27"].stock, 1, "stock is decremented");
        assert!(s.carts.is_empty(), "checkout empties the cart");
        assert!(text(&get(&mut state, "http://amazon.com/orders")).contains("Order 1001"));
        assert!(text(&get(&mut state, "http://amazon.com/orders/1001")).contains("429.99"));
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
        assert!(text(&get(&mut state, "http://ticketmaster.com/my-tickets")).contains("TM-2210"));
        assert!(text(&get(
            &mut state,
            "http://ticketmaster.com/event/devcon-2026"
        ))
        .contains("249.00"));
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
