//! Retail banking: northwind.example. Balances are integer cents and a transfer writes both legs, so
//! the ledger can never disagree with itself. Only `ctx.actor`'s own accounts are addressable.
//!
//! Simulated money only: no real institution, card number or payment network is involved.
use cw_protocol::{HttpRequest, HttpResponse, PageAction, PageElement, PageTheme, Result};
use cw_sdk::{Registry, Service, ServiceContext};
use cw_service_common as web;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;
pub struct BankService;
pub fn register(registry: &mut Registry) -> Result<()> {
    registry.register(BankService)
}
/// Documented seed keys, checked by container type only — the crate that fills them owns the rest.
const OBJECTS: &[&str] = &["theme", "accounts", "transactions", "payees"];
const ARRAYS: &[&str] = &[];
/// Kinds that carry a revolving limit; for everything else available funds are the balance.
const CREDIT: &str = "credit";
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct BankState {
    pub brand: String,
    pub tagline: String,
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
fn money(cents: i64) -> String {
    let n = cents.unsigned_abs();
    format!(
        "{}${}.{:02}",
        if cents < 0 { "-" } else { "" },
        n / 100,
        n % 100
    )
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
struct Palette {
    accent: String,
    ink: String,
    muted: String,
    surface: String,
}
fn palette(t: &PageTheme) -> Palette {
    Palette {
        accent: t.accent.clone().unwrap_or_else(|| "#117aca".into()),
        ink: t.ink.clone().unwrap_or_else(|| "#1b1b1b".into()),
        muted: t.muted.clone().unwrap_or_else(|| "#5a5a5a".into()),
        surface: t.surface.clone().unwrap_or_else(|| "#f2f4f6".into()),
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
    });
    PageElement::Form {
        id: id.into(),
        action,
        children,
    }
}
fn chrome(s: &BankState, p: &Palette) -> Vec<PageElement> {
    vec![
        web::styled_row(
            "chrome",
            18,
            "center",
            web::style()
                .background(p.accent.clone())
                .padding(12)
                .radius(6),
            vec![
                web::styled(
                    "wordmark",
                    &s.brand,
                    web::style().size(22).bold().color("#ffffff").width(220),
                ),
                web::link("nav-home", "Accounts", "/"),
                web::link("nav-pay", "Pay & transfer", "/transfers"),
            ],
        ),
        web::spacer("chrome-gap", 14),
    ]
}
fn footer(p: &Palette) -> Vec<PageElement> {
    vec![
        web::spacer("foot-gap", 18),
        web::divider("foot-rule"),
        web::styled(
            "foot",
            "Simulated bank in a training world. Balances, card numbers and payees are invented \
             and no real payment network is contacted.",
            web::style().size(12).color(p.muted.clone()),
        ),
    ]
}
fn amount_badge(id: &str, cents: i64, p: &Palette) -> PageElement {
    web::badge(
        id,
        money(cents),
        web::style()
            .size(14)
            .bold()
            .color(if cents < 0 {
                p.ink.clone()
            } else {
                "#0b6b3a".into()
            })
            .background(p.surface.clone())
            .radius(4)
            .padding(6),
    )
}
fn tx_row(t: &Transaction, p: &Palette) -> PageElement {
    web::card_action(
        &format!("t-{}", t.id),
        web::style()
            .background("#ffffff")
            .border("#dfe3e8")
            .radius(6)
            .padding(10),
        web::visit(format!("/accounts/{}/transactions/{}", t.account, t.id)),
        vec![web::row(
            &format!("t-{}-row", t.id),
            12,
            "center",
            vec![
                web::styled(
                    &format!("t-{}-m", t.id),
                    &t.merchant,
                    web::style().size(15).medium().color(p.ink.clone()).flex(3),
                ),
                web::badge(
                    &format!("t-{}-c", t.id),
                    if t.category.is_empty() {
                        "Uncategorised"
                    } else {
                        &t.category
                    },
                    web::style()
                        .size(12)
                        .color(p.muted.clone())
                        .border("#dfe3e8")
                        .radius(4)
                        .padding(4),
                ),
                web::styled(
                    &format!("t-{}-d", t.id),
                    if t.date.is_empty() {
                        format!("tick {}", t.tick)
                    } else {
                        t.date.clone()
                    },
                    web::style().size(12).color(p.muted.clone()).width(120),
                ),
                amount_badge(&format!("t-{}-a", t.id), t.amount_cents, p),
            ],
        )],
    )
}
fn overview(s: &BankState, p: &Palette, actor: &str) -> Result<HttpResponse> {
    let mut e = chrome(s, p);
    let accounts = s.owned(actor);
    e.push(web::styled(
        "lead",
        "Your accounts",
        web::style().size(24).bold().color(p.ink.clone()),
    ));
    e.push(web::spacer("lead-gap", 10));
    if accounts.is_empty() {
        e.push(web::styled(
            "none",
            "No accounts are open in your name.",
            web::style().color(p.muted.clone()),
        ));
    }
    e.push(web::grid(
        "accounts",
        3,
        14,
        accounts
            .iter()
            .map(|a| {
                web::card_action(
                    &format!("a-{}", a.id),
                    web::style()
                        .background("#ffffff")
                        .border("#dfe3e8")
                        .radius(8)
                        .padding(14),
                    web::visit(format!("/accounts/{}", a.id)),
                    vec![
                        web::styled(
                            &format!("a-{}-n", a.id),
                            &a.name,
                            web::style().size(15).medium().color(p.ink.clone()),
                        ),
                        web::styled(
                            &format!("a-{}-b", a.id),
                            money(a.balance_cents),
                            web::style().size(26).bold().color(p.accent.clone()),
                        ),
                        web::styled(
                            &format!("a-{}-av", a.id),
                            format!(
                                "{} {}",
                                if a.credit() {
                                    "Available credit"
                                } else {
                                    "Available"
                                },
                                money(a.available_cents)
                            ),
                            web::style().size(12).color(p.muted.clone()),
                        ),
                    ],
                )
            })
            .collect(),
    ));
    e.push(web::spacer("act-gap", 18));
    e.push(web::styled(
        "act-head",
        "Recent activity",
        web::style().size(18).bold().color(p.ink.clone()),
    ));
    for t in s.activity(actor, "").into_iter().take(8) {
        e.push(tx_row(t, p));
    }
    e.extend(footer(p));
    web::themed_page(&s.brand, s.theme.clone(), e)
}
fn account_page(
    s: &BankState,
    p: &Palette,
    actor: &str,
    id: &str,
    category: &str,
    q: &str,
) -> Result<HttpResponse> {
    let a = match s.account(actor, id) {
        Ok(a) => a.clone(),
        Err(e) => return web::error(403, e),
    };
    let mut e = chrome(s, p);
    e.push(web::styled(
        "lead",
        &a.name,
        web::style().size(24).bold().color(p.ink.clone()),
    ));
    e.push(web::row(
        "totals",
        14,
        "center",
        vec![
            web::styled(
                "bal",
                format!("Balance {}", money(a.balance_cents)),
                web::style().size(18).bold().color(p.accent.clone()),
            ),
            web::styled(
                "avail",
                format!("Available {}", money(a.available_cents)),
                web::style().size(14).color(p.muted.clone()),
            ),
            web::link("statement", "Statements", format!("/statements/{id}/all")),
        ],
    ));
    e.push(web::spacer("filter-gap", 12));
    e.push(form_el(
        "filter",
        act(
            "GET",
            &format!("/accounts/{id}"),
            &[("category", "$filter-category"), ("q", "$filter-q")],
        ),
        vec![
            input("filter-category", "Category", category),
            input("filter-q", "Search merchant or memo", q),
        ],
        "Filter",
    ));
    if !s.categories(actor).is_empty() {
        e.push(web::row(
            "cats",
            8,
            "center",
            s.categories(actor)
                .iter()
                .map(|c| {
                    web::link(
                        &format!("cat-{}", slug(c)),
                        c,
                        format!("/accounts/{id}?category={c}"),
                    )
                })
                .collect(),
        ));
    }
    e.push(web::spacer("list-gap", 12));
    let rows: Vec<&Transaction> = s
        .activity(actor, id)
        .into_iter()
        .filter(|t| BankState::matches(t, category, q))
        .collect();
    e.push(web::styled(
        "count",
        format!("{} transaction(s)", rows.len()),
        web::style().size(13).color(p.muted.clone()),
    ));
    for t in rows {
        e.push(tx_row(t, p));
    }
    e.extend(footer(p));
    web::themed_page(&format!("{} — {}", a.name, s.brand), s.theme.clone(), e)
}
fn tx_page(s: &BankState, p: &Palette, actor: &str, id: &str, tx: &str) -> Result<HttpResponse> {
    if s.account(actor, id).is_err() {
        return web::error(403, "account unavailable");
    }
    let Some(t) = s.transactions.get(tx).filter(|t| t.account == id) else {
        return web::error(404, "transaction not found");
    };
    let mut e = chrome(s, p);
    e.push(web::styled(
        "merchant",
        &t.merchant,
        web::style().size(26).bold().color(p.ink.clone()),
    ));
    e.push(web::styled(
        "amount",
        money(t.amount_cents),
        web::style().size(32).bold().color(p.accent.clone()),
    ));
    e.push(web::card(
        "facts",
        web::style()
            .background("#ffffff")
            .border("#dfe3e8")
            .radius(8)
            .padding(14),
        vec![
            web::styled(
                "fact-date",
                format!(
                    "Posted {}",
                    if t.date.is_empty() {
                        format!("at tick {}", t.tick)
                    } else {
                        t.date.clone()
                    }
                ),
                web::style().size(14).color(p.ink.clone()),
            ),
            web::styled(
                "fact-cat",
                format!("Category {}", t.category),
                web::style().size(14).color(p.ink.clone()),
            ),
            web::styled(
                "fact-memo",
                &t.memo,
                web::style().size(14).color(p.muted.clone()),
            ),
            web::badge(
                "fact-status",
                if t.pending { "Pending" } else { "Posted" },
                web::style()
                    .size(12)
                    .color(p.muted.clone())
                    .border("#dfe3e8")
                    .radius(4)
                    .padding(4),
            ),
        ],
    ));
    if !t.link.is_empty() {
        e.push(web::styled(
            "link-head",
            "Where this charge came from",
            web::style().size(16).bold().color(p.ink.clone()),
        ));
        e.push(web::link("origin", &t.link, &t.link));
    }
    e.push(web::link(
        "back",
        "Back to the account",
        format!("/accounts/{id}"),
    ));
    e.extend(footer(p));
    web::themed_page(&format!("{} — {}", t.merchant, s.brand), s.theme.clone(), e)
}
fn statement(
    s: &BankState,
    p: &Palette,
    actor: &str,
    id: &str,
    period: &str,
) -> Result<HttpResponse> {
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
    let mut e = chrome(s, p);
    e.push(web::styled(
        "lead",
        format!("Statement — {} — {period}", a.name),
        web::style().size(22).bold().color(p.ink.clone()),
    ));
    e.push(web::row(
        "sums",
        18,
        "center",
        vec![
            web::styled(
                "sum-in",
                format!("Deposits {}", money(credits)),
                web::style().size(14).color("#0b6b3a"),
            ),
            web::styled(
                "sum-out",
                format!("Withdrawals {}", money(debits)),
                web::style().size(14).color(p.ink.clone()),
            ),
            web::styled(
                "sum-close",
                format!("Closing balance {}", money(a.balance_cents)),
                web::style().size(14).bold().color(p.accent.clone()),
            ),
        ],
    ));
    e.push(web::divider("sum-rule"));
    for t in rows {
        e.push(tx_row(t, p));
    }
    e.extend(footer(p));
    web::themed_page(
        &format!("Statement {period} — {}", s.brand),
        s.theme.clone(),
        e,
    )
}
fn transfers(s: &BankState, p: &Palette, actor: &str) -> Result<HttpResponse> {
    let accounts = s.owned(actor);
    let first = accounts.first().map(|a| a.id.clone()).unwrap_or_default();
    let second = accounts.get(1).map(|a| a.id.clone()).unwrap_or_default();
    let mut e = chrome(s, p);
    e.push(web::styled(
        "lead",
        "Pay & transfer",
        web::style().size(24).bold().color(p.ink.clone()),
    ));
    e.push(web::styled(
        "own",
        accounts
            .iter()
            .map(|a| format!("{} ({})", a.id, money(a.available_cents)))
            .collect::<Vec<_>>()
            .join(" · "),
        web::style().size(13).color(p.muted.clone()),
    ));
    e.push(web::card(
        "xfer-card",
        web::style()
            .background("#ffffff")
            .border("#dfe3e8")
            .radius(8)
            .padding(14),
        vec![
            web::styled(
                "xfer-head",
                "Between your accounts",
                web::style().size(16).bold().color(p.ink.clone()),
            ),
            form_el(
                "xfer",
                act(
                    "POST",
                    "/api/transfers",
                    &[
                        ("from", "$xfer-from"),
                        ("to", "$xfer-to"),
                        ("amount_cents", "$xfer-amount"),
                    ],
                ),
                vec![
                    input("xfer-from", "From account id", &first),
                    input("xfer-to", "To account id", &second),
                    input("xfer-amount", "Amount in cents", "2500"),
                ],
                "Transfer",
            ),
        ],
    ));
    e.push(web::spacer("pay-gap", 14));
    e.push(web::card(
        "pay-card",
        web::style()
            .background("#ffffff")
            .border("#dfe3e8")
            .radius(8)
            .padding(14),
        vec![
            web::styled(
                "pay-head",
                "Pay a bill",
                web::style().size(16).bold().color(p.ink.clone()),
            ),
            web::styled(
                "payees",
                s.payees
                    .values()
                    .map(|x| format!("{} ({})", x.id, x.name))
                    .collect::<Vec<_>>()
                    .join(" · "),
                web::style().size(13).color(p.muted.clone()),
            ),
            form_el(
                "pay",
                act(
                    "POST",
                    "/api/payments",
                    &[
                        ("account", "$pay-account"),
                        ("payee", "$pay-payee"),
                        ("amount_cents", "$pay-amount"),
                    ],
                ),
                vec![
                    input("pay-account", "From account id", &first),
                    input("pay-payee", "Payee id", ""),
                    input("pay-amount", "Amount in cents", "0"),
                ],
                "Pay",
            ),
        ],
    ));
    e.push(web::spacer("payee-gap", 14));
    e.push(form_el(
        "addpayee",
        act(
            "POST",
            "/api/payees",
            &[
                ("name", "$addpayee-name"),
                ("account_hint", "$addpayee-hint"),
            ],
        ),
        vec![
            input("addpayee-name", "New payee name", ""),
            input("addpayee-hint", "Account hint", ""),
        ],
        "Add payee",
    ));
    e.extend(footer(p));
    web::themed_page(&format!("Transfers — {}", s.brand), s.theme.clone(), e)
}
impl Service for BankService {
    fn kind(&self) -> &str {
        "bank"
    }
    fn initialize(&self, initial: Value, _: &ServiceContext) -> Result<Value> {
        let gated = web::shape(initial, OBJECTS, ARRAYS)?;
        web::theme(&gated)?;
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
        let p = palette(&s.theme);
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
    #[test]
    fn pages_render_and_do_not_mutate() {
        let mut state = BankService.initialize(seed(), &ctx()).unwrap();
        let before = state.clone();
        for url in [
            "http://northwind.example/",
            "http://northwind.example/transfers",
            "http://northwind.example/accounts/chk-4417",
            "http://northwind.example/accounts/cc-3310?category=Shopping",
            "http://northwind.example/accounts/cc-3310/transactions/tx-1",
            "http://northwind.example/statements/cc-3310/2026-03",
        ] {
            let page = get(&mut state, url);
            assert_eq!(page.status, 200, "{url}");
            assert_eq!(page, get(&mut state, url), "{url} must be pure");
        }
        assert_eq!(before, state, "rendering must not mutate seed state");
        assert!(text(&get(&mut state, "http://northwind.example/")).contains("8124.55"));
        // Storyline 5: the charge points at the order that produced it.
        assert!(text(&get(
            &mut state,
            "http://northwind.example/accounts/cc-3310/transactions/tx-1"
        ))
        .contains("http://amazon.com/orders/1001"));
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
