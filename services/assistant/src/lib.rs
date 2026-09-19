//! Deterministic chat assistants: chatgpt.com and claude.ai. There is no model behind them, only
//! a scored pattern table, so the same prompt at the same `attempt` always yields the same reply.
//!
//! Two tables answer a prompt. `intents` are generic canned explanations; `facts` are claims about
//! this world that carry a citation the agent can click and check. Facts outrank intents by a
//! fixed margin, so "what is the Atlas release code" is answered from the world, not from patter.
use cw_protocol::{
    HttpRequest, HttpResponse, PageAction, PageElement, PageTheme, Result as SimResult, Style,
};
use cw_sdk::{Registry, Service, ServiceContext};
use cw_service_common as web;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct AssistantState {
    pub brand: String,
    pub tagline: String,
    pub model_label: String,
    pub theme: PageTheme,
    pub greeting: String,
    pub suggestions: Vec<String>,
    /// Generic canned answers, scored by pattern overlap.
    pub intents: Vec<Intent>,
    /// Claims about this world. Each one must be true here and must cite where it is true.
    pub facts: Vec<Fact>,
    pub fallback: Vec<String>,
    pub conversations: BTreeMap<String, Conversation>,
    pub next_id: u64,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Intent {
    pub id: String,
    pub patterns: Vec<String>,
    pub replies: Vec<String>,
    pub citations: Vec<Citation>,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Fact {
    pub id: String,
    pub patterns: Vec<String>,
    pub answer: String,
    pub citations: Vec<Citation>,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Citation {
    pub label: String,
    pub url: String,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Conversation {
    pub id: String,
    pub owner: String,
    pub title: String,
    pub tick: u64,
    pub messages: Vec<Message>,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Message {
    /// `user` or `assistant`.
    pub role: String,
    pub text: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub citations: Vec<Citation>,
    /// Which table row answered, for the reader and for tests; empty means the fallback.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub intent_id: String,
    /// Regenerate counter; it is the only thing that moves a reply off its first choice.
    pub attempt: u64,
    pub tick: u64,
}
/// A citable world fact always beats a generic intent, whatever the pattern overlap says.
const FACT_BONUS: u64 = 1000;
const NO_ANSWER: &str = "I don't have anything on that here.";
/// Lowercase, every non-alphanumeric byte becomes a space, runs collapse. Pure, no locale.
fn normalize(prompt: &str) -> String {
    let mut out = String::with_capacity(prompt.len());
    for c in prompt.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.is_empty() && !out.ends_with(' ') {
            out.push(' ');
        }
    }
    out.trim_end().to_owned()
}
/// Patterns match on token boundaries, so a multi-word pattern is a phrase, and a longer phrase
/// is worth more than the words it contains.
fn hits(normalized: &str, patterns: &[String]) -> u64 {
    let haystack = format!(" {normalized} ");
    patterns
        .iter()
        .map(|p| normalize(p))
        .filter(|p| !p.is_empty() && haystack.contains(&format!(" {p} ")))
        .map(|p| 1 + p.split(' ').count() as u64)
        .sum()
}
/// FNV-1a, inline and dependency-free: reply choice must not depend on the world seed or a clock.
pub fn fnv1a64(text: &str) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in text.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}
fn choose<'a>(options: &'a [String], normalized: &str, attempt: u64) -> &'a str {
    if options.is_empty() {
        return NO_ANSWER;
    }
    let idx = fnv1a64(normalized) ^ attempt.wrapping_mul(0x9e37_79b9_7f4a_7c15);
    &options[(idx % options.len() as u64) as usize]
}
enum Pick<'a> {
    Fact(&'a Fact),
    Intent(&'a Intent),
}
impl AssistantState {
    pub fn open(&self, actor: &str, id: &str) -> Result<&Conversation, String> {
        self.conversations
            .get(id)
            .filter(|c| c.owner == actor)
            .ok_or("conversation unavailable".into())
    }
    fn owned_mut(&mut self, actor: &str, id: &str) -> Result<&mut Conversation, String> {
        self.conversations
            .get_mut(id)
            .filter(|c| c.owner == actor)
            .ok_or("conversation unavailable".into())
    }
    pub fn mine<'a>(&'a self, actor: &'a str) -> impl Iterator<Item = &'a Conversation> + 'a {
        self.conversations
            .values()
            .filter(move |c| c.owner == actor)
    }
    /// The whole reply engine: score both tables, take the best row, pick a string from it.
    pub fn reply(&self, prompt: &str, attempt: u64, tick: u64) -> Message {
        let n = normalize(prompt);
        let mut best: Option<(u64, &str, Pick<'_>)> = None;
        for fact in &self.facts {
            let score = hits(&n, &fact.patterns);
            if score > 0 {
                best = better(best, score + FACT_BONUS, &fact.id, Pick::Fact(fact));
            }
        }
        for intent in &self.intents {
            let score = hits(&n, &intent.patterns);
            if score > 0 {
                best = better(best, score, &intent.id, Pick::Intent(intent));
            }
        }
        let (text, citations, intent_id) = match best {
            // A fact has one answer; regenerating must not invent a second version of the truth.
            Some((_, id, Pick::Fact(f))) => (f.answer.clone(), f.citations.clone(), id.to_owned()),
            Some((_, id, Pick::Intent(i))) => (
                choose(&i.replies, &n, attempt).to_owned(),
                i.citations.clone(),
                id.to_owned(),
            ),
            None => (
                choose(&self.fallback, &n, attempt).to_owned(),
                vec![],
                String::new(),
            ),
        };
        Message {
            role: "assistant".into(),
            text,
            citations,
            intent_id,
            attempt,
            tick,
        }
    }
    pub fn start(
        &mut self,
        actor: &str,
        title: &str,
        message: &str,
        tick: u64,
    ) -> Result<Conversation, String> {
        if message.trim().is_empty() {
            return Err("message required".into());
        }
        self.next_id = self.next_id.max(1);
        while self
            .conversations
            .contains_key(&format!("conv-{}", self.next_id))
        {
            self.next_id = self.next_id.checked_add(1).ok_or("ID space exhausted")?;
        }
        let id = format!("conv-{}", self.next_id);
        let title = match title.trim() {
            "" => summarise(message),
            t => t.to_owned(),
        };
        let conversation = Conversation {
            id: id.clone(),
            owner: actor.into(),
            title,
            tick,
            messages: vec![],
        };
        self.conversations.insert(id.clone(), conversation);
        self.send(actor, &id, message, tick)
    }
    pub fn send(
        &mut self,
        actor: &str,
        id: &str,
        message: &str,
        tick: u64,
    ) -> Result<Conversation, String> {
        if message.trim().is_empty() {
            return Err("message required".into());
        }
        let reply = self.reply(message, 0, tick);
        let conversation = self.owned_mut(actor, id)?;
        conversation.messages.push(Message {
            role: "user".into(),
            text: message.into(),
            tick,
            ..Message::default()
        });
        conversation.messages.push(reply);
        Ok(conversation.clone())
    }
    /// Replace the trailing assistant turn with the next choice for the same prompt.
    pub fn regenerate(&mut self, actor: &str, id: &str, tick: u64) -> Result<Conversation, String> {
        let conversation = self.open(actor, id)?;
        let last = conversation
            .messages
            .last()
            .filter(|m| m.role == "assistant")
            .ok_or("nothing to regenerate")?;
        let attempt = last.attempt.checked_add(1).ok_or("attempts exhausted")?;
        let prompt = conversation
            .messages
            .iter()
            .rev()
            .find(|m| m.role == "user")
            .map(|m| m.text.clone())
            .ok_or("nothing to regenerate")?;
        let reply = self.reply(&prompt, attempt, tick);
        let conversation = self.owned_mut(actor, id)?;
        conversation.messages.pop();
        conversation.messages.push(reply);
        Ok(conversation.clone())
    }
    pub fn rename(&mut self, actor: &str, id: &str, title: &str) -> Result<Conversation, String> {
        if title.trim().is_empty() {
            return Err("title required".into());
        }
        let conversation = self.owned_mut(actor, id)?;
        conversation.title = title.trim().into();
        Ok(conversation.clone())
    }
    pub fn remove(&mut self, actor: &str, id: &str) -> Result<(), String> {
        self.open(actor, id)?;
        self.conversations.remove(id);
        Ok(())
    }
}
/// Max score wins; a tie goes to the lexicographically smaller id, so ordering is total.
fn better<'a>(
    best: Option<(u64, &'a str, Pick<'a>)>,
    score: u64,
    id: &'a str,
    pick: Pick<'a>,
) -> Option<(u64, &'a str, Pick<'a>)> {
    match &best {
        Some((s, i, _)) if *s > score || (*s == score && **i <= *id) => best,
        _ => Some((score, id, pick)),
    }
}
/// Sidebar titles come from the prompt when the caller did not name the conversation.
fn summarise(message: &str) -> String {
    let words: Vec<_> = message.split_whitespace().take(6).collect();
    match words.join(" ") {
        s if s.is_empty() => "New chat".into(),
        s if s.len() < message.trim().len() => format!("{s}…"),
        s => s,
    }
}
pub struct AssistantService;
pub fn register(registry: &mut Registry) -> SimResult<()> {
    registry.register(AssistantService)
}
struct Palette {
    accent: String,
    background: String,
    surface: String,
    ink: String,
    muted: String,
}
impl Palette {
    fn of(theme: &PageTheme) -> Self {
        let pick = |v: &Option<String>, d: &str| v.clone().unwrap_or_else(|| d.to_owned());
        Self {
            accent: pick(&theme.accent, "#10a37f"),
            background: pick(&theme.background, "#212121"),
            surface: pick(&theme.surface, "#2f2f2f"),
            ink: pick(&theme.ink, "#ececec"),
            muted: pick(&theme.muted, "#9b9b9b"),
        }
    }
}
/// A control that posts literal fields; `web` has no button helper and a Button is never inert.
fn post(url: &str, fields: &[(&str, &str)]) -> PageAction {
    PageAction {
        method: "POST".into(),
        url: url.into(),
        fields: fields
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect(),
    }
}
fn button(id: &str, text: impl Into<String>, action: PageAction) -> PageElement {
    PageElement::Button {
        id: id.into(),
        text: text.into(),
        action,
        style: None,
    }
}
fn bubble(p: &Palette, index: usize, message: &Message) -> PageElement {
    let mine = message.role == "user";
    let body = web::styled(
        &format!("msg-{index}-text"),
        &message.text,
        web::style().size(15).color(p.ink.clone()),
    );
    let mut children = vec![body];
    if !message.citations.is_empty() {
        children.push(web::spacer(&format!("msg-{index}-gap"), 8));
        children.push(web::styled(
            &format!("msg-{index}-sources"),
            "Sources",
            web::style().size(12).bold().color(p.muted.clone()),
        ));
        for (n, citation) in message.citations.iter().enumerate() {
            children.push(web::link(
                &format!("msg-{index}-cite-{n}"),
                format!("[{}] {}", n + 1, citation.label),
                &citation.url,
            ));
        }
    }
    let card = web::card(
        &format!("msg-{index}-card"),
        web::style()
            .background(if mine {
                p.surface.clone()
            } else {
                p.background.clone()
            })
            .radius(18)
            .padding(14)
            .flex(if mine { 3 } else { 1 }),
        children,
    );
    if mine {
        return web::styled_row(
            &format!("msg-{index}"),
            12,
            "start",
            Style::default(),
            vec![
                web::card(&format!("msg-{index}-gutter"), web::style().flex(2), vec![]),
                card,
                web::thumbnail(
                    &format!("msg-{index}-avatar"),
                    "You",
                    web::style()
                        .width(32)
                        .height(32)
                        .radius(16)
                        .background(p.surface.clone())
                        .color(p.muted.clone())
                        .flex(0),
                ),
            ],
        );
    }
    web::row(
        &format!("msg-{index}"),
        12,
        "start",
        vec![
            web::thumbnail(
                &format!("msg-{index}-avatar"),
                "AI",
                web::style()
                    .width(32)
                    .height(32)
                    .radius(16)
                    .background(p.accent.clone())
                    .color(p.background.clone())
                    .flex(0),
            ),
            card,
        ],
    )
}
/// Sidebar + main column, the layout both products share.
fn shell(
    s: &AssistantState,
    actor: &str,
    active: Option<&str>,
    main: Vec<PageElement>,
) -> SimResult<HttpResponse> {
    let p = Palette::of(&s.theme);
    let mut side = vec![
        web::row(
            "side-brand",
            10,
            "center",
            vec![
                web::thumbnail(
                    "side-logo",
                    s.brand.chars().next().unwrap_or('A').to_string(),
                    web::style()
                        .width(28)
                        .height(28)
                        .radius(14)
                        .background(p.accent.clone())
                        .color(p.background.clone())
                        .flex(0),
                ),
                web::styled(
                    "side-brand-name",
                    &s.brand,
                    web::style().size(15).bold().color(p.ink.clone()),
                ),
            ],
        ),
        web::spacer("side-gap", 12),
        web::link("side-new", "+  New chat", "/"),
        web::divider("side-rule"),
        web::styled(
            "side-label",
            "Chats",
            web::style().size(11).bold().color(p.muted.clone()),
        ),
    ];
    for conversation in s.mine(actor) {
        let current = active == Some(conversation.id.as_str());
        side.push(web::card_action(
            &format!("side-{}", conversation.id),
            web::style()
                .background(if current {
                    p.surface.clone()
                } else {
                    p.background.clone()
                })
                .radius(8)
                .padding(8),
            web::visit(format!("/c/{}", conversation.id)),
            vec![web::styled(
                &format!("side-{}-title", conversation.id),
                &conversation.title,
                web::style()
                    .size(13)
                    .color(if current { &p.ink } else { &p.muted }.clone())
                    .one_line(),
            )],
        ));
    }
    if s.mine(actor).next().is_none() {
        side.push(web::styled(
            "side-empty",
            "No conversations yet.",
            web::style().size(12).color(p.muted.clone()),
        ));
    }
    web::themed_page(
        &s.brand,
        s.theme.clone(),
        vec![web::row(
            "shell",
            0,
            "stretch",
            vec![
                web::card(
                    "sidebar",
                    web::style()
                        .width(260)
                        .flex(0)
                        .background(p.surface.clone())
                        .padding(14),
                    side,
                ),
                web::card("main", web::style().flex(1).padding(24), main),
            ],
        )],
    )
}
fn home(s: &AssistantState, actor: &str) -> SimResult<HttpResponse> {
    let p = Palette::of(&s.theme);
    let greeting = match s.greeting.as_str() {
        "" => "What can I help with?",
        g => g,
    };
    let mut main = vec![
        web::spacer("home-lead", 64),
        web::styled(
            "home-greeting",
            greeting,
            web::style()
                .size(30)
                .bold()
                .color(p.ink.clone())
                .align("center"),
        ),
        web::spacer("home-gap", 20),
    ];
    if !s.suggestions.is_empty() {
        main.push(web::grid(
            "suggestions",
            2,
            12,
            s.suggestions
                .iter()
                .enumerate()
                .map(|(i, text)| {
                    button(
                        &format!("suggestion-{i}"),
                        text,
                        post("/conversations", &[("message", text)]),
                    )
                })
                .collect(),
        ));
        main.push(web::spacer("suggestions-gap", 20));
    }
    main.push(web::form(
        "composer",
        "/conversations",
        &[("message", "Message", "")],
    ));
    if !s.model_label.is_empty() {
        main.push(web::styled(
            "home-model",
            format!(
                "{} · deterministic replies, citations you can check",
                s.model_label
            ),
            web::style().size(12).color(p.muted.clone()).align("center"),
        ));
    }
    shell(s, actor, None, main)
}
fn conversation(s: &AssistantState, actor: &str, id: &str) -> SimResult<HttpResponse> {
    let p = Palette::of(&s.theme);
    let c = match s.open(actor, id) {
        Ok(c) => c,
        Err(e) => return web::error(404, e),
    };
    let mut main = vec![
        web::row(
            "head",
            10,
            "center",
            vec![
                web::styled(
                    "head-title",
                    &c.title,
                    web::style().size(20).bold().color(p.ink.clone()),
                ),
                web::badge(
                    "head-model",
                    if s.model_label.is_empty() {
                        s.brand.clone()
                    } else {
                        s.model_label.clone()
                    },
                    web::style()
                        .size(11)
                        .background(p.surface.clone())
                        .color(p.muted.clone())
                        .radius(10)
                        .padding(6)
                        .flex(0),
                ),
            ],
        ),
        web::divider("head-rule"),
    ];
    for (index, message) in c.messages.iter().enumerate() {
        main.push(bubble(&p, index, message));
        main.push(web::spacer(&format!("msg-{index}-after"), 10));
    }
    main.push(web::row(
        "turn-controls",
        10,
        "center",
        vec![button(
            "regenerate",
            "Regenerate",
            post(&format!("/conversations/{id}/regenerate"), &[]),
        )],
    ));
    main.push(web::divider("compose-rule"));
    main.push(web::form(
        "composer",
        &format!("/conversations/{id}/messages"),
        &[("message", "Message", "")],
    ));
    main.push(web::form(
        "rename",
        &format!("/conversations/{id}/rename"),
        &[("title", "Rename conversation", &c.title)],
    ));
    main.push(button(
        "delete",
        "Delete conversation",
        post(&format!("/conversations/{id}/delete"), &[]),
    ));
    shell(s, actor, Some(id), main)
}
/// Documented seed keys, checked by container type before the typed load reports anything finer.
const OBJECTS: &[&str] = &["theme", "conversations"];
const ARRAYS: &[&str] = &["suggestions", "intents", "facts", "fallback"];
impl Service for AssistantService {
    fn kind(&self) -> &str {
        "assistant"
    }
    fn initialize(&self, initial: Value, _: &ServiceContext) -> SimResult<Value> {
        let gated = web::shape(initial, OBJECTS, ARRAYS)?;
        let mut s: AssistantState = web::load(&gated)?;
        if s.brand.is_empty() {
            s.brand = "Assistant".into();
        }
        for fact in &s.facts {
            // A fact the reader cannot check is just patter wearing a fact's clothes.
            if fact.citations.is_empty() || fact.citations.iter().any(|c| c.url.trim().is_empty()) {
                return Err(cw_protocol::SimError::invalid(format!(
                    "fact {} needs at least one citation with a URL",
                    fact.id
                )));
            }
        }
        for c in s.conversations.values() {
            if c.owner.is_empty() {
                return Err(cw_protocol::SimError::invalid(format!(
                    "conversation {} needs an owner",
                    c.id
                )));
            }
        }
        Ok(serde_json::to_value(s)?)
    }
    fn handle(
        &self,
        state: &mut Value,
        c: &ServiceContext,
        r: &HttpRequest,
    ) -> SimResult<HttpResponse> {
        let mut s: AssistantState = web::load(state)?;
        let full = web::path(r);
        let path = full.strip_prefix("/api").unwrap_or(&full);
        let api = full.starts_with("/api/") || full == "/api";
        let parts: Vec<_> = path.trim_matches('/').split('/').collect();
        let method = r.method.to_ascii_uppercase();
        if method == "GET" {
            return match parts.as_slice() {
                [""] if !api => home(&s, &c.actor),
                ["c", id] if !api => conversation(&s, &c.actor, id),
                ["conversations"] => HttpResponse::json(200, &s.mine(&c.actor).collect::<Vec<_>>()),
                ["conversations", id] => web::domain(s.open(&c.actor, id).map(|v| json!(v))),
                _ => web::error(404, "route not found"),
            };
        }
        let b = web::body(r)?;
        let message = web::text(&b, "message");
        let mut deleted = false;
        let result = match (method.as_str(), parts.as_slice()) {
            ("POST", ["conversations"]) => s
                .start(&c.actor, &web::text(&b, "title"), &message, c.tick)
                .map(|v| json!(v)),
            ("POST", ["conversations", id, "messages"]) => {
                s.send(&c.actor, id, &message, c.tick).map(|v| json!(v))
            }
            ("POST", ["conversations", id, "regenerate"]) => {
                s.regenerate(&c.actor, id, c.tick).map(|v| json!(v))
            }
            ("POST" | "PATCH", ["conversations", id, "rename"])
            | ("PATCH", ["conversations", id]) => s
                .rename(&c.actor, id, &web::text(&b, "title"))
                .map(|v| json!(v)),
            ("POST", ["conversations", id, "delete"]) | ("DELETE", ["conversations", id]) => {
                deleted = true;
                s.remove(&c.actor, id).map(|()| json!({"deleted": id}))
            }
            _ => return web::error(405, "unsupported route or method"),
        };
        if result.is_err() {
            return web::domain(result);
        }
        web::save(state, &s)?;
        if api {
            return web::domain(result);
        }
        match (deleted, result.as_ref().ok().and_then(|v| v["id"].as_str())) {
            (false, Some(id)) => conversation(&s, &c.actor, id),
            _ => home(&s, &c.actor),
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    /// The two shipped seeds; the crate tests hold them to the same standard the plan sets.
    const OPENAI: &str = include_str!("../../../worlds/company-2026/sites/openai.json");
    const ANTHROPIC: &str = include_str!("../../../worlds/company-2026/sites/anthropic.json");
    fn ctx() -> ServiceContext {
        ServiceContext {
            actor: "alice".into(),
            source: "alice-mac".into(),
            tick: 7,
            seed: 1,
            instance: "assistant".into(),
        }
    }
    fn seeded(source: &str) -> AssistantState {
        let site: Value = serde_json::from_str(source).unwrap();
        let state = AssistantService
            .initialize(site["initial_state"].clone(), &ctx())
            .unwrap();
        serde_json::from_value(state).unwrap()
    }
    /// A page the browser would reject is not a page; ids must be unique and colours well formed.
    fn rendered(response: &HttpResponse) -> cw_protocol::Page {
        assert_eq!(response.status, 200);
        let page: cw_protocol::Page = serde_json::from_slice(&response.body).unwrap();
        page.validate().unwrap();
        page
    }
    #[test]
    fn every_fact_citation_url_is_non_empty() {
        for source in [OPENAI, ANTHROPIC] {
            let s = seeded(source);
            assert!(!s.facts.is_empty(), "a seed with no facts cites nothing");
            for fact in &s.facts {
                assert!(
                    !fact.citations.is_empty(),
                    "fact {} has no citation",
                    fact.id
                );
                for citation in &fact.citations {
                    assert!(!citation.url.trim().is_empty(), "fact {} cites ''", fact.id);
                    assert!(
                        citation.url.starts_with("http://"),
                        "fact {} must cite a page in this world",
                        fact.id
                    );
                    assert!(!citation.label.trim().is_empty());
                }
            }
        }
    }
    #[test]
    fn both_assistants_agree_on_shared_facts() {
        let (a, b) = (seeded(OPENAI), seeded(ANTHROPIC));
        let shared: Vec<_> = a
            .facts
            .iter()
            .filter_map(|f| b.facts.iter().find(|g| g.id == f.id).map(|g| (f, g)))
            .collect();
        assert!(shared.len() >= 3, "the two products share too little world");
        for (left, right) in shared {
            assert_eq!(left.answer, right.answer, "fact {} disagrees", left.id);
            assert_eq!(
                left.citations, right.citations,
                "fact {} cites differently",
                left.id
            );
        }
        // And they agree when asked, not merely in the table.
        for prompt in [
            "What is the Atlas release code?",
            "who owns the atlas launch",
        ] {
            assert_eq!(
                a.reply(prompt, 0, 0).text,
                b.reply(prompt, 0, 0).text,
                "the two assistants answer {prompt:?} differently"
            );
        }
    }
    #[test]
    fn the_same_prompt_at_the_same_attempt_always_replies_the_same() {
        let s = seeded(OPENAI);
        for prompt in [
            "Explain deterministic simulation",
            "what is the atlas release code",
            "something entirely unrelated to this world",
        ] {
            for attempt in 0..4 {
                let first = s.reply(prompt, attempt, 0);
                assert_eq!(first, s.reply(prompt, attempt, 0));
                // A different tick is a different message, never a different answer.
                assert_eq!(first.text, s.reply(prompt, attempt, 99).text);
            }
        }
        // Normalisation, not luck: punctuation and case cannot change the answer.
        assert_eq!(
            s.reply("What is the ATLAS release code?", 0, 0).text,
            s.reply("what   is the atlas release code", 0, 0).text
        );
    }
    #[test]
    fn seeded_transcripts_are_what_the_engine_would_say() {
        for source in [OPENAI, ANTHROPIC] {
            let s = seeded(source);
            for c in s.conversations.values() {
                assert!(!c.messages.is_empty(), "{} is an empty transcript", c.id);
                for pair in c.messages.chunks(2) {
                    assert_eq!(pair.len(), 2, "{} ends mid-turn", c.id);
                    assert_eq!(pair[0].role, "user");
                    let expected = s.reply(&pair[0].text, pair[1].attempt, pair[1].tick);
                    assert_eq!(pair[1], expected, "{} drifted from the reply table", c.id);
                }
            }
        }
    }
    #[test]
    fn facts_outrank_intents_and_carry_their_citations() {
        let s = seeded(ANTHROPIC);
        let m = s.reply("what is the atlas release code", 0, 0);
        assert!(m.text.contains("ATLAS-2026"));
        assert_eq!(m.intent_id, "atlas-release-code");
        assert!(m.citations.iter().all(|c| !c.url.is_empty()));
        // A prompt matching only an intent still answers, and an unknown one falls back.
        assert_eq!(
            s.reply("explain determinism", 0, 0).intent_id,
            "determinism"
        );
        assert!(s.reply("zqx unmatched", 0, 0).intent_id.is_empty());
    }
    #[test]
    fn conversations_mutate_are_private_and_round_trip() {
        let mut s = seeded(OPENAI);
        let started = s
            .start("bob", "", "What is the Atlas release code?", 4)
            .unwrap();
        assert_eq!(started.messages.len(), 2);
        assert!(started.messages[1].text.contains("ATLAS-2026"));
        s.send("bob", &started.id, "and who owns it?", 5).unwrap();
        let grown = s.open("bob", &started.id).unwrap().clone();
        assert_eq!(grown.messages.len(), 4);
        assert!(s.open("alice", &started.id).is_err(), "chats are per actor");
        assert!(s.send("alice", &started.id, "hi", 6).is_err());
        assert!(s.send("bob", &started.id, "   ", 6).is_err());
        s.regenerate("bob", &started.id, 7).unwrap();
        let after = s.open("bob", &started.id).unwrap();
        assert_eq!(
            after.messages.len(),
            4,
            "regenerate replaces, never appends"
        );
        assert_eq!(after.messages[3].attempt, 1);
        s.rename("bob", &started.id, "Release code").unwrap();
        assert_eq!(s.open("bob", &started.id).unwrap().title, "Release code");
        assert!(s.rename("bob", &started.id, " ").is_err());
        let restored: AssistantState =
            serde_json::from_slice(&serde_json::to_vec(&s).unwrap()).unwrap();
        assert_eq!(s, restored, "state must survive a snapshot round trip");
        s.remove("bob", &started.id).unwrap();
        assert!(s.open("bob", &started.id).is_err());
    }
    #[test]
    fn regenerate_walks_an_intent_and_leaves_a_fact_alone() {
        let mut s = seeded(OPENAI);
        let c = s
            .start("alice", "", "Explain deterministic simulation", 0)
            .unwrap();
        let mut seen = vec![c.messages[1].text.clone()];
        for _ in 0..4 {
            let c = s.regenerate("alice", &c.id, 1).unwrap();
            seen.push(c.messages.last().unwrap().text.clone());
        }
        seen.sort();
        seen.dedup();
        assert!(
            seen.len() > 1,
            "regenerate must be able to say it differently"
        );
        let f = s.start("alice", "", "atlas release code", 0).unwrap();
        let fixed = f.messages[1].text.clone();
        assert_eq!(
            s.regenerate("alice", &f.id, 1).unwrap().messages[1].text,
            fixed
        );
    }
    #[test]
    fn routes_render_mutate_and_refuse() {
        let site: Value = serde_json::from_str(OPENAI).unwrap();
        let mut state = AssistantService
            .initialize(site["initial_state"].clone(), &ctx())
            .unwrap();
        let before = state.clone();
        let home = HttpRequest::get("http://chatgpt.com/");
        let page = AssistantService.handle(&mut state, &ctx(), &home).unwrap();
        assert_eq!(rendered(&page).title, "ChatGPT");
        assert_eq!(
            page,
            AssistantService.handle(&mut state, &ctx(), &home).unwrap()
        );
        assert_eq!(before, state, "rendering must not mutate seed state");
        let mut post = HttpRequest::get("http://chatgpt.com/api/conversations");
        post.method = "POST".into();
        post.body = br#"{"message":"What is the Atlas release code?"}"#.to_vec();
        let created = AssistantService.handle(&mut state, &ctx(), &post).unwrap();
        assert_eq!(created.status, 200);
        assert_ne!(before, state, "sending a turn must move real state");
        let created: Value = serde_json::from_slice(&created.body).unwrap();
        let id = created["id"].as_str().unwrap().to_owned();
        let view = HttpRequest::get(format!("http://chatgpt.com/c/{id}"));
        let page = rendered(&AssistantService.handle(&mut state, &ctx(), &view).unwrap());
        let body = serde_json::to_string(&page).unwrap();
        assert!(body.contains("ATLAS-2026") && body.contains("atlas-launch"));
        // Every seeded conversation renders too, which is where duplicate ids would surface.
        for source in [OPENAI, ANTHROPIC] {
            let seed: Value = serde_json::from_str(source).unwrap();
            let mut s = AssistantService
                .initialize(seed["initial_state"].clone(), &ctx())
                .unwrap();
            let state: AssistantState = serde_json::from_value(s.clone()).unwrap();
            for (id, c) in &state.conversations {
                let mut actor = ctx();
                actor.actor = c.owner.clone();
                let url = HttpRequest::get(format!("http://claude.ai/c/{id}"));
                rendered(&AssistantService.handle(&mut s, &actor, &url).unwrap());
                rendered(
                    &AssistantService
                        .handle(&mut s, &actor, &HttpRequest::get("http://claude.ai/"))
                        .unwrap(),
                );
            }
        }
        let mut other = ctx();
        other.actor = "carol".into();
        assert_eq!(
            AssistantService
                .handle(&mut state, &other, &view)
                .unwrap()
                .status,
            404,
            "another actor cannot read the conversation"
        );
        let mut empty = post.clone();
        empty.body = br#"{"message":"  "}"#.to_vec();
        assert_eq!(
            AssistantService
                .handle(&mut state, &ctx(), &empty)
                .unwrap()
                .status,
            400
        );
        assert_eq!(
            AssistantService
                .handle(
                    &mut state,
                    &ctx(),
                    &HttpRequest::get("http://chatgpt.com/nope")
                )
                .unwrap()
                .status,
            404
        );
    }
    /// A site can be bound to a node before its content lands; an empty table still answers.
    #[test]
    fn an_empty_assistant_still_serves_a_themed_home() {
        let mut state = AssistantService.initialize(json!({}), &ctx()).unwrap();
        let page = rendered(
            &AssistantService
                .handle(&mut state, &ctx(), &HttpRequest::get("http://chatgpt.com/"))
                .unwrap(),
        );
        assert!(page.theme.is_some());
        assert_eq!(page.title, "Assistant");
    }
    #[test]
    fn seed_shape_is_gated_at_load() {
        assert!(AssistantService.initialize(json!([]), &ctx()).is_err());
        assert!(AssistantService
            .initialize(json!({"conversations": []}), &ctx())
            .is_err());
        assert!(
            AssistantService
                .initialize(
                    json!({"facts": [{"id": "x", "patterns": ["x"], "answer": "y"}]}),
                    &ctx()
                )
                .is_err(),
            "an uncitable fact must not load"
        );
        assert!(AssistantService.initialize(Value::Null, &ctx()).is_ok());
    }
}
