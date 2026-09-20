//! Page rendering as HTML. One set of routes, two looks: `northwind` is a retail bank's
//! online banking (a blue masthead, account tiles, a ledger table with right-aligned
//! amounts, transfer forms), `paypal` is a wallet (the navy header, a balance card, an
//! activity list with round merchant marks, the send and request flow). The markup is
//! shared; `northwind.css` and `paypal.css` next to this file decide what it looks like,
//! and the seeded palette rides on `<html>` as custom properties.
//!
//! Element ids are the agent API and are the ones the `Page` version used: `chrome`,
//! `wordmark`, `nav-home`, `nav-pay`, `lead`, `none`, `accounts`, `a-<account>` (the whole
//! tile is one link, with `-n`, `-b`, `-av` inside), `act-head`, `t-<tx>` (the whole row is
//! one link, with `-m`, `-c`, `-d`, `-a` inside), `totals`, `bal`, `avail`, `statement`,
//! `filter` (`filter-category`, `filter-q`, `filter-go`), `cats`, `cat-<slug>`, `count`,
//! `merchant`, `amount`, `facts`, `fact-date`, `fact-cat`, `fact-memo`, `fact-status`,
//! `link-head`, `origin`, `back`, `sums`, `sum-in`, `sum-out`, `sum-close`, `own`,
//! `xfer-card`, `xfer-head`, `xfer` (`xfer-from`, `xfer-to`, `xfer-amount`, `xfer-go`),
//! `pay-card`, `pay-head`, `payees`, `pay` (`pay-account`, `pay-payee`, `pay-amount`,
//! `pay-go`), `addpayee` (`addpayee-name`, `addpayee-hint`, `addpayee-go`), `foot`, and
//! the statement's `stmt-periods` (`stmt-all`, `stmt-<yyyy-mm>`).
//!
//! Every control here leads somewhere: the quick actions on the overview land on the form
//! they name, the statement's period chips name only months this account has activity in,
//! and a tab that leads back to the page it is on says so with `aria-current="page"`. The
//! footer's Security / Privacy / Terms of use / Accessibility words are plain text with no
//! pointer, no role and no id, because this world has no such pages to send anyone to.
use crate::{money, slug, Account, BankState, Transaction};
use cw_protocol::{HttpResponse, Result};
use cw_service_common as web;
use cw_service_common::html::{
    button, div, el, form, href, label, link, page, span, text_input, Document, Html,
};

const NORTHWIND_CSS: &str = include_str!("northwind.css");
const PAYPAL_CSS: &str = include_str!("paypal.css");

