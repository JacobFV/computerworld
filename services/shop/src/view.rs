//! Page rendering as HTML. Every storefront serves the same routes; the skin decides
//! whether the page reads as Amazon, eBay, Etsy, Airbnb, Booking.com, Uber, DoorDash or
//! Ticketmaster. `shop.css` is the complete neutral look (the `plain` skin) and each
//! `<skin>.css` next to it overrides the header, the cards and the palette; the seed's
//! theme goes on `<html>` as custom properties so the sheets stay static files.
//!
//! Element ids are the agent API and are the ones the `Page` version used: `chrome`,
//! `wordmark`, `hdr-search` (GET `/s`, field `k` from `hdr-k`, button `hdr-search-go`),
//! `nav-basket`, `nav-orders`, `cats` and `cat-<id>`, `lead`, `deals`/`hits`/`favs`,
//! `p-<id>` (the whole card is one link, with `t-`, `n-`, `r-`/`v-`, `tags-`, `pr-`,
//! `pf-`, `so-<id>` inside), the detail page's `hero`, `gallery`, `shot`, `strip`,
//! `shot-<n>`, `buybox`, `title`, `rating`, `price-row`, `price`, `stock`, `bul-<n>`,
//! `add`/`add-qty`/`add-go`, `fav`/`fav-go`, `desc-head`, `desc`, `seller`,
//! `ask`/`ask-text`/`ask-go`, `rev-head`, `rev-<id>` (`-t`, `-b`, `-a`),
//! `write`/`write-stars`/`write-title`/`write-body`/`write-go`, the event page's `when`,
//! `stage`, `tier-<id>` (`-row`, `-t`, `-p`, `-r`), `buy-<tier>`/`-qty`/`-go`,
//! `venue-map`, the cart's `line-<id>` (`-row`, `-t`, `-n`, `-p`), `qty-<id>`/`-n`/`-go`,
//! `rm-<id>`/`-go`, `subtotal`, `checkout`/`checkout-go`, the order list's `o-<id>`
//! (`-row`, `-id`, `-t`, `-s`, `-p`), the order's `meta`, `it-<n>` (`-row`, `-n`, `-q`,
//! `-p`), `seats`, `seat-<n>`, `total-rule`, `total`, `again-<n>`, and `foot`, `empty`.
//! New, view-only: `nav-favorites`, `cats-all`, `cats-menu` (the header's category entry,
//! a link to `/s`), `top-<category>` (the top bar's category links), and
//! `foot-home|orders|basket|favorites`. `nav-orders` is a store's, not a box office's: a
//! box office keeps its orders behind `nav-basket` and would otherwise draw two entries
//! onto the same page. A link that leads to the page it sits on carries `aria-current`.
use crate::{money, Order, Product, ShopState};
use cw_protocol::{HttpResponse, Result};
use cw_service_common as web;
use cw_service_common::html::{
    button, div, el, empty, form, hidden, href, label, link, page, span, text_input, Document, Html,
};

