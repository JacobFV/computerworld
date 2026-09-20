//! Retail banking: northwind.example. Balances are integer cents and a transfer writes both legs, so
//! the ledger can never disagree with itself. Only `ctx.actor`'s own accounts are addressable.
//!
//! Simulated money only: no real institution, card number or payment network is involved.
use cw_protocol::{HttpRequest, HttpResponse, PageTheme, Result};
use cw_sdk::{Registry, Service, ServiceContext};
use cw_service_common as web;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;
mod view;
use view::{account_page, overview, statement, transfers, tx_page, Chrome};
pub struct BankService;
pub fn register(registry: &mut Registry) -> Result<()> {
    registry.register(BankService)
}
/// Documented seed keys, checked by container type only — the crate that fills them owns the rest.
const OBJECTS: &[&str] = &["theme", "accounts", "transactions", "payees"];
const ARRAYS: &[&str] = &[];
/// Kinds that carry a revolving limit; for everything else available funds are the balance.
const CREDIT: &str = "credit";
/// The looks the service draws. An absent `skin` follows the brand (see [`BankState::skin`]).
const SKINS: &[&str] = &["", "northwind", "paypal"];
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct BankState {
    pub brand: String,
    pub tagline: String,
    /// `northwind` (a retail bank) or `paypal` (a wallet); when a seed names none it follows
    /// the brand, so the shipped seeds need no new key.
    pub skin: String,
    pub theme: PageTheme,
    pub accounts: BTreeMap<String, Account>,
    pub transactions: BTreeMap<String, Transaction>,
    pub payees: BTreeMap<String, Payee>,
    pub next_tx: u64,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Account {
    pub id: String,
    pub owner: String,
    pub kind: String,
    pub name: String,
    pub balance_cents: i64,
    /// Derived, never authored: the balance, or the remaining credit line on a card.
    pub available_cents: i64,
    pub limit_cents: i64,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Transaction {
    pub id: String,
    pub account: String,
    pub tick: u64,
    pub date: String,
    pub merchant: String,
    pub category: String,
    pub amount_cents: i64,
    pub memo: String,
    /// The page that caused the charge, rendered as a real outbound link on the detail view.
    pub link: String,
    pub pending: bool,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Payee {
    pub id: String,
    pub name: String,
    pub account_hint: String,
}
/// `-$1,234.56`: grouped the way a statement prints it.
fn money(cents: i64) -> String {
    let n = cents.unsigned_abs();
    let digits = (n / 100).to_string();
    let mut whole = String::new();
    for (i, d) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            whole.push(',');
        }
        whole.push(d);
    }
    format!("{}${whole}.{:02}", if cents < 0 { "-" } else { "" }, n % 100)
}
fn slug(name: &str) -> String {
    let s: String = name
        .to_ascii_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    s.split('-')
        .filter(|p| !p.is_empty())
        .collect::<Vec<_>>()
        .join("-")
}
impl Account {
    fn credit(&self) -> bool {
        self.kind == CREDIT
    }
    /// Available funds are a function of the balance, so no write can leave the two disagreeing.
    fn resync(&mut self) {
        self.available_cents = if self.credit() {
            self.limit_cents + self.balance_cents
        } else {
            self.balance_cents
        };
    }
}
impl BankState {
    /// The skin a page is drawn in: the seeded one, or the wallet look for a PayPal brand
    /// and the retail bank for everything else.
    pub fn skin(&self) -> &str {
        match self.skin.as_str() {
            "" if self.brand.to_ascii_lowercase().contains("paypal") => "paypal",
            "" => "northwind",
            named => named,
        }
    }
    pub fn account(&self, actor: &str, id: &str) -> std::result::Result<&Account, String> {
        match self.accounts.get(id) {
            Some(a) if a.owner == actor => Ok(a),
            _ => Err("account unavailable".into()),
        }
    }
    fn owned(&self, actor: &str) -> Vec<&Account> {
        self.accounts
            .values()
            .filter(|a| a.owner == actor)
            .collect()
    }
    /// Newest first; the id tiebreak keeps the order total when two land on the same tick.
    pub fn activity(&self, actor: &str, account: &str) -> Vec<&Transaction> {
        let mut v: Vec<&Transaction> = self
            .transactions
            .values()
            .filter(|t| {
                (account.is_empty() || t.account == account)
                    && self.account(actor, &t.account).is_ok()
            })
            .collect();
        v.sort_by(|a, b| b.tick.cmp(&a.tick).then_with(|| b.id.cmp(&a.id)));
        v
    }
    fn matches(t: &Transaction, category: &str, q: &str) -> bool {
        let needle = q.trim().to_ascii_lowercase();
        (category.is_empty() || t.category.eq_ignore_ascii_case(category))
            && (needle.is_empty()
                || t.merchant.to_ascii_lowercase().contains(&needle)
                || t.memo.to_ascii_lowercase().contains(&needle))
    }
    fn categories(&self, actor: &str) -> Vec<String> {
        let mut v: Vec<String> = self
            .activity(actor, "")
            .iter()
            .map(|t| t.category.clone())
            .filter(|c| !c.is_empty())
            .collect();
        v.sort();
        v.dedup();
        v
    }
    fn take_tx_id(&mut self) -> String {
        if self.next_tx == 0 {
            self.next_tx = 1;
        }
        let mut id = format!("tx-{}", self.next_tx);
        while self.transactions.contains_key(&id) {
            self.next_tx += 1;
            id = format!("tx-{}", self.next_tx);
        }
        self.next_tx += 1;
        id
    }
    fn post_leg(
        &mut self,
        account: &str,
        amount: i64,
        merchant: &str,
        category: &str,
        memo: &str,
        tick: u64,
    ) -> String {
        let id = self.take_tx_id();
        if let Some(a) = self.accounts.get_mut(account) {
            a.balance_cents += amount;
            a.resync();
        }
        self.transactions.insert(
            id.clone(),
            Transaction {
                id: id.clone(),
                account: account.to_owned(),
                tick,
                date: String::new(),
                merchant: merchant.to_owned(),
                category: category.to_owned(),
                amount_cents: amount,
                memo: memo.to_owned(),
                link: String::new(),
                pending: false,
            },
        );
        id
    }
    /// Both legs or neither: every check runs before the first cent moves.
    pub fn transfer(
        &mut self,
        actor: &str,
        from: &str,
        to: &str,
        amount: i64,
        tick: u64,
    ) -> std::result::Result<Value, String> {
        if amount <= 0 {
            return Err("amount must be positive".into());
        }
        if from == to {
            return Err("choose two different accounts".into());
        }
        let source = self.account(actor, from)?.clone();
        let target = self.account(actor, to)?.clone();
        if source.available_cents < amount {
            return Err("insufficient available funds".into());
        }
        let debit = self.post_leg(
            from,
            -amount,
            &target.name,
            "Transfer",
            &format!("Transfer to {}", target.name),
            tick,
        );
        let credit = self.post_leg(
            to,
            amount,
            &source.name,
            "Transfer",
            &format!("Transfer from {}", source.name),
            tick,
        );
        Ok(json!({"debit": debit, "credit": credit, "amount_cents": amount}))
    }
    pub fn pay(
        &mut self,
        actor: &str,
        account: &str,
        payee: &str,
        amount: i64,
        tick: u64,
    ) -> std::result::Result<Value, String> {
        if amount <= 0 {
            return Err("amount must be positive".into());
        }
        let source = self.account(actor, account)?.clone();
        let p = self.payees.get(payee).ok_or("unknown payee")?.clone();
        if source.available_cents < amount {
            return Err("insufficient available funds".into());
        }
        let id = self.post_leg(
            account,
            -amount,
            &p.name,
            "Bills",
            &format!("Payment to {} {}", p.name, p.account_hint),
            tick,
        );
        Ok(json!({"transaction": id, "payee": p.id, "amount_cents": amount}))
    }
    pub fn add_payee(&mut self, name: &str, hint: &str) -> std::result::Result<Payee, String> {
        let id = slug(name);
        if id.is_empty() {
            return Err("payee name required".into());
        }
        if self.payees.contains_key(&id) {
            return Err("payee conflict: already on file".into());
        }
        let p = Payee {
            id: id.clone(),
            name: name.trim().to_owned(),
            account_hint: hint.trim().to_owned(),
        };
        self.payees.insert(id, p.clone());
        Ok(p)
    }
}
impl Service for BankService {
    fn kind(&self) -> &str {
        "bank"
    }
    fn initialize(&self, initial: Value, _: &ServiceContext) -> Result<Value> {
        let gated = web::shape(initial, OBJECTS, ARRAYS)?;
        web::theme(&gated)?;
        web::variant(&gated, "skin", SKINS)?;
        let mut s: BankState = web::load(&gated)?;
        if s.brand.is_empty() {
            s.brand = "Bank".into();
        }
        if s.next_tx == 0 {
            s.next_tx = 1;
        }
        // Derive available funds at load so a seed can never author an inconsistent pair.
        for a in s.accounts.values_mut() {
            a.resync();
        }
        Ok(serde_json::to_value(s)?)
    }
    fn handle(
        &self,
        state: &mut Value,
        c: &ServiceContext,
        r: &HttpRequest,
    ) -> Result<HttpResponse> {
        let mut s: BankState = web::load(state)?;
        let p = Chrome::read(&s, &c.actor);
        let path = web::path(r);
        let parts: Vec<&str> = path.trim_matches('/').split('/').collect();
        let method = r.method.to_ascii_uppercase();
        if method == "GET" {
            return match parts.as_slice() {
                [""] => overview(&s, &p, &c.actor),
                ["transfers"] => transfers(&s, &p, &c.actor),
                ["accounts", id] => account_page(
                    &s,
                    &p,
                    &c.actor,
                    id,
                    &web::query(r, "category").unwrap_or_default(),
                    &web::query(r, "q").unwrap_or_default(),
                ),
                ["accounts", id, "transactions", tx] => tx_page(&s, &p, &c.actor, id, tx),
                ["statements", id, period] => statement(&s, &p, &c.actor, id, period),
                ["api", "accounts"] => HttpResponse::json(200, &s.owned(&c.actor)),
                ["api", "accounts", id] => web::domain(s.account(&c.actor, id).map(|a| json!(a))),
                ["api", "accounts", id, "transactions"] => match s.account(&c.actor, id) {
                    Ok(_) => HttpResponse::json(200, &s.activity(&c.actor, id)),
                    Err(e) => web::error(403, e),
                },
                ["api", "payees"] => HttpResponse::json(200, &s.payees),
                _ => web::error(404, "route not found"),
            };
        }
        if method != "POST" {
            return web::error(405, "method not allowed");
        }
        let b = web::body(r)?;
        let text = |k: &str| web::text(&b, k);
        let cents = |k: &str| -> i64 {
            b.get(k)
                .and_then(|v| v.as_i64().or_else(|| v.as_str()?.trim().parse().ok()))
                .unwrap_or(0)
        };
        let outcome: std::result::Result<(Value, String), String> = match parts.as_slice() {
            ["api", "transfers"] => s
                .transfer(
                    &c.actor,
                    &text("from"),
                    &text("to"),
                    cents("amount_cents"),
                    c.tick,
                )
                .map(|v| {
                    let url = format!("/accounts/{}", text("from"));
                    (v, url)
                }),
            ["api", "payments"] => s
                .pay(
                    &c.actor,
                    &text("account"),
                    &text("payee"),
                    cents("amount_cents"),
                    c.tick,
                )
                .map(|v| {
                    let url = format!("/accounts/{}", text("account"));
                    (v, url)
                }),
            ["api", "payees"] => s
                .add_payee(&text("name"), &text("account_hint"))
                .map(|v| (json!(v), "/transfers".to_owned())),
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
        let parts: Vec<&str> = next.trim_matches('/').split('/').collect();
        match parts.as_slice() {
            ["accounts", id] => account_page(&s, &p, &c.actor, id, "", ""),
            ["transfers"] => transfers(&s, &p, &c.actor),
            _ => HttpResponse::json(200, &value),
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use cw_service_common::html;
    fn ctx() -> ServiceContext {
        ServiceContext {
            actor: "alice".into(),
            source: "alice-mac".into(),
            tick: 12,
            seed: 1,
            instance: "bank".into(),
        }
    }
    fn seed() -> Value {
        json!({
            "brand": "Testbank", "next_tx": 2, "theme": {"accent": "#117aca"},
            "accounts": {
                "chk-4417": {"id": "chk-4417", "owner": "alice", "kind": "checking",
                             "name": "Everyday Checking (...4417)", "balance_cents": 812455},
                "sav-9902": {"id": "sav-9902", "owner": "alice", "kind": "savings",
                             "name": "Savings (...9902)", "balance_cents": 1450000},
                "cc-3310": {"id": "cc-3310", "owner": "alice", "kind": "credit",
                            "name": "Rewards Card (...3310)", "balance_cents": -48231,
                            "limit_cents": 1000000},
                "chk-7781": {"id": "chk-7781", "owner": "bob", "kind": "checking",
                             "name": "Bob Checking (...7781)", "balance_cents": 120000}
            },
            "transactions": {
                "tx-1": {"id": "tx-1", "account": "cc-3310", "tick": 6, "date": "2026-03-02",
                         "merchant": "AMAZON.COM", "category": "Shopping", "amount_cents": -42999,
                         "memo": "Order 1001", "link": "http://amazon.com/orders/1001"}
            },
            "payees": {"city-power": {"id": "city-power", "name": "Cascade City Power",
                                      "account_hint": "...8821"}}
        })
    }
    fn get(state: &mut Value, url: &str) -> HttpResponse {
        BankService
            .handle(state, &ctx(), &HttpRequest::get(url))
            .unwrap()
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
        BankService.handle(state, &ctx(), &r).unwrap()
    }
    fn text(r: &HttpResponse) -> String {
        String::from_utf8(r.body.clone()).unwrap()
    }
    #[test]
    fn seed_shape_is_gated_at_load() {
        assert!(BankService.initialize(json!([]), &ctx()).is_err());
        assert!(BankService
            .initialize(json!({"accounts": []}), &ctx())
            .is_err());
        assert!(BankService.initialize(Value::Null, &ctx()).is_ok());
    }
    #[test]
    fn available_credit_is_derived_from_the_balance_and_the_limit() {
        let state = BankService.initialize(seed(), &ctx()).unwrap();
        let s: BankState = web::load(&state).unwrap();
        assert_eq!(s.accounts["cc-3310"].available_cents, 951_769);
        assert_eq!(s.accounts["chk-4417"].available_cents, 812_455);
    }
    /// The response as a parsed document, after the strict validator has accepted it.
    fn dom(r: &HttpResponse) -> cw_web::dom::Document {
        assert_eq!(r.header("content-type"), Some(html::HTML_MEDIA_TYPE));
        let body = text(r);
        html::validate_strict(&body).unwrap_or_else(|e| panic!("strict validation: {e:?}"));
        cw_web::html::parse(&body)
    }
    fn node(doc: &cw_web::dom::Document, id: &str) -> cw_web::dom::NodeId {
        *doc.by_id(id).first().unwrap_or_else(|| panic!("no element #{id}"))
    }
    fn paypal_seed() -> Value {
        let mut seed = seed();
        seed["brand"] = json!("PayPal");
        seed["theme"] = json!({"accent": "#0070ba", "ink": "#001c64"});
        seed
    }
    const PAGES: [&str; 8] = [
        "/",
        "/transfers",
        "/accounts/chk-4417",
        "/accounts/cc-3310?category=Shopping",
        "/accounts/cc-3310?q=nothing-matches",
        "/accounts/cc-3310/transactions/tx-1",
        "/statements/cc-3310/2026-03",
        "/statements/cc-3310/all",
    ];
    #[test]
    fn pages_render_and_do_not_mutate() {
        let mut state = BankService.initialize(seed(), &ctx()).unwrap();
        let before = state.clone();
        for path in PAGES {
            let url = format!("http://northwind.example{path}");
            let page = get(&mut state, &url);
            assert_eq!(page.status, 200, "{url}");
            assert_eq!(page, get(&mut state, &url), "{url} must be pure");
        }
        assert_eq!(before, state, "rendering must not mutate seed state");
        let home = dom(&get(&mut state, "http://northwind.example/"));
        assert_eq!(home.text_content(node(&home, "a-chk-4417-b")), "$8,124.55");
        // Storyline 5: the charge points at the order that produced it.
        let tx = dom(&get(
            &mut state,
            "http://northwind.example/accounts/cc-3310/transactions/tx-1",
        ));
        assert_eq!(tx.attr(node(&tx, "origin"), "href"), Some("http://amazon.com/orders/1001"));
        assert_eq!(tx.text_content(node(&tx, "amount")), "-$429.99");
    }
    #[test]
    fn every_page_of_every_skin_passes_the_strict_validator() {
        for (seed, host, skin) in [
            (seed(), "northwind.example", "skin-northwind"),
            (paypal_seed(), "paypal.com", "skin-paypal"),
        ] {
            let mut state = BankService.initialize(seed, &ctx()).unwrap();
            for path in PAGES {
                let page = get(&mut state, &format!("http://{host}{path}"));
                assert_eq!(page.status, 200, "{host}{path}");
                let doc = dom(&page);
                let body = doc.descendants(cw_web::dom::Document::ROOT).find(|n| doc.is(*n, "body")).unwrap();
                assert!(doc.attr(body, "class").unwrap().contains(skin), "{host}{path}");
                for id in ["chrome", "wordmark", "nav-home", "nav-pay", "foot"] {
                    node(&doc, id);
                }
            }
            // Somebody with no accounts still gets a valid page.
            let bob = ServiceContext { actor: "carol".into(), ..ctx() };
            let empty = BankService
                .handle(&mut state, &bob, &HttpRequest::get(format!("http://{host}/")))
                .unwrap();
            let doc = dom(&empty);
            node(&doc, "none");
        }
        assert!(BankService.initialize(json!({"skin": "nonesuch"}), &ctx()).is_err());
        let named = BankService.initialize(json!({"brand": "Any", "skin": "paypal"}), &ctx()).unwrap();
        let s: BankState = web::load(&named).unwrap();
        assert_eq!(s.skin(), "paypal");
    }
    #[test]
    fn ids_links_and_forms_are_the_ones_the_page_version_had() {
        let mut state = BankService.initialize(seed(), &ctx()).unwrap();
        let home = dom(&get(&mut state, "http://northwind.example/"));
        assert_eq!(home.attr(node(&home, "nav-home"), "href"), Some("/"));
        assert_eq!(home.attr(node(&home, "nav-pay"), "href"), Some("/transfers"));
        assert_eq!(home.text_content(node(&home, "wordmark")), "Testbank");
        assert_eq!(home.text_content(node(&home, "lead")), "Your accounts");
        assert_eq!(home.text_content(node(&home, "act-head")), "Recent activity");
        let tile = node(&home, "a-cc-3310");
        assert_eq!(home.tag(tile), Some("a"));
        assert_eq!(home.attr(tile, "href"), Some("/accounts/cc-3310"));
        assert_eq!(home.text_content(node(&home, "a-cc-3310-n")), "Rewards Card (...3310)");
        assert_eq!(home.text_content(node(&home, "a-cc-3310-b")), "-$482.31");
        assert_eq!(home.text_content(node(&home, "a-cc-3310-av")), "Available credit $9,517.69");
        assert_eq!(home.text_content(node(&home, "a-chk-4417-av")), "Available $8,124.55");
        let row = node(&home, "t-tx-1");
        assert_eq!(home.tag(row), Some("a"));
        assert_eq!(home.attr(row, "href"), Some("/accounts/cc-3310/transactions/tx-1"));
        assert_eq!(home.text_content(node(&home, "t-tx-1-m")), "AMAZON.COM");
        assert_eq!(home.text_content(node(&home, "t-tx-1-c")), "Shopping");
        assert_eq!(home.text_content(node(&home, "t-tx-1-d")), "Mar 2, 2026");
        assert_eq!(home.attr(node(&home, "t-tx-1-d"), "datetime"), Some("2026-03-02"));
        assert_eq!(home.text_content(node(&home, "t-tx-1-a")), "-$429.99");

        let account = dom(&get(&mut state, "http://northwind.example/accounts/cc-3310?category=Shopping&q=order"));
        assert_eq!(account.text_content(node(&account, "lead")), "Rewards Card (...3310)");
        assert_eq!(account.text_content(node(&account, "bal")), "Balance -$482.31");
        assert_eq!(account.text_content(node(&account, "avail")), "Available credit $9,517.69");
        assert_eq!(account.attr(node(&account, "statement"), "href"), Some("/statements/cc-3310/all"));
        let filter = node(&account, "filter");
        assert_eq!(account.attr(filter, "action"), Some("/accounts/cc-3310"));
        assert_eq!(account.attr(filter, "method"), Some("get"));
        assert_eq!(account.attr(node(&account, "filter-category"), "name"), Some("category"));
        assert_eq!(account.attr(node(&account, "filter-category"), "value"), Some("Shopping"));
        assert_eq!(account.attr(node(&account, "filter-q"), "name"), Some("q"));
        assert_eq!(account.attr(node(&account, "filter-q"), "value"), Some("order"));
        assert_eq!(account.tag(node(&account, "filter-go")), Some("button"));
        assert_eq!(account.attr(node(&account, "cat-shopping"), "href"), Some("/accounts/cc-3310?category=Shopping"));
        assert_eq!(account.text_content(node(&account, "count")), "1 transaction");
        node(&account, "t-tx-1");
        let none = dom(&get(&mut state, "http://northwind.example/accounts/cc-3310?q=nothing-matches"));
        assert_eq!(none.text_content(node(&none, "count")), "0 transactions");
        assert!(none.by_id("t-tx-1").is_empty());

        let tx = dom(&get(&mut state, "http://northwind.example/accounts/cc-3310/transactions/tx-1"));
        assert_eq!(tx.text_content(node(&tx, "merchant")), "AMAZON.COM");
        assert_eq!(tx.text_content(node(&tx, "fact-date")), "Posted Mar 2, 2026");
        assert_eq!(tx.text_content(node(&tx, "fact-cat")), "Category Shopping");
        assert_eq!(tx.text_content(node(&tx, "fact-memo")), "Order 1001");
        assert_eq!(tx.text_content(node(&tx, "fact-status")), "Posted");
        assert_eq!(tx.text_content(node(&tx, "link-head")), "Where this charge came from");
        assert_eq!(tx.attr(node(&tx, "back"), "href"), Some("/accounts/cc-3310"));
        node(&tx, "facts");

        let st = dom(&get(&mut state, "http://northwind.example/statements/cc-3310/2026-03"));
        assert_eq!(st.text_content(node(&st, "sum-in")), "Deposits $0.00");
        assert_eq!(st.text_content(node(&st, "sum-out")), "Withdrawals -$429.99");
        assert_eq!(st.text_content(node(&st, "sum-close")), "Closing balance -$482.31");
        node(&st, "sums");
        // The period control offers only the months this account has activity in, and the
        // one being read says so instead of pretending there is somewhere else to go.
        assert_eq!(st.text_content(node(&st, "lead")), "Statement — Rewards Card (...3310) — Mar 2026");
        assert_eq!(st.attr(node(&st, "stmt-all"), "href"), Some("/statements/cc-3310/all"));
        assert_eq!(st.text_content(node(&st, "stmt-2026-03")), "Mar 2026");
        assert_eq!(st.attr(node(&st, "stmt-2026-03"), "aria-current"), Some("page"));
        assert_eq!(st.attr(node(&st, "stmt-all"), "aria-current"), None);
        assert!(st.by_id("stmt-2026-01").is_empty(), "a month with nothing in it is not offered");
        let all = dom(&get(&mut state, "http://northwind.example/statements/cc-3310/all"));
        assert_eq!(all.text_content(node(&all, "lead")), "Statement — Rewards Card (...3310) — all activity");
        assert_eq!(all.attr(node(&all, "stmt-all"), "aria-current"), Some("page"));

        let pay = dom(&get(&mut state, "http://northwind.example/transfers"));
        assert_eq!(pay.text_content(node(&pay, "lead")), "Pay & transfer");
        for (form, action, fields, go) in [
            ("xfer", "/api/transfers", vec![("xfer-from", "from", "cc-3310"), ("xfer-to", "to", "chk-4417"), ("xfer-amount", "amount_cents", "2500")], "xfer-go"),
            ("pay", "/api/payments", vec![("pay-account", "account", "cc-3310"), ("pay-payee", "payee", ""), ("pay-amount", "amount_cents", "0")], "pay-go"),
            ("addpayee", "/api/payees", vec![("addpayee-name", "name", ""), ("addpayee-hint", "account_hint", "")], "addpayee-go"),
        ] {
            let f = node(&pay, form);
            assert_eq!(pay.attr(f, "action"), Some(action));
            assert_eq!(pay.attr(f, "method"), Some("post"));
            for (id, name, value) in fields {
                let input = node(&pay, id);
                assert_eq!(pay.attr(input, "name"), Some(name), "{id}");
                assert_eq!(pay.attr(input, "value").unwrap_or(""), value, "{id}");
                assert!(pay.ancestors(input).any(|a| a == f), "{id} is inside #{form}");
            }
            let button = node(&pay, go);
            assert_eq!(pay.tag(button), Some("button"));
            assert!(pay.ancestors(button).any(|a| a == f));
        }
        assert!(pay.text_content(node(&pay, "own")).contains("chk-4417"));
        assert!(pay.text_content(node(&pay, "payees")).contains("city-power (Cascade City Power)"));
        for id in ["xfer-card", "xfer-head", "pay-card", "pay-head"] {
            node(&pay, id);
        }
    }
    #[test]
    fn a_submitted_form_answers_with_the_page_it_changed() {
        let mut state = BankService.initialize(seed(), &ctx()).unwrap();
        let moved = post(
            &mut state,
            "http://northwind.example/api/transfers",
            &[("from", "chk-4417"), ("to", "sav-9902"), ("amount_cents", "25000")],
        );
        let doc = dom(&moved);
        assert_eq!(doc.text_content(node(&doc, "bal")), "Balance $7,874.55");
        assert_eq!(doc.text_content(node(&doc, "count")), "1 transaction");
        let added = post(
            &mut state,
            "http://northwind.example/api/payees",
            &[("name", "Rainier Fibre"), ("account_hint", "...4402")],
        );
        let doc = dom(&added);
        assert!(doc.text_content(node(&doc, "payees")).contains("rainier-fibre"));
    }
    #[test]
    fn only_the_actors_own_accounts_are_addressable() {
        let mut state = BankService.initialize(seed(), &ctx()).unwrap();
        assert_eq!(
            get(&mut state, "http://northwind.example/accounts/chk-7781").status,
            403
        );
        assert_eq!(
            get(
                &mut state,
                "http://northwind.example/accounts/chk-7781/transactions/tx-1"
            )
            .status,
            403
        );
        assert!(!text(&get(&mut state, "http://northwind.example/")).contains("7781"));
        assert_eq!(
            post(
                &mut state,
                "http://northwind.example/api/transfers",
                &[
                    ("from", "chk-4417"),
                    ("to", "chk-7781"),
                    ("amount_cents", "100")
                ]
            )
            .status,
            403,
            "money cannot be pushed into somebody else's account"
        );
    }
    #[test]
    fn a_transfer_writes_both_legs_atomically() {
        let mut state = BankService.initialize(seed(), &ctx()).unwrap();
        assert_eq!(
            post(
                &mut state,
                "http://northwind.example/api/transfers",
                &[
                    ("from", "chk-4417"),
                    ("to", "sav-9902"),
                    ("amount_cents", "25000")
                ]
            )
            .status,
            200
        );
        let s: BankState = web::load(&state).unwrap();
        assert_eq!(s.accounts["chk-4417"].balance_cents, 787_455);
        assert_eq!(s.accounts["sav-9902"].balance_cents, 1_475_000);
        assert_eq!(s.accounts["chk-4417"].available_cents, 787_455);
        let legs: Vec<&Transaction> = s
            .transactions
            .values()
            .filter(|t| t.category == "Transfer")
            .collect();
        assert_eq!(legs.len(), 2);
        assert_eq!(legs.iter().map(|t| t.amount_cents).sum::<i64>(), 0);
        let round: BankState = serde_json::from_value(serde_json::to_value(&s).unwrap()).unwrap();
        assert_eq!(round, s);
    }
    #[test]
    fn transfers_are_refused_when_they_should_be() {
        let mut state = BankService.initialize(seed(), &ctx()).unwrap();
        let before = state.clone();
        for body in [
            vec![
                ("from", "chk-4417"),
                ("to", "sav-9902"),
                ("amount_cents", "0"),
            ],
            vec![
                ("from", "chk-4417"),
                ("to", "chk-4417"),
                ("amount_cents", "100"),
            ],
            vec![
                ("from", "chk-4417"),
                ("to", "sav-9902"),
                ("amount_cents", "99999999"),
            ],
            vec![
                ("from", "ghost"),
                ("to", "sav-9902"),
                ("amount_cents", "100"),
            ],
        ] {
            let r = post(&mut state, "http://northwind.example/api/transfers", &body);
            assert!(
                r.status == 400 || r.status == 403,
                "{body:?} -> {}",
                r.status
            );
        }
        assert_eq!(before, state, "a refused transfer moves nothing");
    }
    #[test]
    fn paying_a_payee_debits_once_and_new_payees_persist() {
        let mut state = BankService.initialize(seed(), &ctx()).unwrap();
        assert_eq!(
            post(
                &mut state,
                "http://northwind.example/api/payments",
                &[
                    ("account", "chk-4417"),
                    ("payee", "city-power"),
                    ("amount_cents", "9120")
                ]
            )
            .status,
            200
        );
        let s: BankState = web::load(&state).unwrap();
        assert_eq!(s.accounts["chk-4417"].balance_cents, 803_335);
        assert_eq!(
            s.transactions
                .values()
                .filter(|t| t.category == "Bills")
                .count(),
            1
        );
        assert_eq!(
            post(
                &mut state,
                "http://northwind.example/api/payments",
                &[
                    ("account", "chk-4417"),
                    ("payee", "nobody"),
                    ("amount_cents", "100")
                ]
            )
            .status,
            400
        );
        assert_eq!(
            post(
                &mut state,
                "http://northwind.example/api/payees",
                &[("name", "Rainier Fibre"), ("account_hint", "...4402")]
            )
            .status,
            200
        );
        let s: BankState = web::load(&state).unwrap();
        assert_eq!(s.payees["rainier-fibre"].name, "Rainier Fibre");
        assert_eq!(
            post(
                &mut state,
                "http://northwind.example/api/payees",
                &[("name", "Rainier Fibre"), ("account_hint", "...4402")]
            )
            .status,
            409,
            "a duplicate payee is a conflict"
        );
    }
    #[test]
    fn non_get_post_methods_are_rejected() {
        let mut state = BankService.initialize(seed(), &ctx()).unwrap();
        let mut r = HttpRequest::get("http://northwind.example/");
        r.method = "PUT".into();
        assert_eq!(
            BankService.handle(&mut state, &ctx(), &r).unwrap().status,
            405
        );
    }
}