/// Everything a page shell needs, resolved once per request.
pub(crate) struct Chrome {
    skin: String,
    brand: String,
    actor: String,
    root_style: String,
    /// Where the wallet's "Activity" tab goes: the actor's first account.
    first_account: Option<String>,
}
impl Chrome {
    pub(crate) fn read(s: &BankState, actor: &str) -> Self {
        let skin = s.skin().to_owned();
        let paypal = skin == "paypal";
        let t = &s.theme;
        let or = |v: &Option<String>, fallback: &str| v.clone().unwrap_or_else(|| fallback.to_owned());
        let root_style = format!(
            "--accent: {}; --ink: {}; --muted: {}; --surface: {}; --paper: {}; --content: {}px",
            or(&t.accent, if paypal { "#0070ba" } else { "#117aca" }),
            or(&t.ink, if paypal { "#001c64" } else { "#1b1b1b" }),
            or(&t.muted, if paypal { "#5b6b7b" } else { "#5a5a5a" }),
            or(&t.surface, if paypal { "#f5f7fa" } else { "#f2f4f6" }),
            or(&t.background, "#ffffff"),
            t.content_width.unwrap_or(1000).clamp(320, 1400),
        );
        Chrome {
            skin,
            brand: s.brand.clone(),
            actor: actor.to_owned(),
            root_style,
            first_account: s.owned(actor).first().map(|a| a.id.clone()),
        }
    }
    fn paypal(&self) -> bool {
        self.skin == "paypal"
    }
    /// `alice` -> `Alice`: the name in the masthead and the greeting.
    fn person(&self) -> String {
        let mut chars = self.actor.chars();
        match chars.next() {
            Some(first) => first.to_ascii_uppercase().to_string() + chars.as_str(),
            None => "Guest".to_owned(),
        }
    }
    /// The masthead: the mark, the two routes every page reaches, and who is signed in.
    fn header(&self, current: &str) -> Html {
        let on = |tab: &str| if tab == current { "tab on" } else { "tab" };
        // The current tab still links to itself, the way every real tab strip does, and
        // says so, so the agent is told where it is standing rather than sent in a circle.
        let here = |tab: &'static str| move |n: Html| {
            if tab == current { n.attr("aria-current", "page") } else { n }
        };
        let (home, pay) = if self.paypal() {
            ("Home", "Send and Request")
        } else {
            ("Accounts", "Pay & transfer")
        };
        let nav = el("nav")
            .class("tabs")
            .attr("aria-label", "Main")
            .child(here("home")(link("nav-home", "/", home).class(on("home"))))
            .child(here("pay")(link("nav-pay", "/transfers", pay).class(on("pay"))))
            .maybe(self.first_account.as_ref().map(|id| {
                here("activity")(
                    link("nav-activity", format!("/accounts/{id}"), "Activity").class(on("activity")),
                )
            }));
        let person = self.person();
        el("header").id("chrome").class("mast").child(
            div("wrap")
                .child(
                    div("brand")
                        .child(span("glyph").attr("aria-hidden", "true").child(el("i")))
                        .child(span("name").id("wordmark").text(self.brand.as_str())),
                )
                .child(nav)
                .child(
                    div("who")
                        .id("who")
                        .child(span("hello").text(person.as_str()))
                        .child(
                            span("avatar")
                                .attr("aria-hidden", "true")
                                .text(person.chars().next().unwrap_or('G').to_string()),
                        ),
                ),
        )
    }
    fn footer(&self) -> Html {
        el("footer").class("foot").child(
            div("wrap")
                .child(
                    div("cols")
                        .child(span("").text("Security"))
                        .child(span("").text("Privacy"))
                        .child(span("").text("Terms of use"))
                        .child(span("").text("Accessibility")),
                )
                .child(el("p").id("foot").text(
                    "Simulated bank in a training world. Balances, card numbers and payees are \
                     invented and no real payment network is contacted.",
                )),
        )
    }
    fn document(&self, title: &str, page_class: &str, current: &str, main: Vec<Html>) -> Result<HttpResponse> {
        let doc = Document::new(title)
            .lang("en")
            .stylesheet(if self.paypal() { PAYPAL_CSS } else { NORTHWIND_CSS })
            .root_style(&self.root_style)
            .body_class(&format!("skin-{} {page_class}", self.skin))
            .body([
                self.header(current),
                el("main").class("wrap").children(main),
                self.footer(),
            ]);
        page(&doc)
    }
}