const BASE: &str = include_str!("shop.css");
/// `skin` is an optional seed key; absent, the brand decides, and an unknown brand is `plain`.
pub(crate) const SKINS: &[&str] = &[
    "plain",
    "amazon",
    "ebay",
    "etsy",
    "airbnb",
    "booking",
    "uber",
    "doordash",
    "ticketmaster",
];
fn skin_css(skin: &str) -> &'static str {
    match skin {
        "amazon" => include_str!("amazon.css"),
        "ebay" => include_str!("ebay.css"),
        "etsy" => include_str!("etsy.css"),
        "airbnb" => include_str!("airbnb.css"),
        "booking" => include_str!("booking.css"),
        "uber" => include_str!("uber.css"),
        "doordash" => include_str!("doordash.css"),
        "ticketmaster" => include_str!("ticketmaster.css"),
        _ => "",
    }
}
pub(crate) fn skin_of(s: &ShopState) -> &'static str {
    let wanted = if s.skin.is_empty() {
        s.brand.to_ascii_lowercase().replace(".com", "").replace(' ', "")
    } else {
        s.skin.clone()
    };
    SKINS.iter().copied().find(|k| *k == wanted).unwrap_or("plain")
}
fn hash(text: &str) -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    for b in text.bytes() {
        h = (h ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}
/// Two tones for a synthetic picture, chosen from its label, stable across renders.
fn tones(label: &str) -> (&'static str, &'static str) {
    const PAIRS: [(&str, &str); 10] = [
        ("#8fb4d9", "#3d6a98"),
        ("#e2b07a", "#a8622d"),
        ("#9ccfb0", "#3d7f5d"),
        ("#d8a0b4", "#93506b"),
        ("#b9a7e0", "#6650a4"),
        ("#e6cf7d", "#a88a25"),
        ("#8ed0d6", "#2f7f88"),
        ("#e59d8b", "#a4483a"),
        ("#a9b8c6", "#566878"),
        ("#b8d38a", "#5f7d2e"),
    ];
    PAIRS[(hash(label) % 10) as usize]
}
/// A synthetic picture: a two-tone box carrying its label, the way a photo would sit there.
fn pic(id: &str, label_text: &str, class: &str) -> Html {
    let (a, b) = tones(label_text);
    span("pic")
        .class(class)
        .id(id)
        .style(&format!("--c1: {a}; --c2: {b}"))
        .child(el("i").class("obj"))
        .child(span("pic-label").text(label_text))
}
/// Five grey stars with the earned share drawn over them; `tenths` is 0 through 50. The
/// glyphs are generated content, so the element's text stays the rating line beside it.
fn stars(tenths: u64) -> Html {
    span("stars")
        .attr("aria-hidden", "true")
        .child(el("i").style(&format!("width: {}%", tenths.min(50) * 2)))
}
/// `APR`, `14` from `2026-04-14`; a date the seed wrote some other way is shown whole.
fn date_parts(date: &str) -> Option<(&'static str, String, String)> {
    const MONTHS: [&str; 12] = ["JAN", "FEB", "MAR", "APR", "MAY", "JUN", "JUL", "AUG", "SEP", "OCT", "NOV", "DEC"];
    let mut it = date.split('-');
    let (y, m, d) = (it.next()?, it.next()?.parse::<usize>().ok()?, it.next()?.parse::<u32>().ok()?);
    if it.next().is_some() || !(1..=12).contains(&m) || y.len() != 4 {
        return None;
    }
    Some((MONTHS[m - 1], d.to_string(), y.to_owned()))
}
/// `1 item`, `4 items`: a count that reads as prose rather than `4 item(s)`.
fn count(n: usize, word: &str) -> String {
    if n == 1 {
        format!("{n} {word}")
    } else {
        format!("{n} {word}s")
    }
}
/// Booking's score out of ten, in tenths, and the word beside it.
fn score(product: &Product) -> Option<(u64, &'static str)> {
    let tenths = (product.rating_sum * 20 + product.rating_count / 2).checked_div(product.rating_count)?;
    let word = match tenths {
        90.. => "Superb",
        85..=89 => "Fabulous",
        80..=84 => "Very good",
        70..=79 => "Good",
        _ => "Pleasant",
    };
    Some((tenths, word))
}

pub(crate) struct View<'a> {
    s: &'a ShopState,
    skin: &'static str,
    actor: &'a str,
    /// The page being rendered, so a link to it can say so rather than offering the reader
    /// a trip to where they already are.
    here: String,
}
impl<'a> View<'a> {
    pub(crate) fn new(s: &'a ShopState, actor: &'a str) -> Self {
        Self { s, skin: skin_of(s), actor, here: String::new() }
    }
    /// The page this render is of: `/`, `/cart`, `/s?c=<id>`, ...
    fn at(&self, url: &str) -> bool {
        url == self.here
    }
    /// A link that says when it leads to the page it sits on.
    fn here_aware(&self, node: Html, url: &str) -> Html {
        node.when(self.at(url), |n| n.attr("aria-current", "page"))
    }
    fn is(&self, skin: &str) -> bool {
        self.skin == skin
    }
    /// Stays are priced per night, and a round price drops its cents, as those sites do.
    fn stay(&self) -> bool {
        self.is("airbnb") || self.is("booking")
    }
    fn product_url(&self, id: &str) -> String {
        if self.s.tickets() {
            format!("/event/{id}")
        } else {
            format!("/dp/{id}")
        }
    }
    fn basket(&self) -> (&'static str, &'static str) {
        if self.s.tickets() {
            ("My tickets", "/my-tickets")
        } else {
            ("Cart", "/cart")
        }
    }
    fn cart_count(&self) -> u64 {
        self.s.cart(self.actor).values().sum()
    }
    /// A price. Amazon sets the currency sign and the cents as superscripts; the point
    /// between them stays in the text (off screen) so the element still reads `$429.99`.
    fn price(&self, id: &str, cents: u64) -> Html {
        let node = span("price").when(!id.is_empty(), |n| n.id(id));
        if self.is("amazon") {
            return node
                .child(el("sup").class("cur").text("$"))
                .child(span("whole").text((cents / 100).to_string()))
                .child(span("sr").text("."))
                .child(el("sup").class("frac").text(format!("{:02}", cents % 100)));
        }
        if self.stay() && cents.is_multiple_of(100) {
            return node.text(format!("${}", cents / 100));
        }
        node.text(money(cents))
    }
    fn per(&self) -> Html {
        if self.stay() {
            span("per").text(" night")
        } else {
            empty()
        }
    }

