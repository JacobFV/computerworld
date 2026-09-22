//! Deterministic chat assistants: chatgpt.com and claude.ai. There is no model behind them, only
//! a scored pattern table, so the same prompt at the same `attempt` always yields the same reply.
//!
//! Two tables answer a prompt. `intents` are generic canned explanations; `facts` are claims about
//! this world that carry a citation the agent can click and check. Facts outrank intents by a
//! fixed margin, so "what is the Atlas release code" is answered from the world, not from patter.
use cw_protocol::{HttpRequest, HttpResponse, PageTheme, Result as SimResult};
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
    /// `chatgpt` or `claude`; empty means "decide from the brand".
    pub skin: String,
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
mod view;
use view::{conversation, home};
/// The two looks: ChatGPT's dark shell and Claude's warm paper. An absent `skin` is read
/// from the brand, so a seed written before the key existed still gets its own look.
pub const SKINS: &[&str] = &["chatgpt", "claude"];
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
        if !s.skin.is_empty() && !SKINS.contains(&s.skin.as_str()) {
            return Err(cw_protocol::SimError::invalid(format!(
                "unknown skin {}; expected one of {}",
                s.skin,
                SKINS.join(", ")
            )));
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
    use cw_web::dom::Document as Dom;
    use serde_json::json;
    /// The two shipped seeds; the crate tests hold them to the same standard the plan sets.
    const OPENAI: &str = include_str!("../../../../worlds/company-2026/sites/openai.json");
    const ANTHROPIC: &str = include_str!("../../../../worlds/company-2026/sites/anthropic.json");
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
    /// A page the engine would refuse is not a page: every response runs through the strict
    /// validator (known CSS only, unique ids) before a test reads it.
    fn rendered(response: &HttpResponse) -> Dom {
        assert_eq!(response.status, 200);
        assert_eq!(
            response.header("content-type"),
            Some(web::html::HTML_MEDIA_TYPE)
        );
        let html = std::str::from_utf8(&response.body).unwrap();
        web::html::validate_strict(html).unwrap_or_else(|e| panic!("strict: {e:?}"));
        cw_web::html::parse(html)
    }
    fn node(doc: &Dom, id: &str) -> cw_web::dom::NodeId {
        *doc.by_id(id).first().unwrap_or_else(|| panic!("no #{id}"))
    }
    fn attr<'a>(doc: &'a Dom, id: &str, name: &str) -> &'a str {
        doc.attr(node(doc, id), name)
            .unwrap_or_else(|| panic!("#{id} has no {name}"))
    }
    fn text_of(doc: &Dom, id: &str) -> String {
        doc.text_content(node(doc, id))
    }
    fn title(doc: &Dom) -> String {
        let t = doc
            .descendants(Dom::ROOT)
            .find(|n| doc.is(*n, "title"))
            .expect("a title");
        doc.text_content(t)
    }
    fn form_post(url: &str, body: &str) -> HttpRequest {
        let mut r = HttpRequest::get(url);
        r.method = "POST".into();
        r.headers.insert(
            "content-type".into(),
            "application/x-www-form-urlencoded".into(),
        );
        r.body = body.as_bytes().to_vec();
        r
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
        let doc = rendered(&page);
        assert_eq!(title(&doc), "ChatGPT");
        assert_eq!(attr(&doc, "composer", "action"), "/conversations");
        assert_eq!(attr(&doc, "composer", "method"), "post");
        assert_eq!(attr(&doc, "composer-message", "name"), "message");
        assert_eq!(doc.tag(node(&doc, "composer-submit")), Some("button"));
        assert_eq!(attr(&doc, "suggestion-1-form", "action"), "/conversations");
        assert_eq!(
            text_of(&doc, "suggestion-1"),
            "What is the Atlas release code?"
        );
        assert_eq!(attr(&doc, "side-new", "href"), "/");
        assert_eq!(attr(&doc, "side-conv-1", "href"), "/c/conv-1");
        assert_eq!(text_of(&doc, "side-conv-1-title"), "Deterministic services");
        assert_eq!(text_of(&doc, "home-greeting"), "What can I help with?");
        assert!(text_of(&doc, "home-model").starts_with("GPT-5"));
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
        let doc = rendered(&AssistantService.handle(&mut state, &ctx(), &view).unwrap());
        assert_eq!(title(&doc), "What is the Atlas release code? - ChatGPT");
        assert_eq!(
            text_of(&doc, "msg-0-text"),
            "What is the Atlas release code?"
        );
        assert!(text_of(&doc, "msg-1-text").contains("ATLAS-2026"));
        assert_eq!(text_of(&doc, "msg-1-sources"), "Sources");
        assert!(attr(&doc, "msg-1-cite-0", "href").contains("atlas-launch"));
        assert_eq!(
            attr(&doc, "composer", "action"),
            format!("/conversations/{id}/messages")
        );
        assert_eq!(
            attr(&doc, "regenerate-form", "action"),
            format!("/conversations/{id}/regenerate")
        );
        assert_eq!(doc.tag(node(&doc, "regenerate")), Some("button"));
        assert_eq!(
            attr(&doc, "rename", "action"),
            format!("/conversations/{id}/rename")
        );
        assert_eq!(attr(&doc, "rename-title", "name"), "title");
        assert_eq!(
            attr(&doc, "rename-title", "value"),
            "What is the Atlas release code?"
        );
        assert_eq!(
            attr(&doc, "delete-form", "action"),
            format!("/conversations/{id}/delete")
        );
        assert!(attr(&doc, &format!("side-{id}"), "class").contains("on"));
        for kept in [
            "shell",
            "sidebar",
            "main",
            "side-brand",
            "side-logo",
            "side-brand-name",
            "side-label",
            "head",
            "head-title",
            "head-model",
            "msg-0",
            "msg-0-card",
            "msg-0-avatar",
            "turn-controls",
            "rename-submit",
            "delete",
        ] {
            node(&doc, kept);
        }
        // The forms the page offers work as a browser submits them: urlencoded posts that
        // answer with the conversation page, and a delete that lands back on the home page.
        let sent = AssistantService
            .handle(
                &mut state,
                &ctx(),
                &form_post(
                    &format!("http://chatgpt.com/conversations/{id}/messages"),
                    "message=who+owns+the+atlas+launch",
                ),
            )
            .unwrap();
        let doc = rendered(&sent);
        assert_eq!(text_of(&doc, "msg-2-text"), "who owns the atlas launch");
        assert!(text_of(&doc, "msg-3-text").contains("Carol"));
        let renamed = AssistantService
            .handle(
                &mut state,
                &ctx(),
                &form_post(
                    &format!("http://chatgpt.com/conversations/{id}/rename"),
                    "title=Release+code",
                ),
            )
            .unwrap();
        assert_eq!(text_of(&rendered(&renamed), "head-title"), "Release code");
        rendered(
            &AssistantService
                .handle(
                    &mut state,
                    &ctx(),
                    &form_post(
                        &format!("http://chatgpt.com/conversations/{id}/regenerate"),
                        "",
                    ),
                )
                .unwrap(),
        );
        let gone = rendered(
            &AssistantService
                .handle(
                    &mut state,
                    &ctx(),
                    &form_post(&format!("http://chatgpt.com/conversations/{id}/delete"), ""),
                )
                .unwrap(),
        );
        assert!(gone.by_id(&format!("side-{id}")).is_empty());
        node(&gone, "home-greeting");
        let started = rendered(
            &AssistantService
                .handle(
                    &mut state,
                    &ctx(),
                    &form_post(
                        "http://chatgpt.com/conversations",
                        "message=Explain+git+rebase",
                    ),
                )
                .unwrap(),
        );
        assert_eq!(text_of(&started, "msg-0-text"), "Explain git rebase");
        let id = "conv-1".to_owned();
        let view = HttpRequest::get(format!("http://chatgpt.com/c/{id}"));
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
        assert_eq!(title(&page), "Assistant");
        let root = page
            .descendants(Dom::ROOT)
            .find(|n| page.is(*n, "html"))
            .unwrap();
        assert!(page
            .attr(root, "style")
            .unwrap()
            .contains("--accent: #10a37f"));
        node(&page, "side-empty");
        assert!(page.by_id("suggestions").is_empty() && page.by_id("home-model").is_empty());
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