const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
/// `2026-03-02` -> `Mar 2, 2026`; anything else is shown as the seed wrote it.
fn pretty_date(date: &str) -> String {
    let parts: Vec<&str> = date.split('-').collect();
    if let [y, m, d] = parts.as_slice() {
        if let (Ok(y), Ok(m), Ok(d)) = (y.parse::<u32>(), m.parse::<usize>(), d.parse::<u32>()) {
            if (1..=12).contains(&m) {
                return format!("{} {d}, {y}", MONTHS[m - 1]);
            }
        }
    }
    date.to_owned()
}
/// `2026-03` -> `March 2026`; anything else is shown as it was asked for.
fn pretty_month(period: &str) -> String {
    match period.split_once('-') {
        Some((y, m)) => match m.parse::<usize>() {
            Ok(m) if (1..=12).contains(&m) => format!("{} {y}", MONTHS[m - 1]),
            _ => period.to_owned(),
        },
        None => period.to_owned(),
    }
}
/// What the heading calls the period it is showing.
fn pretty_period(period: &str) -> String {
    if period == "all" {
        return "all activity".to_owned();
    }
    pretty_month(period)
}
fn when(t: &Transaction) -> Html {
    if t.date.is_empty() {
        el("time").text(format!("tick {}", t.tick))
    } else {
        el("time").attr("datetime", t.date.as_str()).text(pretty_date(&t.date))
    }
}
fn sign(cents: i64) -> &'static str {
    if cents < 0 {
        "neg"
    } else {
        "pos"
    }
}
/// A flat tint for a merchant's round mark, stable across renders.
fn tint(label: &str) -> &'static str {
    const PALETTE: [&str; 8] = ["#0070ba", "#1f8a5b", "#c2603a", "#7a4fb8", "#00a2c7", "#b8536b", "#5f7d2e", "#9a6b1f"];
    let mut h = 0xcbf2_9ce4_8422_2325_u64;
    for b in label.bytes() {
        h = (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3);
    }
    PALETTE[(h % 8) as usize]
}
/// One ledger line; the whole row is the link to the transaction.
fn tx_row(t: &Transaction) -> Html {
    let id = &t.id;
    let initial = t.merchant.chars().find(|c| c.is_alphanumeric()).unwrap_or('#').to_string();
    el("a")
        .id(format!("t-{id}"))
        .class("tx")
        .attr("href", format!("/accounts/{}/transactions/{id}", t.account))
        .child(
            span("ico")
                .attr("aria-hidden", "true")
                .style(&format!("background-color: {}", tint(&t.merchant)))
                .text(initial),
        )
        .child(when(t).id(format!("t-{id}-d")).class("d"))
        .child(
            span("m")
                .id(format!("t-{id}-m"))
                .text(t.merchant.as_str())
                .when(t.pending, |m| m.child(span("pend").id(format!("t-{id}-p")).text("Pending"))),
        )
        .child(span("c").id(format!("t-{id}-c")).text(if t.category.is_empty() {
            "Uncategorised"
        } else {
            t.category.as_str()
        }))
        .child(span(&format!("a {}", sign(t.amount_cents))).id(format!("t-{id}-a")).text(money(t.amount_cents)))
}
/// The ledger: a header line and the rows under it.
fn ledger<'a>(rows: impl IntoIterator<Item = &'a Transaction>) -> Html {
    div("ledger")
        .child(
            div("thead")
                .attr("aria-hidden", "true")
                .child(span("d").text("Date"))
                .child(span("m").text("Description"))
                .child(span("c").text("Category"))
                .child(span("a").text("Amount")),
        )
        .each(rows, tx_row)
}
fn kind_label(a: &Account) -> &'static str {
    match a.kind.as_str() {
        "credit" => "Credit",
        "savings" => "Savings",
        "checking" => "Checking",
        "balance" => "Balance",
        _ => "Account",
    }
}
fn tile(a: &Account) -> Html {
    let id = &a.id;
    el("a")
        .id(format!("a-{id}"))
        .class(&format!("tile kind-{}", slug(&a.kind)))
        .attr("href", format!("/accounts/{id}"))
        .child(
            span("head")
                .child(span("n").id(format!("a-{id}-n")).text(a.name.as_str()))
                .child(span("k").text(kind_label(a))),
        )
        .child(
            span("figures")
                .child(
                    span("fig")
                        .child(span("b").id(format!("a-{id}-b")).text(money(a.balance_cents)))
                        .child(span("cap").text(if a.credit() { "Current balance" } else { "Balance" })),
                )
                .child(span("fig av").id(format!("a-{id}-av")).text(format!(
                    "{} {}",
                    if a.credit() { "Available credit" } else { "Available" },
                    money(a.available_cents)
                ))),
        )
        .child(span("chev").attr("aria-hidden", "true"))
}

pub(crate) fn overview(s: &BankState, p: &Chrome, actor: &str) -> Result<HttpResponse> {
    let accounts = s.owned(actor);
    let total: i64 = accounts.iter().filter(|a| !a.credit()).map(|a| a.balance_cents).sum();
    let hero = el("section")
        .class("hero")
        .child(
            div("greet")
                .child(el("h1").id("lead").text("Your accounts"))
                .child(el("p").class("sub").id("welcome").text(format!("Welcome back, {}", p.person()))),
        )
        .child(
            div("worth")
                .child(span("cap").text("Total deposits"))
                .child(span("sum").id("worth").text(money(total))),
        );
    // Each quick action names a form on the transfer desk and lands on that form. The
    // wallet has no way to request money from anyone in this world, so it does not offer
    // one; its second action is the thing it can do, which is put a contact on file.
    let (send, pay) = if p.paypal() {
        (("Send", "/transfers#pay-card"), ("Add a contact", "/transfers#payee-card"))
    } else {
        (("Transfer money", "/transfers#xfer-card"), ("Pay a bill", "/transfers#pay-card"))
    };
    let round = |id: &str, class: &str, (text, href): (&str, &str)| {
        el("a")
            .id(id)
            .class(class)
            .attr("href", href)
            .child(span("disc").attr("aria-hidden", "true").child(el("i")))
            .child(span("t").text(text))
    };
    let actions = div("quick")
        .id("quick")
        .child(round("quick-send", "round send", send))
        .child(round("quick-pay", "round pay", pay))
        .each(accounts.iter().take(if p.paypal() { 0 } else { 3 }), |a| {
            el("a")
                .id(format!("quick-stmt-{}", a.id))
                .class("round stmt")
                .attr("href", format!("/statements/{}/all", a.id))
                .child(span("disc").attr("aria-hidden", "true").child(el("i")))
                .child(span("t").text(format!("Statement · {}", kind_label(a))))
        });
    let tiles = div("tiles")
        .id("accounts")
        .when(accounts.is_empty(), |d| {
            d.child(el("p").id("none").class("none").text("No accounts are open in your name."))
        })
        .each(accounts.iter(), |a| tile(a));
    let activity = el("section")
        .class("panel activity")
        .child(
            div("panel-head")
                .child(el("h2").id("act-head").text("Recent activity"))
                .maybe(
                    p.first_account
                        .as_ref()
                        .map(|id| link("act-all", format!("/accounts/{id}"), "Show all")),
                ),
        )
        .child(ledger(s.activity(actor, "").into_iter().take(8)));
    // The wallet shows activity beside the balance; the bank lists it under the tiles.
    let board = if p.paypal() {
        div("board")
            .child(div("col main-col").child(tiles).child(actions))
            .child(el("aside").class("col side-col").child(activity))
    } else {
        div("board")
            .child(div("col main-col").child(tiles).child(activity))
            .child(el("aside").class("col side-col").child(actions))
    };
    p.document(&s.brand, "page-home", "home", vec![hero, board])
}