    // ---- chrome -------------------------------------------------------------------
    fn logo(&self) -> Html {
        let brand = self.s.brand.as_str();
        let mark = self.here_aware(el("a").id("wordmark").class("logo").attr("href", "/").attr("aria-label", brand), "/");
        match self.skin {
            "ebay" => mark.each(brand.chars().enumerate(), |(i, c)| span(&format!("c{}", i % 4)).text(c.to_string())),
            "doordash" => mark.child(span("dash")).child(span("name").text(brand)),
            "booking" => {
                let (name, tld) = brand.split_once('.').unwrap_or((brand, ""));
                mark.child(span("name").text(name)).when(!tld.is_empty(), |m| m.child(span("tld").text(format!(".{tld}"))))
            }
            _ => mark.child(span("name").text(brand)),
        }
    }
    fn search(&self, query: &str, placeholder: &str, scope: &str) -> Html {
        form("hdr-search", "/s", "get").class("search").attr("role", "search")
            .when(!scope.is_empty(), |f| f.child(span("scope").text(scope)))
            .child(span("glass").attr("aria-hidden", "true"))
            .child(
                text_input("hdr-k", "k", query)
                    .attr("aria-label", "Search the catalogue")
                    .attr("placeholder", placeholder)
                    .attr("autocomplete", "off"),
            )
            .child(button("hdr-search-go", "Search").class("go"))
    }
    fn nav_basket(&self, shown: &str) -> Html {
        let (text, url) = self.basket();
        let count = self.cart_count();
        self.here_aware(el("a").id("nav-basket").class("nav cart").attr("href", url).attr("aria-label", text), url)
            .child(span("ico").attr("aria-hidden", "true"))
            .when(!self.s.tickets(), |a| a.child(span("count").text(count.to_string())))
            .child(span("l2").text(if shown.is_empty() { text } else { shown }))
    }
    /// The orders entry. A box office has no second order list: its tickets live behind
    /// `nav-basket`, and a duplicate "Orders" beside it would land on the same page.
    /// The words a storefront lines its top bar with. The catalogue's own categories, as
    /// links that browse them: the hard-coded list they replace led nowhere at all.
    fn top_cats(&self, take: usize) -> Vec<Html> {
        self.s
            .categories
            .iter()
            .take(take)
            .map(|c| {
                let url = href("/s", &[("c", c.id.as_str())]);
                self.here_aware(
                    el("a").id(format!("top-{}", c.id)).class("u").attr("href", url.as_str()).text(c.title.as_str()),
                    &url,
                )
            })
            .collect()
    }
    fn nav_orders(&self, line1: &str, line2: &str) -> Html {
        if self.s.tickets() {
            return empty();
        }
        self.nav("nav-orders", "/orders", "Orders", line1, line2)
    }
    fn nav(&self, id: &str, url: &str, name: &str, line1: &str, line2: &str) -> Html {
        self.here_aware(el("a").id(id).class("nav").attr("href", url).attr("aria-label", name), url)
            .child(span("ico").attr("aria-hidden", "true"))
            .when(!line1.is_empty(), |a| a.child(span("l1").text(line1)))
            .child(span("l2").text(line2))
    }
    fn header(&self, query: &str, home: bool) -> Html {
        let me = self.actor;
        let head = el("header").id("chrome").class("hdr");
        let tagline = self.s.tagline.as_str();
        match self.skin {
            "amazon" => head.child(
                div("bar")
                    .child(self.logo())
                    .child(div("deliver").child(span("pin")).child(span("l1").text(format!("Deliver to {me}"))).child(span("l2").text("Seattle 98101")))
                    .child(self.search(query, "Search Amazon", "All"))
                    .child(div("lang").child(span("flag")).child(span("l2").text("EN")))
                    .child(self.nav("nav-favorites", "/favorites", "Favourites", &format!("Hello, {me}"), "Account & Lists"))
                    .child(self.nav_orders("Returns", "& Orders"))
                    .child(self.nav_basket("")),
            ),
            "ebay" => head
                .child(
                    div("util")
                        .child(span("hi").text(format!("Hi {me}!")))
                        .child(span("u").text("Daily Deals"))
                        .child(span("u").text("Brand Outlet"))
                        .child(span("u").text("Help & Contact"))
                        .child(span("grow"))
                        .child(span("u").text("Ship to"))
                        .child(span("u").text("Sell"))
                        .child(self.nav("nav-favorites", "/favorites", "Favourites", "", "Watchlist"))
                        .child(self.nav_orders("", "My eBay"))
                        .child(span("bell"))
                        .child(self.nav_basket("")),
                )
                .child(
                    div("bar")
                        .child(self.logo())
                        .child(self.here_aware(el("a").id("cats-menu").class("shopby").attr("href", "/s").text("Shop by category"), "/s"))
                        .child(self.search(query, "Search for anything", "All Categories")),
                ),
            "etsy" => head.child(
                div("bar")
                    .child(self.logo())
                    .child(self.here_aware(
                        el("a").id("cats-menu").class("menu").attr("href", "/s").child(span("burger")).child(span("").text("Categories")),
                        "/s",
                    ))
                    .child(self.search(query, "Search for anything", ""))
                    .child(self.nav("nav-favorites", "/favorites", "Favourites", "", "Favourites"))
                    .child(self.nav_orders("", "Orders"))
                    .child(self.nav_basket("")),
            ),
            "airbnb" => head.child(
                div("bar")
                    .child(self.logo())
                    .child(self.search(query, "Search destinations", "Anywhere"))
                    .child(
                        div("right")
                            .child(span("host").text("Airbnb your home"))
                            .child(span("globe"))
                            .child(self.nav("nav-favorites", "/favorites", "Favourites", "", "Wishlists"))
                            .child(self.nav_orders("", "Trips"))
                            .child(self.nav_basket("Reserve")),
                    ),
            ),
            "booking" => head
                .child(
                    div("bar")
                        .child(self.logo())
                        .child(span("grow"))
                        .child(span("cur").text("USD"))
                        .child(span("flag"))
                        .child(span("help").text("?"))
                        .child(span("list").text("List your property"))
                        .child(self.nav("nav-favorites", "/favorites", "Favourites", "", "Saved"))
                        .child(self.nav_orders("", "Bookings"))
                        .child(self.nav_basket("Basket")),
                )
                .when(home, |h| {
                    h.child(
                        div("hero")
                            .child(el("h1").text("Find your next stay"))
                            .child(el("p").text("Search deals on hotels, homes, and much more...")),
                    )
                })
                .child(
                    div("searchband").child(self.search(query, "Where are you going?", "")),
                ),
            "uber" => head
                .child(
                    div("bar")
                        .child(self.logo())
                        .children(self.top_cats(5))
                        .child(span("grow"))
                        .child(span("u").text("EN"))
                        .child(self.nav("nav-favorites", "/favorites", "Favourites", "", "Saved"))
                        .child(self.nav_orders("", "Activity"))
                        .child(self.nav_basket("")),
                )
                .child(
                    div("hero")
                        .when(!home, |h| h.class("slim"))
                        .child(
                            div("pitch")
                                .when(home, |p| p.child(el("h1").text("Go anywhere with Uber")).child(el("p").class("sub").text("Request a ride, hop in, and go.")))
                                .child(self.search(query, "Where to?", "")),
                        )
                        .when(home, |h| h.child(div("art").child(span("road")).child(span("car")).child(span("sun")))),
                ),
            "doordash" => head.child(
                div("bar")
                    .child(span("burger"))
                    .child(self.logo())
                    .child(div("addr").child(span("pin")).child(span("").text("410 Pine St")))
                    .child(self.search(query, "Search stores, dishes, products", ""))
                    .child(self.nav("nav-favorites", "/favorites", "Favourites", "", "Saved"))
                    .child(self.nav_orders("", "Orders"))
                    .child(self.nav_basket("")),
            ),
            "ticketmaster" => head
                .child(
                    div("bar")
                        .child(self.logo())
                        .children(self.top_cats(4))
                        .child(span("grow"))
                        .child(self.nav("nav-favorites", "/favorites", "Favourites", "", "Favorites"))
                        .child(self.nav_orders("", "Orders"))
                        .child(self.nav_basket("")),
                )
                .child(
                    div("hero")
                        .when(!home, |h| h.class("slim"))
                        .when(home, |h| h.child(el("h1").text("Let's make live happen")).child(el("p").class("sub").text(tagline)))
                        .child(self.search(query, "Search by artist, event or venue", "")),
                ),
            _ => head.child(
                div("bar")
                    .child(self.logo())
                    .child(self.search(query, "Search", ""))
                    .child(self.nav("nav-favorites", "/favorites", "Favourites", "", "Favourites"))
                    .child(self.nav_orders("", "Orders"))
                    .child(self.nav_basket("")),
            ),
        }
    }
    /// The category strip: real links that browse the catalogue by category.
    fn cats(&self, current: &str) -> Html {
        if self.s.categories.is_empty() {
            return empty();
        }
        el("nav").id("cats").class("cats").child(
            div("inner")
                .child(
                    self.here_aware(el("a").id("cats-all").class(if current.is_empty() { "cat all" } else { "cat all off" }).attr("href", "/s"), "/s")
                        .child(span("ico").attr("aria-hidden", "true"))
                        .child(span("t").text("All")),
                )
                .each(&self.s.categories, |c| {
                    let (a, b) = tones(&c.id);
                    self.here_aware(
                        el("a").id(format!("cat-{}", c.id)).class(if c.id == current { "cat on" } else { "cat" }).attr("href", href("/s", &[("c", c.id.as_str())])),
                        &href("/s", &[("c", c.id.as_str())]),
                    )
                        .child(span("ico").attr("aria-hidden", "true").style(&format!("--c1: {a}; --c2: {b}")).attr("data-letter", c.title.chars().next().unwrap_or(' ').to_string()))
                        .child(span("t").text(c.title.as_str()))
                }),
        )
    }
    fn footer(&self) -> Html {
        let columns: &[(&str, &[&str])] = match self.skin {
            "amazon" => &[
                ("Get to Know Us", &["Careers", "Blog", "About Amazon", "Investor Relations"]),
                ("Make Money with Us", &["Sell products on Amazon", "Become an Affiliate", "Advertise Your Products"]),
                ("Amazon Payment Products", &["Amazon Business Card", "Shop with Points", "Reload Your Balance"]),
            ],
            "ebay" => &[
                ("Buy", &["Registration", "Bidding & buying help", "Stores"]),
                ("Sell", &["Start selling", "How to sell", "Business sellers"]),
                ("About eBay", &["Company info", "News", "Investors", "Policies"]),
            ],
            "etsy" => &[
                ("Shop", &["Gift cards", "Etsy Registry", "Sitemap"]),
                ("Sell", &["Sell on Etsy", "Teams", "Forums"]),
                ("About", &["Etsy, Inc.", "Policies", "Careers", "Impact"]),
            ],
            "airbnb" => &[
                ("Support", &["Help Center", "AirCover", "Anti-discrimination", "Cancellation options"]),
                ("Hosting", &["Airbnb your home", "AirCover for Hosts", "Hosting resources"]),
                ("Airbnb", &["Newsroom", "New features", "Careers", "Investors"]),
            ],
            "booking" => &[
                ("Support", &["Coronavirus (COVID-19) FAQs", "Manage your trips", "Contact Customer Service"]),
                ("Discover", &["Genius loyalty program", "Seasonal and holiday deals", "Travel articles"]),
                ("Partners", &["Extranet login", "Partner help", "List your property"]),
            ],
            "uber" => &[
                ("Company", &["About us", "Our offerings", "Newsroom", "Investors"]),
                ("Products", &["Ride", "Drive", "Deliver", "Eat", "Uber for Business"]),
                ("Travel", &["Reserve", "Airports", "Cities"]),
            ],
            "doordash" => &[
                ("Get to Know Us", &["About Us", "Careers", "Investors", "Company Blog"]),
                ("Let Us Help You", &["Account Details", "Order History", "Help"]),
                ("Doing Business", &["Become a Dasher", "List Your Business", "Get Dashers for Deliveries"]),
            ],
            "ticketmaster" => &[
                ("Helpful Links", &["Help/FAQ", "Sell", "My Account", "Gift Cards"]),
                ("Our Network", &["Live Nation", "House of Blues", "Front Gate Tickets"]),
                ("About Us", &["Ticketmaster Blog", "Ticketing Truths", "Careers"]),
            ],
            _ => &[],
        };
        let (basket, basket_url) = self.basket();
        el("footer").class("ftr")
            .when(self.is("amazon"), |f| f.child(el("a").class("top").attr("href", "#chrome").text("Back to top")))
            .child(
                div("cols")
                    .each(columns.iter(), |(title, items)| {
                        div("col").child(el("h3").text(*title)).each(items.iter(), |item| span("fitem").text(*item))
                    })
                    .child(
                        div("col")
                            .child(el("h3").text(if self.is("amazon") { "Let Us Help You" } else { "Your account" }))
                            .child(self.here_aware(link("foot-home", "/", format!("{} home", self.s.brand)), "/"))
                            .when(!self.s.tickets(), |c| c.child(self.here_aware(link("foot-orders", "/orders", "Your orders"), "/orders")))
                            .child(self.here_aware(link("foot-basket", basket_url, basket), basket_url))
                            .child(self.here_aware(link("foot-favorites", "/favorites", "Favourites"), "/favorites")),
                    ),
            )
            .child(
                div("legal")
                    .child(span("brand").text(self.s.brand.as_str()))
                    .child(el("p").id("foot").text(
                        "Simulated storefront in a training world. No real order is ever placed and no real payment system is contacted.",
                    )),
            )
    }
    /// `here` is the path this page answers at, so the chrome can mark the link that leads
    /// back to it instead of offering a trip to nowhere.
    fn document(&self, title: &str, page_class: &str, query: &str, category: &str, here: &str, main: Vec<Html>) -> Result<HttpResponse> {
        let me = View { s: self.s, skin: self.skin, actor: self.actor, here: here.to_owned() };
        let t = &self.s.theme;
        let or = |v: &Option<String>, fallback: &str| v.clone().unwrap_or_else(|| fallback.to_owned());
        let doc = Document::new(title)
            .lang("en")
            .stylesheet(BASE)
            .stylesheet(skin_css(self.skin))
            .root_style(&format!(
                "--accent: {}; --ink: {}; --muted: {}; --surface: {}; --paper: {}; --content: {}px",
                or(&t.accent, "#146eb4"),
                or(&t.ink, "#0f1111"),
                or(&t.muted, "#565959"),
                or(&t.surface, "#eaeded"),
                or(&t.background, "#ffffff"),
                t.content_width.unwrap_or(1120).clamp(320, 1600)
            ))
            .body_class(&format!(
                "skin-{} page-{page_class}{}",
                self.skin,
                if self.s.tickets() { " mode-tickets" } else { "" }
            ))
            .body([
                me.header(query, page_class == "home"),
                me.cats(category),
                el("main").id("main").class("wrap").children(main),
                me.footer(),
            ]);
        page(&doc)
    }

    // ---- cards --------------------------------------------------------------------
    fn card(&self, product: &Product) -> Html {
        let id = product.id.as_str();
        let tickets = self.s.tickets();
        let date = date_parts(&product.event_date);
        let tenths = product.stars_tenths();
        let rating = if tickets {
            span("where").id(format!("v-{id}")).text(format!("{} · {}", product.venue_name, product.event_date))
        } else if product.rating_count == 0 {
            span("rating none").id(format!("r-{id}")).text("No reviews yet")
        } else {
            span("rating").id(format!("r-{id}")).attr("aria-label", product.rating_line())
                .child(span("rv").text(format!("{}.{}", tenths / 10, tenths % 10)))
                .child(stars(tenths))
                .child(span("ct").text(format!("({})", product.rating_count)))
        };
        let tags = span("tags").id(format!("tags-{id}"))
            .when(tickets, |t| t.child(span("from").text("From ")))
            .child(self.price(&format!("pr-{id}"), product.cheapest_cents()))
            .child(self.per())
            .when(product.fast_shipping, |t| {
                t.child(span("fast").id(format!("pf-{id}")).text(match self.skin {
                    "ebay" => "Free shipping",
                    "uber" | "doordash" => "Fast",
                    _ => "Two-day",
                }))
            })
            .when(!product.available(), |t| t.child(span("soldout").id(format!("so-{id}")).text("Sold out")));
        let booking = self.is("booking");
        let blurb = product.bullets.get(usize::from(booking)).or(product.bullets.first());
        let info = span("info")
            .child(span("name").id(format!("n-{id}")).text(product.title.as_str()))
            .when(!product.seller.is_empty(), |i| i.child(span("by").text(product.seller.as_str())))
            .child(rating)
            .maybe(blurb.map(|b| span("blurb").text(b.as_str())))
            .maybe(product.bullets.get(2).filter(|_| booking).map(|b| span(if b.starts_with("Free") { "perk free" } else { "perk" }).text(b.as_str())));
        let picture = pic(&format!("t-{id}"), &product.title, "")
            .when(self.is("airbnb") && tenths >= 48, |p| p.child(span("fave").text("Guest favorite")));
        let card = el("a").id(format!("p-{id}")).class("card").attr("href", self.product_url(id))
            .maybe(date.filter(|_| tickets).map(|(mon, day, year)| {
                span("date").child(span("mon").text(mon)).child(span("day").text(day)).child(span("year").text(year))
            }))
            .child(picture);
        if booking {
            // Booking's price sits on the right, under the review score, above the button.
            return card.child(info).child(
                span("side")
                    .maybe(score(product).map(|(t, word)| {
                        span("review")
                            .child(span("word").text(word))
                            .child(span("n").text(format!("{} reviews", product.rating_count)))
                            .child(span("badge").text(format!("{}.{}", t / 10, t % 10)))
                    }))
                    .child(span("nights").text("1 night, 2 adults"))
                    .child(tags)
                    .child(span("cta").text("See availability")),
            );
        }
        card.child(info.child(tags)).when(tickets, |a| a.child(span("cta").text("See Tickets")))
    }
    fn grid<'p>(&self, id: &str, products: impl IntoIterator<Item = &'p Product>) -> Html {
        div("grid").id(id).each(products, |p| self.card(p))
    }

    // ---- pages --------------------------------------------------------------------
    pub(crate) fn home(&self) -> Result<HttpResponse> {
        let s = self.s;
        let lead = match self.skin {
            _ if s.tickets() => "On sale now",
            "airbnb" => "Stays guests love",
            "booking" => "Homes and hotels guests love",
            "uber" => "Suggestions",
            "doordash" => "Fastest near you",
            "etsy" => "Popular right now",
            "ebay" => "Today's Deals",
            _ => "Today's picks",
        };
        let main = vec![
            if self.is("amazon") || self.is("etsy") || self.is("ebay") || self.is("doordash") || self.is("plain") {
                el("section").class("banner")
                    .child(el("h1").text(match self.skin {
                        "amazon" => "Spring deals are here",
                        "etsy" => "Find something you'll love, made by someone who cares",
                        "ebay" => "Things. People. Love.",
                        "doordash" => "Everything you crave, delivered",
                        _ => s.brand.as_str(),
                    }))
                    .child(el("p").text(s.tagline.as_str()))
            } else {
                empty()
            },
            el("h2").id("lead").class("lead").text(lead),
            self.grid("deals", {
                // A box office lists what is on next first; a store keeps catalogue order.
                let mut all: Vec<&Product> = s.products.values().collect();
                if s.tickets() {
                    all.sort_by(|a, b| a.event_tick.cmp(&b.event_tick).then_with(|| a.id.cmp(&b.id)));
                }
                all
            }),
        ];
        self.document(&s.brand, "home", "", "", "/", main)
    }
    pub(crate) fn results(&self, query: &str, category: &str) -> Result<HttpResponse> {
        let s = self.s;
        let hits = s.search(query, category);
        let title = match (query.is_empty(), category.is_empty()) {
            (true, false) => format!(
                "Browsing {}",
                s.categories.iter().find(|c| c.id == category).map_or(category, |c| c.title.as_str())
            ),
            (true, true) => "Everything".to_owned(),
            _ => format!("Results for \"{query}\""),
        };
        let mut params: Vec<(&str, &str)> = Vec::new();
        if !query.is_empty() {
            params.push(("k", query));
        }
        if !category.is_empty() {
            params.push(("c", category));
        }
        let here = href("/s", &params);
        let main = vec![
            el("h1").id("lead").class("lead").text(format!("{title} — {}", count(hits.len(), "item"))),
            if hits.is_empty() {
                el("p").id("empty").class("empty").text("Nothing matched. Try a broader word.")
            } else {
                self.grid("hits", hits.iter().copied())
            },
        ];
        self.document(&format!("{} — {}", title, s.brand), "results", query, category, &here, main)
    }
    pub(crate) fn favorites(&self) -> Result<HttpResponse> {
        let s = self.s;
        let list = s.favorites.get(self.actor).cloned().unwrap_or_default();
        let main = vec![
            el("h1").id("lead").class("lead").text("Favourites"),
            if list.is_empty() {
                el("p").id("empty").class("empty").text("Nothing saved yet.")
            } else {
                self.grid("favs", list.iter().filter_map(|id| s.products.get(id)))
            },
        ];
        self.document(&format!("Favourites — {}", s.brand), "favorites", "", "", "/favorites", main)
    }
    pub(crate) fn detail(&self, id: &str) -> Result<HttpResponse> {
        let s = self.s;
        let Some(product) = s.products.get(id) else {
            return web::error(404, "product not found");
        };
        let favorited = s.favorites.get(self.actor).is_some_and(|f| f.iter().any(|x| x == id));
        let tenths = product.stars_tenths();
        let in_stock = product.stock > 0;
        let (add_label, stock_label) = match self.skin {
            "airbnb" | "booking" => ("Reserve", format!("{} night{} open", product.stock, if product.stock == 1 { "" } else { "s" })),
            "uber" => ("Add to cart", format!("{} available", product.stock)),
            _ => ("Add to cart", format!("{} in stock", product.stock)),
        };
        let info = div("info")
            .child(el("h1").id("title").text(product.title.as_str()))
            .when(!product.seller.is_empty(), |i| i.child(span("by").text(product.seller.as_str())))
            .child(
                div("rating").id("rating")
                    .when(product.rating_count > 0, |r| r.child(stars(tenths)))
                    .child(span("rating-text").text(product.rating_line())),
            )
            .child(div("price-row").id("price-row").child(self.price("price", product.price_cents)).child(self.per()))
            .when(product.fast_shipping, |i| i.child(div("ship").child(span("fast").text("Two-day")).child(span("").text(" delivery at no extra cost"))))
            .child(el("h2").class("about").text(if self.stay() { "What this place offers" } else { "About this item" }))
            .child(el("ul").class("bullets").each(product.bullets.iter().enumerate(), |(i, b)| el("li").id(format!("bul-{i}")).text(b.as_str())));
        let buybox = el("aside").id("buybox").class("buybox")
            .child(div("buy-price").child(self.price("", product.price_cents)).child(self.per()))
            .child(div("stock").id("stock").class(if in_stock { "in" } else { "out" }).text(if in_stock { stock_label } else { "Sold out".to_owned() }))
            .when(in_stock, |b| {
                b.child(
                    form("add", "/api/cart", "post")
                        .child(hidden("product", id))
                        .child(label("add-qty", "Quantity"))
                        .child(text_input("add-qty", "qty", "1").attr("inputmode", "numeric"))
                        .child(button("add-go", add_label).class("primary")),
                )
            })
            .child(
                form("fav", format!("/api/products/{id}/favorite"), "post")
                    .child(button("fav-go", if favorited { "Remove favourite" } else { "Save to favourites" }).class("secondary")),
            )
            .when(!product.seller.is_empty() && !self.stay(), |b| {
                b.child(div("soldby").child(span("k").text("Ships from")).child(span("v").text(s.brand.as_str())).child(span("k").text("Sold by")).child(span("v").text(product.seller.as_str())))
            });
        let gallery = div("gallery").id("gallery")
            .child(div("strip").id("strip").each(1..4, |i| pic(&format!("shot-{i}"), &format!("View {i}"), "thumb")))
            .child(pic("shot", &product.title, "main"));
        let mut main = vec![
            div("crumbs").child(link("crumb-home", "/", s.brand.as_str())).child(span("sep").text("›")).child(
                link("crumb-cat", href("/s", &[("c", product.category.as_str())]), s.categories.iter().find(|c| c.id == product.category).map_or(product.category.as_str(), |c| c.title.as_str())),
            ),
            div("detail").id("hero").child(gallery).child(info).child(buybox),
            el("section").class("block").child(el("h2").id("desc-head").text(if self.stay() { "About this place" } else { "Product description" })).child(el("p").id("desc").text(product.description.as_str())),
        ];
        if !product.seller.is_empty() {
            main.push(
                el("section").class("block seller")
                    // The seeds of the stay and delivery skins already name the party the way
                    // their real site does ("Hosted by Marta (Superhost)", a restaurant name),
                    // so only the marketplaces prefix it.
                    .child(el("p").id("seller").text(if self.stay() || self.is("uber") || self.is("doordash") {
                        product.seller.clone()
                    } else {
                        format!("Sold by {}", product.seller)
                    }))
                    .child(
                        form("ask", "/api/messages", "post")
                            .child(hidden("product", id))
                            .child(label("ask-text", "Message the seller"))
                            .child(text_input("ask-text", "text", "").attr("placeholder", "Ask a question"))
                            .child(button("ask-go", "Send message").class("secondary")),
                    ),
            );
        }
        main.push(
            el("section").class("block reviews")
                .child(el("h2").id("rev-head").text(format!("{} review{}", product.reviews.len(), if product.reviews.len() == 1 { "" } else { "s" })))
                .each(&product.reviews, |r| {
                    let (a, _) = tones(&r.author);
                    el("article").id(format!("rev-{}", r.id)).class("review")
                        .child(div("who").child(span("avatar").style(&format!("background: {a}")).text(r.author.chars().next().unwrap_or('?').to_uppercase().to_string())).child(span("author").text(r.author.as_str())))
                        .child(div("head").id(format!("rev-{}-t", r.id)).child(stars(r.stars * 10)).child(span("sr").text(format!("{}★ ", r.stars))).child(el("b").text(r.title.as_str())))
                        .child(el("p").id(format!("rev-{}-b", r.id)).text(r.body.as_str()))
                        .child(el("p").class("meta").id(format!("rev-{}-a", r.id)).text(format!("{} · tick {}", r.author, r.tick)))
                })
                .child(
                    form("write", format!("/api/products/{id}/reviews"), "post").class("write")
                        .child(el("h3").text("Write a review"))
                        .child(label("write-stars", "Stars (1-5)"))
                        .child(text_input("write-stars", "stars", "5").attr("inputmode", "numeric"))
                        .child(label("write-title", "Headline"))
                        .child(text_input("write-title", "title", ""))
                        .child(label("write-body", "Your review"))
                        .child(text_input("write-body", "body", ""))
                        .child(button("write-go", "Post review").class("secondary")),
                ),
        );
        self.document(&format!("{} — {}", product.title, s.brand), "detail", "", &product.category, &format!("/dp/{id}"), main)
    }
    pub(crate) fn event(&self, id: &str) -> Result<HttpResponse> {
        let s = self.s;
        let Some(product) = s.products.get(id) else {
            return web::error(404, "event not found");
        };
        let mut main = vec![
            el("section").class("event-hero")
                .child(pic("stage", &product.venue_name, "stage"))
                .child(
                    div("event-info")
                        .child(el("h1").id("title").text(product.title.as_str()))
                        .child(el("p").id("when").text(format!("{} · {} · tick {}", product.event_date, product.venue_name, product.event_tick))),
                ),
            el("p").id("desc").class("event-desc").text(product.description.as_str()),
            el("h2").class("tiers-head").text("Tickets"),
        ];
        for t in &product.tiers {
            let tid = t.id.as_str();
            main.push(
                div("tier").id(format!("tier-{tid}")).child(
                    div("tier-row").id(format!("tier-{tid}-row"))
                        .child(div("tier-name").child(span("t").id(format!("tier-{tid}-t")).text(t.title.as_str())).child(span("sec").text(format!("Section {}", t.section))))
                        .child(span("left").id(format!("tier-{tid}-r")).text(format!("{} left", t.remaining)))
                        .child(span("price").id(format!("tier-{tid}-p")).text(money(t.price_cents)))
                        .when(t.remaining > 0, |row| {
                            row.child(
                                form(&format!("buy-{tid}"), "/api/checkout", "post")
                                    .child(hidden("event", id))
                                    .child(hidden("tier", tid))
                                    .child(label(&format!("buy-{tid}-qty"), "Tickets"))
                                    .child(text_input(&format!("buy-{tid}-qty"), "qty", "1").attr("inputmode", "numeric"))
                                    .child(button(&format!("buy-{tid}-go"), "Buy").class("primary")),
                            )
                        }),
                ),
            );
        }
        if !product.venue.is_empty() {
            main.push(
                link("venue-map", format!("http://maps.google.com/maps/place/{}", product.venue), format!("Directions to {}", product.venue_name)).class("venue-map"),
            );
        }
        self.document(&format!("{} — {}", product.title, s.brand), "event", "", &product.category, &format!("/event/{id}"), main)
    }
    pub(crate) fn cart(&self) -> Result<HttpResponse> {
        let s = self.s;
        let cart = s.cart(self.actor);
        let count: u64 = cart.values().sum();
        let lines = div("lines")
            .child(el("h1").id("lead").class("lead").text(if self.is("doordash") || self.is("uber") { "Your cart" } else { "Shopping cart" }))
            .when(cart.is_empty(), |l| l.child(el("p").id("empty").class("empty").text("Your cart is empty.")))
            .each(cart.iter().filter_map(|(id, qty)| s.products.get(id).map(|p| (id, qty, p))), |(id, qty, product)| {
                div("line").id(format!("line-{id}")).child(
                    div("line-row").id(format!("line-{id}-row"))
                        .child(pic(&format!("line-{id}-t"), &product.title, "thumb"))
                        .child(
                            div("line-body")
                                .child(el("a").class("name").id(format!("line-{id}-n")).attr("href", self.product_url(id)).text(product.title.as_str()))
                                .child(if product.stock == 0 {
                                    span("outstock").text("Out of stock")
                                } else if product.stock < *qty {
                                    // The catalogue moved under the line: say so here, because
                                    // /api/checkout is all-or-nothing and would refuse the lot.
                                    span("outstock").text(format!("Only {} left", product.stock))
                                } else {
                                    span("instock").text("In stock")
                                })
                                .child(
                                    div("line-actions")
                                        .child(
                                            form(&format!("qty-{id}"), "/api/cart", "post")
                                                .child(hidden("product", id))
                                                .child(label(&format!("qty-{id}-n"), "Qty"))
                                                .child(text_input(&format!("qty-{id}-n"), "qty", &qty.to_string()).attr("inputmode", "numeric"))
                                                .child(button(&format!("qty-{id}-go"), "Update").class("small")),
                                        )
                                        .child(
                                            form(&format!("rm-{id}"), "/api/cart", "post")
                                                .child(hidden("product", id))
                                                .child(hidden("qty", "0"))
                                                .child(button(&format!("rm-{id}-go"), "Remove").class("textlink")),
                                        ),
                                ),
                        )
                        .child(span("price").id(format!("line-{id}-p")).text(money(product.price_cents * qty))),
                )
            });
        let summary = el("aside").class("summary")
            .child(div("subtotal").id("subtotal").child(span("k").text(format!("Subtotal ({count} item{}): ", if count == 1 { "" } else { "s" }))).child(el("b").text(money(s.cart_total(self.actor)))))
            .when(!cart.is_empty(), |a| {
                a.child(form("checkout", "/api/checkout", "post").child(button("checkout-go", "Place your order").class("primary")))
            });
        self.document(&format!("Cart — {}", s.brand), "cart", "", "", self.basket().1, vec![div("cart-layout").child(lines).child(summary)])
    }
    fn when(o: &Order) -> String {
        if o.date.is_empty() {
            format!("tick {}", o.tick)
        } else {
            o.date.clone()
        }
    }
    pub(crate) fn orders(&self) -> Result<HttpResponse> {
        let s = self.s;
        let orders = s.orders_of(self.actor);
        let main = vec![
            el("h1").id("lead").class("lead").text(if s.tickets() { "My tickets" } else { "Your orders" }),
            if orders.is_empty() { el("p").id("empty").class("empty").text("No orders yet.") } else { empty() },
            div("orders").each(orders, |o| {
                let id = o.id.as_str();
                let titles = o.items.iter().map(|i| i.title.as_str()).collect::<Vec<_>>().join(", ");
                el("a").id(format!("o-{id}")).class("order").attr("href", format!("/orders/{id}"))
                    .child(
                        span("order-head")
                            .child(span("cell").child(span("k").text("Order placed")).child(span("v").text(Self::when(o))))
                            .child(span("cell").child(span("k").text("Total")).child(span("v price").id(format!("o-{id}-p")).text(money(o.total_cents))))
                            .child(span("cell grow").child(span("k").text("Ship to")).child(span("v").text(o.buyer.as_str())))
                            .child(span("cell num").id(format!("o-{id}-id")).text(format!("Order {id}"))),
                    )
                    .child(
                        span("order-body").id(format!("o-{id}-row"))
                            .child(pic(&format!("o-{id}-pic"), o.items.first().map_or("", |i| i.title.as_str()), "thumb"))
                            .child(span("what").child(span("status").id(format!("o-{id}-s")).text(o.status.as_str())).child(span("titles").id(format!("o-{id}-t")).text(titles)))
                            .child(span("cta").text(if s.tickets() { "View tickets" } else { "View order details" })),
                    )
            }),
        ];
        self.document(&format!("Orders — {}", s.brand), "orders", "", "", if s.tickets() { "/my-tickets" } else { "/orders" }, main)
    }
    pub(crate) fn order(&self, id: &str) -> Result<HttpResponse> {
        let s = self.s;
        let Some(o) = s.orders.get(id).filter(|o| o.buyer == self.actor) else {
            return web::error(404, "order not found");
        };
        let main = vec![
            div("crumbs").child(link("crumb-orders", "/orders", if s.tickets() { "My tickets" } else { "Your orders" })).child(span("sep").text("›")).child(span("here").text(format!("Order {}", o.id))),
            el("h1").id("lead").class("lead").text(format!("Order {}", o.id)),
            el("p").id("meta").class("meta").text(format!("{} · confirmation {} · {}", Self::when(o), o.confirmation, o.status)),
            div("receipt")
                .each(o.items.iter().enumerate(), |(i, item)| {
                    div("item").id(format!("it-{i}")).child(
                        div("item-row").id(format!("it-{i}-row"))
                            .child(pic(&format!("it-{i}-pic"), &item.title, "thumb"))
                            .child(span("name").id(format!("it-{i}-n")).text(item.title.as_str()))
                            .child(span("qty").id(format!("it-{i}-q")).text(format!("x{}", item.qty)))
                            .child(span("price").id(format!("it-{i}-p")).text(money(item.price_cents * item.qty))),
                    )
                })
                .when(!o.seats.is_empty(), |r| {
                    r.child(div("seats").id("seats").each(o.seats.iter().enumerate(), |(i, seat)| span("seat").id(format!("seat-{i}")).text(format!("Seat {seat}"))))
                })
                .child(el("hr").id("total-rule"))
                .child(div("total").id("total").text(format!("Order total: {}", money(o.total_cents)))),
            div("again").each(o.items.iter().enumerate().filter(|(_, item)| s.products.contains_key(&item.product)), |(i, item)| {
                link(&format!("again-{i}"), self.product_url(&item.product), format!("View {}", item.title)).class("again-link")
            }),
        ];
        self.document(&format!("Order {} — {}", o.id, s.brand), "order", "", "", &format!("/orders/{id}"), main)
    }
}