pub(crate) fn account_page(
    s: &BankState,
    p: &Chrome,
    actor: &str,
    id: &str,
    category: &str,
    q: &str,
) -> Result<HttpResponse> {
    let a = match s.account(actor, id) {
        Ok(a) => a.clone(),
        Err(e) => return web::error(403, e),
    };
    let rows: Vec<&Transaction> = s
        .activity(actor, id)
        .into_iter()
        .filter(|t| BankState::matches(t, category, q))
        .collect();
    let categories = s.categories(actor);
    let summary = el("section")
        .class("summary")
        .child(
            div("title")
                .child(link("crumb-home", "/", if p.paypal() { "Home" } else { "Accounts" }).class("crumb"))
                .child(el("h1").id("lead").text(a.name.as_str())),
        )
        .child(
            div("totals")
                .id("totals")
                .child(
                    div("fig big")
                        .id("bal")
                        .child(span("cap").text("Balance "))
                        .child(span("v").text(money(a.balance_cents))),
                )
                .child(
                    div("fig")
                        .id("avail")
                        .child(span("cap").text(if a.credit() { "Available credit " } else { "Available " }))
                        .child(span("v").text(money(a.available_cents))),
                )
                .child(link("statement", format!("/statements/{id}/all"), "Statements").class("btn ghost"))
                .child(link("to-transfer", "/transfers", if p.paypal() { "Transfer Money" } else { "Transfer money" }).class("btn")),
        );
    let filter = form("filter", format!("/accounts/{id}"), "get")
        .class("filter")
        .child(
            div("field")
                .child(label("filter-category", "Category"))
                .child(text_input("filter-category", "category", category).attr("placeholder", "All categories")),
        )
        .child(
            div("field grow")
                .child(label("filter-q", "Search merchant or memo"))
                .child(text_input("filter-q", "q", q).attr("placeholder", "Search")),
        )
        .child(button("filter-go", "Filter").class("btn"));
    let cats = if categories.is_empty() {
        None
    } else {
        Some(el("nav").id("cats").class("chips").attr("aria-label", "Categories").each(categories.iter(), |c| {
            let on = c.eq_ignore_ascii_case(category);
            link(&format!("cat-{}", slug(c)), href(&format!("/accounts/{id}"), &[("category", c)]), c.as_str())
                .class(if on { "chip on" } else { "chip" })
                .when(on, |n| n.attr("aria-current", "page"))
        }))
    };
    let panel = el("section")
        .class("panel")
        .child(
            div("panel-head")
                .child(el("h2").text("Transactions"))
                .child(el("p").id("count").class("count").text(match rows.len() {
                    1 => "1 transaction".to_owned(),
                    n => format!("{n} transactions"),
                })),
        )
        .child(filter)
        .maybe(cats)
        .child(ledger(rows));
    p.document(&format!("{} — {}", a.name, s.brand), "page-account", "activity", vec![summary, panel])
}

pub(crate) fn tx_page(s: &BankState, p: &Chrome, actor: &str, id: &str, tx: &str) -> Result<HttpResponse> {
    if s.account(actor, id).is_err() {
        return web::error(403, "account unavailable");
    }
    let Some(t) = s.transactions.get(tx).filter(|t| t.account == id) else {
        return web::error(404, "transaction not found");
    };
    let initial = t.merchant.chars().find(|c| c.is_alphanumeric()).unwrap_or('#').to_string();
    let fact = |id: &str, name: &str, value: Html| {
        div("fact").id(id).child(span("cap").text(format!("{name} "))).child(value)
    };
    let posted = if t.date.is_empty() { format!("at tick {}", t.tick) } else { pretty_date(&t.date) };
    let card = el("section")
        .class("receipt")
        .child(
            div("receipt-head")
                .child(
                    span("ico")
                        .attr("aria-hidden", "true")
                        .style(&format!("background-color: {}", tint(&t.merchant)))
                        .text(initial),
                )
                .child(el("h1").id("merchant").text(t.merchant.as_str()))
                .child(el("p").id("amount").class(&format!("amount {}", sign(t.amount_cents))).text(money(t.amount_cents))),
        )
        .child(
            div("facts")
                .id("facts")
                .child(fact("fact-date", "Posted", span("v").text(posted)))
                .child(fact("fact-cat", "Category", span("v").text(t.category.as_str())))
                .child(div("fact").child(span("cap").text("Memo ")).child(span("v").id("fact-memo").text(t.memo.as_str())))
                .child(
                    div("fact").child(span("cap").text("Status ")).child(
                        span(if t.pending { "status pending" } else { "status" })
                            .id("fact-status")
                            .text(if t.pending { "Pending" } else { "Posted" }),
                    ),
                ),
        )
        .when(!t.link.is_empty(), |c| {
            c.child(
                div("origin")
                    .child(el("h2").id("link-head").text("Where this charge came from"))
                    .child(link("origin", t.link.as_str(), t.link.as_str())),
            )
        })
        .child(link("back", format!("/accounts/{id}"), "Back to the account").class("btn ghost"));
    p.document(&format!("{} — {}", t.merchant, s.brand), "page-tx", "activity", vec![card])
}

pub(crate) fn statement(s: &BankState, p: &Chrome, actor: &str, id: &str, period: &str) -> Result<HttpResponse> {
    let a = match s.account(actor, id) {
        Ok(a) => a.clone(),
        Err(e) => return web::error(403, e),
    };
    let rows: Vec<&Transaction> = s
        .activity(actor, id)
        .into_iter()
        .filter(|t| period == "all" || t.date.starts_with(period))
        .collect();
    let (mut credits, mut debits) = (0i64, 0i64);
    for t in &rows {
        if t.amount_cents < 0 {
            debits += t.amount_cents;
        } else {
            credits += t.amount_cents;
        }
    }
    let sum = |id: &str, class: &str, name: &str, cents: i64| {
        div(&format!("fig {class}"))
            .id(id)
            .child(span("cap").text(format!("{name} ")))
            .child(span("v").text(money(cents)))
    };
    // The periods on offer are the months this account actually has activity in, newest
    // first, so the range control can never name a statement that would come back empty.
    let mut months: Vec<String> = s
        .activity(actor, id)
        .iter()
        .filter(|t| t.date.len() >= 7)
        .map(|t| t.date[..7].to_owned())
        .collect();
    months.sort();
    months.dedup();
    months.reverse();
    let tab = |value: &str, text: String| {
        let on = value == period;
        link(&format!("stmt-{value}"), format!("/statements/{id}/{value}"), text)
            .class(if on { "chip on" } else { "chip" })
            .when(on, |n| n.attr("aria-current", "page"))
    };
    let periods = el("nav")
        .id("stmt-periods")
        .class("chips")
        .attr("aria-label", "Statement period")
        .child(tab("all", "All activity".to_owned()))
        .each(months.iter(), |m| tab(m, pretty_month(m)));
    let head = el("section")
        .class("summary")
        .child(
            div("title")
                .child(link("crumb-account", format!("/accounts/{id}"), a.name.as_str()).class("crumb"))
                .child(el("h1").id("lead").text(format!("Statement — {} — {}", a.name, pretty_period(period)))),
        )
        .child(
            div("totals")
                .id("sums")
                .child(sum("sum-in", "pos", "Deposits", credits))
                .child(sum("sum-out", "", "Withdrawals", debits))
                .child(sum("sum-close", "big", "Closing balance", a.balance_cents)),
        );
    let panel = el("section")
        .class("panel")
        .child(div("panel-head").child(el("h2").text("Statement activity")))
        .child(periods)
        .child(match rows.is_empty() {
            true => el("p").id("stmt-none").class("none").text("Nothing was posted in this period."),
            false => ledger(rows),
        });
    p.document(&format!("Statement {period} — {}", s.brand), "page-statement", "activity", vec![head, panel])
}

/// A labelled text field: the label is the control's accessible name.
fn field(id: &str, name: &str, caption: &str, value: &str, hint: &str) -> Html {
    div("field")
        .child(label(id, caption))
        .child(text_input(id, name, value).attr("placeholder", hint).attr("autocomplete", "off"))
}
pub(crate) fn transfers(s: &BankState, p: &Chrome, actor: &str) -> Result<HttpResponse> {
    let accounts = s.owned(actor);
    let first = accounts.first().map(|a| a.id.clone()).unwrap_or_default();
    let second = accounts.get(1).map(|a| a.id.clone()).unwrap_or_default();
    let own = el("ul")
        .id("own")
        .class("refs")
        .when(accounts.is_empty(), |n| {
            n.child(el("li").text("No accounts are open in your name."))
        })
        .each(accounts.iter(), |a| {
            el("li")
                .child(el("code").text(a.id.as_str()))
                .child(span("what").text(format!(" {} ", a.name)))
                .child(span("v").text(format!("({})", money(a.available_cents))))
        });
    let payees = el("ul").id("payees").class("refs").when(s.payees.is_empty(), |n| {
        n.child(el("li").text("Nobody is on file yet."))
    }).each(s.payees.values(), |x| {
        el("li")
            .child(el("code").text(x.id.as_str()))
            .child(span("what").text(format!(" ({}) ", x.name)))
            .child(span("v").text(x.account_hint.as_str()))
    });
    let xfer = el("section")
        .id("xfer-card")
        .class("panel formcard")
        .child(el("h2").id("xfer-head").text("Between your accounts"))
        .child(
            form("xfer", "/api/transfers", "post")
                .child(field("xfer-from", "from", "From account id", &first, "chk-0000"))
                .child(field("xfer-to", "to", "To account id", &second, "sav-0000"))
                .child(field("xfer-amount", "amount_cents", "Amount in cents", "2500", "0"))
                .child(button("xfer-go", "Transfer").class("btn")),
        );
    let pay = el("section")
        .id("pay-card")
        .class("panel formcard")
        .child(el("h2").id("pay-head").text(if p.paypal() { "Send a payment" } else { "Pay a bill" }))
        .child(
            form("pay", "/api/payments", "post")
                .child(field("pay-account", "account", "From account id", &first, "chk-0000"))
                .child(field("pay-payee", "payee", "Payee id", "", "payee id from the list"))
                .child(field("pay-amount", "amount_cents", "Amount in cents", "0", "0"))
                .child(button("pay-go", "Pay").class("btn")),
        );
    let add = el("section")
        .id("payee-card")
        .class("panel formcard")
        .child(el("h2").text(if p.paypal() { "Add a contact" } else { "Add a payee" }))
        .child(
            form("addpayee", "/api/payees", "post")
                .child(field("addpayee-name", "name", "New payee name", "", "Name"))
                .child(field("addpayee-hint", "account_hint", "Account hint", "", "...0000"))
                .child(button("addpayee-go", "Add payee").class("btn")),
        );
    let refs = el("aside")
        .class("panel refcard")
        .child(el("h2").text("Your accounts"))
        .child(own)
        .child(el("h2").class("second").text(if p.paypal() { "Contacts and merchants" } else { "Payees on file" }))
        .child(payees);
    let body = vec![
        el("section").class("summary").child(
            div("title")
                .child(link("crumb-home", "/", if p.paypal() { "Home" } else { "Accounts" }).class("crumb"))
                .child(el("h1").id("lead").text(if p.paypal() { "Send and Request" } else { "Pay & transfer" })),
        ),
        // The wallet leads with sending money; the bank with moving it between accounts.
        div("board")
            .child(if p.paypal() {
                div("col main-col").child(pay).child(xfer).child(add)
            } else {
                div("col main-col").child(xfer).child(pay).child(add)
            })
            .child(refs.class("col side-col")),
    ];
    p.document(&format!("Transfers — {}", s.brand), "page-transfers", "pay", body)
}
