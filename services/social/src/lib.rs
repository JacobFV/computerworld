//! Feeds: x.com and mastodon.social (`mode: microblog`), linkedin.com (`mode: professional`),
//! instagram.com and pinterest.com (`mode: photos`: every post is a picture with a caption).
//! Handles are world-wide identities, so every account maps to an OS actor or to nobody.
//!
//! Pages are HTML (`view.rs`), one skin per product (`x.css`, `bsky.css`, `mastodon.css`,
//! `facebook.css`, `instagram.css`, `linkedin.css`, `pinterest.css` over the shared `social.css`);
//! the skin is the seed's optional `skin`, else inferred from the brand, else from the mode.
//!
//! One router renders every GET, and a successful form POST re-renders through the same router,
//! so a control always lands the caller on a real page instead of a JSON blob. `/api/*` mirrors
//! the mutating routes for agents that would rather read JSON.
use cw_protocol::{HttpRequest, HttpResponse, PageTheme, Result as SimResult, SimError};
use cw_sdk::{Registry, Service, ServiceContext};
use cw_service_common as web;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
mod view;
pub use view::{skin_of, SKINS};
use view::View;
pub struct SocialService;
pub fn register(registry: &mut Registry) -> SimResult<()> {
    registry.register(SocialService)
}
/// Documented seed keys, gated by container type before the typed load so an authoring typo is a
/// load error rather than a silently empty feed.
const OBJECTS: &[&str] = &[
    "theme",
    "accounts",
    "posts",
    "follows",
    "likes",
    "reposts",
    "connections",
    "messages",
];
const ARRAYS: &[&str] = &[];
/// `mode` is the documented discriminant; an unlisted value is a seed typo, not a fallback.
pub const MODES: &[&str] = &["microblog", "professional", "photos"];
pub const MAX_POST: usize = 500;
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct SocialState {
    pub mode: String,
    /// The look: one of [`SKINS`]. Optional; absent, the brand and then the mode decide, and
    /// it stays out of the serialized state so a seed without it snapshots as it always did.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub skin: String,
    pub brand: String,
    pub tagline: String,
    /// The second timeline's name: "Explore" on X, "Local timeline" on a Mastodon instance.
    pub explore_title: String,
    pub theme: PageTheme,
    pub accounts: BTreeMap<String, Account>,
    pub posts: BTreeMap<String, Post>,
    /// Per OS actor, the handles they follow. `ctx.actor` is the identity across every site.
    pub follows: BTreeMap<String, BTreeSet<String>>,
    /// Per post, the actors who liked it; the seeded `Post::likes` is the crowd behind them.
    pub likes: BTreeMap<String, BTreeSet<String>>,
    pub reposts: BTreeMap<String, BTreeSet<String>>,
    pub connections: BTreeMap<String, BTreeSet<String>>,
    pub messages: BTreeMap<String, Conversation>,
    pub next_id: u64,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Account {
    pub handle: String,
    pub name: String,
    /// The OS user this account belongs to, or `null` for someone with no machine in this world.
    pub actor: Option<String>,
    /// Federated instances render `@handle@domain`; a single-instance site leaves this empty.
    pub domain: String,
    pub bio: String,
    pub headline: String,
    pub location: String,
    pub followers: u64,
    pub following: u64,
    pub verified: bool,
    pub experience: Vec<Role>,
    pub site: String,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Role {
    pub title: String,
    pub company: String,
    pub period: String,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Post {
    pub id: String,
    pub author: String,
    pub text: String,
    pub tick: u64,
    pub likes: u64,
    pub reposts: u64,
    pub reply_to: Option<String>,
    pub quoted: Option<String>,
    /// `photos` mode only: what the picture shows, drawn as a labelled tile above the caption.
    /// Left out of the serialized state when empty so the other modes' state is unchanged.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub image: String,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Conversation {
    pub id: String,
    pub title: String,
    /// Handles in the thread; an actor may read it when one of them is their account.
    pub members: BTreeSet<String>,
    pub messages: Vec<Dm>,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Dm {
    pub id: String,
    pub from: String,
    pub text: String,
    pub tick: u64,
}
impl SocialState {
    pub fn professional(&self) -> bool {
        self.mode == "professional"
    }
    pub fn photos(&self) -> bool {
        self.mode == "photos"
    }
    pub fn account_of(&self, actor: &str) -> Option<&Account> {
        self.accounts
            .values()
            .find(|a| a.actor.as_deref() == Some(actor))
    }
    fn handle_of(&self, actor: &str) -> Result<String, String> {
        self.account_of(actor)
            .map(|a| a.handle.clone())
            .ok_or_else(|| format!("no account on this site for {actor}"))
    }
    pub fn likes_of(&self, id: &str) -> u64 {
        let seeded = self.posts.get(id).map_or(0, |p| p.likes);
        seeded + self.likes.get(id).map_or(0, BTreeSet::len) as u64
    }
    pub fn reposts_of(&self, id: &str) -> u64 {
        let seeded = self.posts.get(id).map_or(0, |p| p.reposts);
        seeded + self.reposts.get(id).map_or(0, BTreeSet::len) as u64
    }
    fn mint(&mut self, prefix: &str) -> Result<String, String> {
        loop {
            self.next_id = self.next_id.checked_add(1).ok_or("ID space exhausted")?;
            let id = format!("{prefix}-{}", self.next_id);
            if !self.posts.contains_key(&id) {
                return Ok(id);
            }
        }
    }
    /// Newest first, with the id as the tiebreak so two posts on one tick still order totally.
    fn newest(&self, mut ids: Vec<String>) -> Vec<String> {
        ids.sort_by(|a, b| {
            let (x, y) = (&self.posts[a], &self.posts[b]);
            y.tick.cmp(&x.tick).then_with(|| y.id.cmp(&x.id))
        });
        ids
    }
    fn roots(&self) -> impl Iterator<Item = &Post> {
        self.posts.values().filter(|p| p.reply_to.is_none())
    }
    /// Home: the accounts this actor follows plus their own voice. Nobody followed, or no account
    /// at all, falls back to the public timeline rather than an empty page.
    pub fn home(&self, actor: &str) -> Vec<String> {
        let mut seen: BTreeSet<String> = self.follows.get(actor).cloned().unwrap_or_default();
        if let Some(a) = self.account_of(actor) {
            seen.insert(a.handle.clone());
        }
        let ids: Vec<String> = self
            .roots()
            .filter(|p| seen.contains(&p.author))
            .map(|p| p.id.clone())
            .collect();
        if ids.is_empty() {
            self.explore()
        } else {
            self.newest(ids)
        }
    }
    /// Explore ranks by reach, which is what makes it a different page from Home.
    pub fn explore(&self) -> Vec<String> {
        let mut ids: Vec<String> = self.roots().map(|p| p.id.clone()).collect();
        ids.sort_by(|a, b| {
            let reach = |id: &str| self.likes_of(id) + self.reposts_of(id);
            let (x, y) = (reach(a), reach(b));
            y.cmp(&x)
                .then_with(|| self.posts[b].tick.cmp(&self.posts[a].tick))
                .then_with(|| a.cmp(b))
        });
        ids
    }
    pub fn by_author(&self, handle: &str) -> Vec<String> {
        self.newest(
            self.posts
                .values()
                .filter(|p| p.author == handle)
                .map(|p| p.id.clone())
                .collect(),
        )
    }
    pub fn replies_to(&self, id: &str) -> Vec<String> {
        let mut ids: Vec<String> = self
            .posts
            .values()
            .filter(|p| p.reply_to.as_deref() == Some(id))
            .map(|p| p.id.clone())
            .collect();
        ids.sort_by(|a, b| {
            let (x, y) = (&self.posts[a], &self.posts[b]);
            x.tick.cmp(&y.tick).then_with(|| x.id.cmp(&y.id))
        });
        ids
    }
    /// Posts, handles and display names, matched case-insensitively. Newest first.
    pub fn search(&self, q: &str) -> Vec<String> {
        let needle = q.trim().to_lowercase();
        if needle.is_empty() {
            return vec![];
        }
        self.newest(
            self.posts
                .values()
                .filter(|p| {
                    let name = self.accounts.get(&p.author).map(|a| a.name.to_lowercase());
                    p.text.to_lowercase().contains(&needle)
                        || p.author.to_lowercase().contains(&needle)
                        || name.is_some_and(|n| n.contains(&needle))
                })
                .map(|p| p.id.clone())
                .collect(),
        )
    }
    pub fn inbox(&self, actor: &str) -> Vec<&Conversation> {
        let mine = self.account_of(actor).map(|a| a.handle.clone());
        self.messages
            .values()
            .filter(|c| mine.as_ref().is_some_and(|h| c.members.contains(h)))
            .collect()
    }
    fn conversation(&self, actor: &str, id: &str) -> Result<&Conversation, String> {
        let handle = self.handle_of(actor)?;
        self.messages
            .get(id)
            .filter(|c| c.members.contains(&handle))
            .ok_or_else(|| "conversation unavailable".into())
    }
    pub fn compose(
        &mut self,
        actor: &str,
        text: &str,
        reply_to: Option<&str>,
        quoted: Option<&str>,
        tick: u64,
    ) -> Result<Post, String> {
        let author = self.handle_of(actor)?;
        let text = text.trim();
        if text.is_empty() {
            return Err("post text required".into());
        }
        if text.chars().count() > MAX_POST {
            return Err(format!("post is longer than {MAX_POST} characters"));
        }
        for parent in reply_to.iter().chain(quoted.iter()) {
            if !self.posts.contains_key(*parent) {
                return Err(format!("no post {parent}"));
            }
        }
        let post = Post {
            id: self.mint("p")?,
            author,
            text: text.into(),
            tick,
            likes: 0,
            reposts: 0,
            reply_to: reply_to.map(str::to_owned),
            quoted: quoted.map(str::to_owned),
            image: String::new(),
        };
        self.posts.insert(post.id.clone(), post.clone());
        Ok(post)
    }
    /// Like and repost are toggles, because a real one is: the second tap takes it back.
    fn toggle(table: &mut BTreeMap<String, BTreeSet<String>>, id: &str, actor: &str) -> bool {
        let set = table.entry(id.into()).or_default();
        let on = set.insert(actor.into());
        if !on {
            set.remove(actor);
        }
        on
    }
    pub fn like(&mut self, actor: &str, id: &str) -> Result<Value, String> {
        if !self.posts.contains_key(id) {
            return Err(format!("no post {id}"));
        }
        let on = Self::toggle(&mut self.likes, id, actor);
        Ok(json!({"id": id, "liked": on, "likes": self.likes_of(id)}))
    }
    pub fn repost(&mut self, actor: &str, id: &str) -> Result<Value, String> {
        if !self.posts.contains_key(id) {
            return Err(format!("no post {id}"));
        }
        let on = Self::toggle(&mut self.reposts, id, actor);
        Ok(json!({"id": id, "reposted": on, "reposts": self.reposts_of(id)}))
    }
    pub fn follow(&mut self, actor: &str, handle: &str) -> Result<Value, String> {
        if !self.accounts.contains_key(handle) {
            return Err(format!("no account @{handle}"));
        }
        if self.account_of(actor).is_some_and(|a| a.handle == handle) {
            return Err("an account cannot follow itself".into());
        }
        let on = Self::toggle(&mut self.follows, actor, handle);
        // The follower counter is part of the page, so it has to move with the edge.
        if let Some(a) = self.accounts.get_mut(handle) {
            a.followers = if on {
                a.followers.saturating_add(1)
            } else {
                a.followers.saturating_sub(1)
            };
        }
        Ok(json!({"handle": handle, "following": on}))
    }
    pub fn connect(&mut self, actor: &str, handle: &str) -> Result<Value, String> {
        if !self.professional() {
            return Err("connections are a professional-mode feature".into());
        }
        if !self.accounts.contains_key(handle) {
            return Err(format!("no account {handle}"));
        }
        if self.account_of(actor).is_some_and(|a| a.handle == handle) {
            return Err("an account cannot connect to itself".into());
        }
        let on = Self::toggle(&mut self.connections, actor, handle);
        Ok(json!({"handle": handle, "invited": on}))
    }
    pub fn send(&mut self, actor: &str, thread: &str, text: &str, tick: u64) -> Result<Dm, String> {
        self.conversation(actor, thread)?;
        let from = self.handle_of(actor)?;
        let text = text.trim();
        if text.is_empty() {
            return Err("message text required".into());
        }
        self.next_id = self.next_id.checked_add(1).ok_or("ID space exhausted")?;
        let dm = Dm {
            id: format!("dm-{}", self.next_id),
            from,
            text: text.into(),
            tick,
        };
        self.messages
            .get_mut(thread)
            .ok_or("conversation unavailable")?
            .messages
            .push(dm.clone());
        Ok(dm)
    }
}
/// The one router: every GET and every re-render after a successful form POST goes through here.
fn view(
    s: &SocialState,
    ctx: &ServiceContext,
    path: &str,
    q: Option<&str>,
) -> SimResult<HttpResponse> {
    let parts: Vec<&str> = path.trim_matches('/').split('/').collect();
    let at = |page| View::new(s, ctx, path, page);
    match parts.as_slice() {
        [""] => at("home").timeline(true),
        ["explore"] | ["local"] => View::new(s, ctx, "/explore", "explore").timeline(false),
        ["search"] => at("search").search(q.unwrap_or_default()),
        ["messages"] => at("inbox").inbox("/messages", None),
        ["messaging"] => at("inbox").inbox("/messaging", None),
        ["messages", id] => at("inbox").inbox("/messages", Some(id)),
        ["messaging", id] => at("inbox").inbox("/messaging", Some(id)),
        [handle] => at("profile").profile(handle),
        [handle, "status", id] => at("thread").thread(handle, id),
        _ => web::error(404, "route not found"),
    }
}
impl Service for SocialService {
    fn kind(&self) -> &str {
        "social"
    }
    fn initialize(&self, initial: Value, _: &ServiceContext) -> SimResult<Value> {
        let gated = web::shape(initial, OBJECTS, ARRAYS)?;
        let mode = web::variant(&gated, "mode", MODES)?;
        let mut s: SocialState = web::load(&gated)?;
        s.mode = mode;
        if !s.skin.is_empty() && !SKINS.contains(&s.skin.as_str()) {
            return Err(SimError::invalid(format!(
                "unknown skin {}; expected one of {}",
                s.skin,
                SKINS.join(", ")
            )));
        }
        for (key, a) in &s.accounts {
            if a.handle != *key {
                return Err(SimError::invalid(format!(
                    "account {key} carries handle {}",
                    a.handle
                )));
            }
        }
        for (key, p) in &s.posts {
            if p.id != *key {
                return Err(SimError::invalid(format!("post {key} carries id {}", p.id)));
            }
            if !s.accounts.contains_key(&p.author) {
                return Err(SimError::invalid(format!(
                    "post {key} is by unknown handle {}",
                    p.author
                )));
            }
            for parent in p.reply_to.iter().chain(p.quoted.iter()) {
                if !s.posts.contains_key(parent) {
                    return Err(SimError::invalid(format!("post {key} points at {parent}")));
                }
            }
        }
        for c in s.messages.values() {
            for m in &c.members {
                if !s.accounts.contains_key(m) {
                    return Err(SimError::invalid(format!(
                        "conversation {} includes unknown handle {m}",
                        c.id
                    )));
                }
            }
        }
        Ok(serde_json::to_value(s)?)
    }
    fn handle(
        &self,
        state: &mut Value,
        ctx: &ServiceContext,
        r: &HttpRequest,
    ) -> SimResult<HttpResponse> {
        let mut s: SocialState = web::load(state)?;
        let p = web::path(r);
        let api = p.starts_with("/api/") || p == "/api";
        let routed = p.strip_prefix("/api").unwrap_or(&p);
        let parts: Vec<&str> = routed.trim_matches('/').split('/').collect();
        let method = r.method.to_ascii_uppercase();
        if method == "GET" {
            if !api {
                return view(&s, ctx, &p, web::query(r, "q").as_deref());
            }
            return match parts.as_slice() {
                ["timeline"] => HttpResponse::json(200, &s.home(&ctx.actor)),
                ["explore"] => HttpResponse::json(200, &s.explore()),
                ["search"] => {
                    HttpResponse::json(200, &s.search(&web::query(r, "q").unwrap_or_default()))
                }
                ["accounts"] => HttpResponse::json(200, &s.accounts),
                ["accounts", h] => web::domain(
                    s.accounts
                        .get(*h)
                        .ok_or_else(|| format!("no account {h}"))
                        .map(|a| json!(a)),
                ),
                ["posts", id] => {
                    web::domain(s.posts.get(*id).ok_or_else(|| format!("no post {id}")).map(
                        |post| {
                            json!({"post": post, "likes": s.likes_of(id),
                                   "reposts": s.reposts_of(id), "replies": s.replies_to(id)})
                        },
                    ))
                }
                ["messages"] => HttpResponse::json(200, &s.inbox(&ctx.actor)),
                _ => web::error(404, "route not found"),
            };
        }
        if method != "POST" {
            return web::error(405, "method not allowed");
        }
        let b = web::body(r)?;
        // The search form is a POST that reads; nothing else on this site is.
        if !api && parts.as_slice() == ["search"] {
            return view(&s, ctx, "/search", Some(&web::text(&b, "q")));
        }
        let back = match web::text(&b, "view") {
            v if v.is_empty() => None,
            v => Some(v),
        };
        let (result, fallback) = match parts.as_slice() {
            ["posts"] => {
                let reply = web::text(&b, "reply_to");
                let quote = web::text(&b, "quoted");
                let r = s.compose(
                    &ctx.actor,
                    &web::text(&b, "text"),
                    (!reply.is_empty()).then_some(reply.as_str()),
                    (!quote.is_empty()).then_some(quote.as_str()),
                    ctx.tick,
                );
                let at = r.as_ref().map_or_else(
                    |_| "/".to_owned(),
                    |p| format!("/{}/status/{}", p.author, p.id),
                );
                (r.map(|p| json!(p)), at)
            }
            ["posts", id, "replies"] => {
                let parent = s.posts.get(*id).map(|p| p.author.clone());
                let at = parent.map_or("/".into(), |h| format!("/{h}/status/{id}"));
                (
                    s.compose(&ctx.actor, &web::text(&b, "text"), Some(id), None, ctx.tick)
                        .map(|p| json!(p)),
                    at,
                )
            }
            ["posts", id, "like"] => (s.like(&ctx.actor, id), "/".into()),
            ["posts", id, "repost"] => (s.repost(&ctx.actor, id), "/".into()),
            ["accounts", h, "follow"] => (s.follow(&ctx.actor, h), format!("/{h}")),
            ["accounts", h, "connect"] => (s.connect(&ctx.actor, h), format!("/{h}")),
            ["messages"] => {
                let thread = web::text(&b, "thread");
                (
                    s.send(&ctx.actor, &thread, &web::text(&b, "text"), ctx.tick)
                        .map(|m| json!(m)),
                    format!("/messages/{thread}"),
                )
            }
            ["messages", id, "messages"] | ["messaging", id, "messages"] => (
                s.send(&ctx.actor, id, &web::text(&b, "text"), ctx.tick)
                    .map(|m| json!(m)),
                format!("/{}/{id}", parts[0]),
            ),
            _ => return web::error(404, "route not found"),
        };
        if result.is_err() {
            return web::domain(result);
        }
        web::save(state, &s)?;
        if api {
            return web::domain(result);
        }
        let target = back.unwrap_or(fallback);
        let (path, q) = match target.split_once('?') {
            Some((p, rest)) => (p.to_owned(), rest.strip_prefix("q=").map(str::to_owned)),
            None => (target, None),
        };
        view(&s, ctx, &path, q.as_deref())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn ctx(actor: &str) -> ServiceContext {
        ServiceContext {
            actor: actor.into(),
            source: "alice-mac".into(),
            tick: 7,
            seed: 1,
            instance: "x-social".into(),
        }
    }
    fn seed() -> Value {
        json!({
            "mode": "microblog",
            "brand": "X",
            "theme": {"accent": "#1d9bf0"},
            "accounts": {
                "alicechen": {"handle": "alicechen", "name": "Alice Chen", "actor": "alice",
                              "bio": "Systems.", "followers": 2841, "verified": true},
                "tweber": {"handle": "tweber", "name": "Tom Weber", "bio": "The Verge.",
                           "followers": 40122}
            },
            "posts": {
                "p-1001": {"id": "p-1001", "author": "alicechen", "text": "Atlas 1.0 is out.",
                           "tick": 1, "likes": 412, "reposts": 88},
                "p-1002": {"id": "p-1002", "author": "tweber", "text": "Anyone at Northstar free?",
                           "tick": 2, "likes": 12}
            },
            "follows": {"alice": ["tweber"]},
            "messages": {"c-1": {"id": "c-1", "title": "Comment?",
                                 "members": ["alicechen", "tweber"],
                                 "messages": [{"id": "dm-1", "from": "tweber",
                                               "text": "Got a minute?", "tick": 1}]}},
            "next_id": 1002
        })
    }
    fn live() -> Value {
        SocialService.initialize(seed(), &ctx("alice")).unwrap()
    }
    fn post(state: &mut Value, actor: &str, url: &str, body: &str) -> HttpResponse {
        let mut r = HttpRequest::get(url);
        r.method = "POST".into();
        r.headers.insert(
            "content-type".into(),
            "application/x-www-form-urlencoded".into(),
        );
        r.body = body.as_bytes().to_vec();
        SocialService.handle(state, &ctx(actor), &r).unwrap()
    }
    fn html(r: &HttpResponse) -> (String, cw_web::dom::Document) {
        assert_eq!(r.header("content-type"), Some(web::html::HTML_MEDIA_TYPE));
        let body = String::from_utf8(r.body.clone()).unwrap();
        web::html::validate_strict(&body).unwrap_or_else(|e| panic!("{e:?}"));
        let doc = cw_web::html::parse(&body);
        (body, doc)
    }
    fn one(doc: &cw_web::dom::Document, id: &str) -> cw_web::dom::NodeId {
        *doc.by_id(id).first().unwrap_or_else(|| panic!("no element #{id}"))
    }
    /// Every page an agent can reach must survive the strict validator: unique ids and only
    /// HTML and CSS the engine renders. A duplicate id would make a control ambiguous to click.
    #[test]
    fn every_route_renders_a_valid_page() {
        let mut state = live();
        for url in [
            "http://x.com/",
            "http://x.com/explore",
            "http://x.com/search?q=atlas",
            "http://x.com/messages",
            "http://x.com/messages/c-1",
            "http://x.com/alicechen",
            "http://x.com/alicechen/status/p-1001",
        ] {
            let r = SocialService
                .handle(&mut state, &ctx("alice"), &HttpRequest::get(url))
                .unwrap();
            assert_eq!(r.status, 200, "{url}");
            html(&r);
        }
    }
    /// The ids, forms and links the `Page` version exposed are the agent API; each is on the
    /// element that plays the same role.
    #[test]
    fn the_ids_forms_and_links_an_agent_uses_are_where_they_were() {
        let mut state = live();
        let get = |state: &mut Value, url: &str| {
            SocialService
                .handle(state, &ctx("alice"), &HttpRequest::get(url))
                .unwrap()
        };
        let (_, doc) = html(&get(&mut state, "http://x.com/"));
        for (id, href) in [
            ("brand", "/"),
            ("nav-home", "/"),
            ("nav-explore", "/explore"),
            ("nav-search", "/search"),
            ("nav-inbox", "/messages"),
            ("nav-me", "/alicechen"),
            ("post-p-1002", "/tweber/status/p-1002"),
            ("p-1002-replies", "/tweber/status/p-1002"),
            ("p-1002-name", "/tweber"),
        ] {
            let node = one(&doc, id);
            assert!(doc.is(node, "a"), "{id} is a link");
            assert_eq!(doc.attr(node, "href"), Some(href), "{id}");
        }
        assert_eq!(doc.text_content(one(&doc, "nav-home-text")), "Home");
        assert_eq!(doc.text_content(one(&doc, "p-1002-text")), "Anyone at Northstar free?");
        assert_eq!(doc.text_content(one(&doc, "p-1002-handle")), "@tweber · 5t");
        assert!(doc.by_id("p-1001-verified").len() == 1 && doc.by_id("p-1002-verified").is_empty());
        let compose = one(&doc, "compose");
        assert_eq!(doc.attr(compose, "action"), Some("/posts"));
        assert_eq!(doc.attr(compose, "method"), Some("post"));
        assert_eq!(doc.attr(one(&doc, "compose-text"), "name"), Some("text"));
        assert!(doc.is(one(&doc, "compose-submit"), "button"));
        // Like and repost are one-button forms that carry the page to come back to.
        for (id, action, label) in [
            ("p-1002-like", "/posts/p-1002/like", "Like 12"),
            ("p-1002-repost", "/posts/p-1002/repost", "Repost 0"),
        ] {
            let b = one(&doc, id);
            assert!(doc.is(b, "button"));
            assert_eq!(doc.attr(b, "aria-label"), Some(label));
            assert_eq!(doc.text_content(one(&doc, &format!("{id}-label"))), label);
            let f = one(&doc, &format!("{id}-form"));
            assert_eq!(doc.attr(f, "action"), Some(action));
            assert_eq!(doc.attr(f, "method"), Some("post"));
            let view = doc
                .descendants(f)
                .find(|n| doc.attr(*n, "name") == Some("view"))
                .unwrap();
            assert_eq!(doc.attr(view, "value"), Some("/"));
        }
        let (_, doc) = html(&get(&mut state, "http://x.com/tweber"));
        assert_eq!(doc.text_content(one(&doc, "name")), "Tom Weber");
        assert_eq!(doc.text_content(one(&doc, "handle")), "@tweber");
        assert_eq!(doc.text_content(one(&doc, "followers")), "40122 followers");
        assert_eq!(doc.text_content(one(&doc, "follow-label")), "Following");
        assert_eq!(doc.attr(one(&doc, "follow-form"), "action"), Some("/accounts/tweber/follow"));
        let (_, doc) = html(&get(&mut state, "http://x.com/alicechen"));
        assert!(doc.by_id("follow").is_empty(), "nobody follows themselves");
        let (_, doc) = html(&get(&mut state, "http://x.com/alicechen/status/p-1001"));
        assert_eq!(doc.attr(one(&doc, "reply"), "action"), Some("/posts/p-1001/replies"));
        assert_eq!(doc.attr(one(&doc, "reply-text"), "name"), Some("text"));
        assert!(doc.is(one(&doc, "reply-submit"), "button"));
        assert_eq!(doc.text_content(one(&doc, "replies-title")), "0 replies");
        let (_, doc) = html(&get(&mut state, "http://x.com/search?q=atlas"));
        let search = one(&doc, "search");
        assert_eq!(doc.attr(search, "action"), Some("/search"));
        assert_eq!(doc.attr(search, "method"), Some("post"));
        assert_eq!(doc.attr(one(&doc, "search-q"), "name"), Some("q"));
        assert_eq!(doc.attr(one(&doc, "search-q"), "value"), Some("atlas"));
        assert_eq!(doc.text_content(one(&doc, "feed-title")), "1 result for \"atlas\"");
        let (_, doc) = html(&get(&mut state, "http://x.com/messages/c-1"));
        assert_eq!(doc.attr(one(&doc, "thread-c-1"), "href"), Some("/messages/c-1"));
        assert_eq!(doc.text_content(one(&doc, "open-title")), "Comment?");
        assert_eq!(doc.text_content(one(&doc, "dm-dm-1-text")), "Got a minute?");
        assert_eq!(doc.attr(one(&doc, "dm"), "action"), Some("/messages/c-1/messages"));
        assert_eq!(doc.attr(one(&doc, "dm-text"), "name"), Some("text"));
        assert!(doc.is(one(&doc, "dm-submit"), "button"));
    }
    /// A seed may name its skin; otherwise the brand, and then the mode, picks one. Every
    /// skin renders every route through the strict validator.
    #[test]
    fn every_skin_renders_every_route_strictly() {
        assert!(SocialService
            .initialize(json!({"skin": "nonesuch"}), &ctx("alice"))
            .is_err());
        for (brand, mode, expect) in [
            ("X", "microblog", "x"),
            ("Bluesky", "microblog", "bsky"),
            ("mastodon.social", "microblog", "mastodon"),
            ("Facebook", "microblog", "facebook"),
            ("Instagram", "photos", "instagram"),
            ("LinkedIn", "professional", "linkedin"),
            ("Pinterest", "photos", "pinterest"),
            ("Somewhere", "photos", "instagram"),
            ("Somewhere", "professional", "linkedin"),
            ("Somewhere", "microblog", "x"),
        ] {
            let mut seed = seed();
            seed["brand"] = json!(brand);
            seed["mode"] = json!(mode);
            seed["posts"]["p-1001"]["image"] = json!("A harbour at dusk");
            seed["posts"]["p-1003"] = json!({"id": "p-1003", "author": "tweber", "text": "Quoting.",
                                             "tick": 3, "quoted": "p-1001", "reply_to": "p-1002"});
            let mut state = SocialService.initialize(seed, &ctx("alice")).unwrap();
            let s: SocialState = web::load(&state).unwrap();
            assert_eq!(skin_of(&s), expect, "{brand} in {mode}");
            for path in ["/", "/explore", "/local", "/search?q=atlas", "/search", "/messages",
                         "/messaging", "/messages/c-1", "/messaging/c-1",
                         "/alicechen", "/tweber", "/tweber/status/p-1003"] {
                for actor in ["alice", "carol"] {
                    let r = SocialService
                        .handle(&mut state, &ctx(actor), &HttpRequest::get(format!("http://site.example{path}")))
                        .unwrap();
                    if actor == "carol" && (path.starts_with("/messaging/") || path.starts_with("/messages/")) {
                        assert_eq!(r.status, 403);
                        continue;
                    }
                    assert_eq!(r.status, 200, "{brand} {path}");
                    let (body, _) = html(&r);
                    assert!(body.contains(&format!("skin-{expect} mode-{mode}")), "{brand} {path}");
                }
            }
        }
        let mut named = seed();
        named["skin"] = json!("mastodon");
        let state = SocialService.initialize(named, &ctx("alice")).unwrap();
        assert_eq!(skin_of(&web::load::<SocialState>(&state).unwrap()), "mastodon");
        assert_eq!(state["skin"], "mastodon");
        assert!(live().get("skin").is_none(), "an unnamed skin stays out of the state");
    }
    #[test]
    fn seed_shape_and_references_are_gated_at_load() {
        assert!(SocialService.initialize(json!([]), &ctx("alice")).is_err());
        assert!(SocialService
            .initialize(json!({"accounts": []}), &ctx("alice"))
            .is_err());
        assert!(SocialService
            .initialize(json!({"mode": "nonesuch"}), &ctx("alice"))
            .is_err());
        assert!(SocialService
            .initialize(
                json!({"posts": {"p-1": {"id": "p-1", "author": "ghost"}}}),
                &ctx("alice")
            )
            .is_err());
        assert!(SocialService.initialize(Value::Null, &ctx("alice")).is_ok());
    }
    #[test]
    fn timeline_renders_followed_accounts_and_never_mutates() {
        let mut state = live();
        let before = state.clone();
        let page = SocialService
            .handle(
                &mut state,
                &ctx("alice"),
                &HttpRequest::get("http://x.com/"),
            )
            .unwrap();
        assert_eq!(before, state, "a render must not mutate state");
        let body = String::from_utf8(page.body).unwrap();
        assert!(body.contains("Atlas 1.0 is out."), "own post is on home");
        assert!(body.contains("Anyone at Northstar free?"), "followed post");
        assert!(
            body.contains("<form id=\"p-1002-like-form\" action=\"/posts/p-1002/like\" method=\"post\""),
            "like is a real control"
        );
    }
    #[test]
    fn twitter_alias_and_relative_routes_reach_the_same_pages() {
        let mut state = live();
        let a = SocialService
            .handle(
                &mut state,
                &ctx("alice"),
                &HttpRequest::get("http://x.com/tweber"),
            )
            .unwrap();
        let b = SocialService
            .handle(
                &mut state,
                &ctx("alice"),
                &HttpRequest::get("http://mobile.twitter.com/tweber"),
            )
            .unwrap();
        assert_eq!(a, b, "the site answers on every alias identically");
        assert_eq!(
            SocialService
                .handle(
                    &mut state,
                    &ctx("alice"),
                    &HttpRequest::get("http://x.com/nobody")
                )
                .unwrap()
                .status,
            404
        );
    }
    #[test]
    fn posting_liking_reposting_and_following_all_mutate() {
        let mut state = live();
        let page = post(
            &mut state,
            "alice",
            "http://x.com/posts",
            "text=Shipping+notes&view=/",
        );
        assert_eq!(page.status, 200);
        assert!(String::from_utf8(page.body)
            .unwrap()
            .contains("Shipping notes"));
        let s: SocialState = web::load(&state).unwrap();
        assert_eq!(s.posts.len(), 3);
        assert_eq!(s.next_id, 1003);
        post(
            &mut state,
            "alice",
            "http://x.com/posts/p-1002/like",
            "view=/",
        );
        let s: SocialState = web::load(&state).unwrap();
        assert_eq!(s.likes_of("p-1002"), 13, "a like adds to the seeded crowd");
        post(
            &mut state,
            "alice",
            "http://x.com/posts/p-1002/like",
            "view=/",
        );
        let s: SocialState = web::load(&state).unwrap();
        assert_eq!(s.likes_of("p-1002"), 12, "and taps off again");
        post(
            &mut state,
            "alice",
            "http://x.com/posts/p-1002/repost",
            "view=/",
        );
        assert_eq!(
            web::load::<SocialState>(&state)
                .unwrap()
                .reposts_of("p-1002"),
            1
        );
        post(
            &mut state,
            "alice",
            "http://x.com/accounts/tweber/follow",
            "view=/tweber",
        );
        let s: SocialState = web::load(&state).unwrap();
        assert!(
            !s.follows["alice"].contains("tweber"),
            "a second tap unfollows"
        );
        assert_eq!(s.accounts["tweber"].followers, 40121);
    }
    #[test]
    fn replies_build_a_chain_and_land_on_the_thread() {
        let mut state = live();
        let page = post(
            &mut state,
            "alice",
            "http://x.com/posts/p-1002/replies",
            "text=Happy+to+talk",
        );
        let body = String::from_utf8(page.body).unwrap();
        assert!(
            body.contains("Anyone at Northstar free?"),
            "parent is in view"
        );
        assert!(body.contains("Happy to talk"));
        let s: SocialState = web::load(&state).unwrap();
        let reply = s
            .posts
            .values()
            .find(|p| p.text == "Happy to talk")
            .unwrap();
        assert_eq!(reply.reply_to.as_deref(), Some("p-1002"));
        assert_eq!(s.replies_to("p-1002"), vec![reply.id.clone()]);
    }
    #[test]
    fn refusals_are_real_and_leave_state_alone() {
        let mut state = live();
        let before = state.clone();
        assert_eq!(
            post(&mut state, "alice", "http://x.com/posts", "text=%20").status,
            400
        );
        assert_eq!(
            post(&mut state, "carol", "http://x.com/posts", "text=hello").status,
            400,
            "an actor with no account here cannot post"
        );
        assert_eq!(
            post(
                &mut state,
                "alice",
                "http://x.com/posts/p-9999/like",
                "view=/"
            )
            .status,
            400
        );
        assert_eq!(
            post(
                &mut state,
                "alice",
                "http://x.com/accounts/alicechen/follow",
                ""
            )
            .status,
            400
        );
        assert_eq!(
            post(
                &mut state,
                "alice",
                "http://x.com/accounts/tweber/connect",
                ""
            )
            .status,
            400,
            "connections are professional-mode only"
        );
        assert_eq!(before, state, "a refusal writes nothing");
    }
    #[test]
    fn direct_messages_are_membership_scoped() {
        let mut state = live();
        let page = post(
            &mut state,
            "alice",
            "http://x.com/messages/c-1/messages",
            "text=Sure,+quote+me",
        );
        assert!(String::from_utf8(page.body)
            .unwrap()
            .contains("Sure, quote me"));
        assert_eq!(
            web::load::<SocialState>(&state).unwrap().messages["c-1"]
                .messages
                .len(),
            2
        );
        let mut r = HttpRequest::get("http://x.com/messages/c-1/messages");
        r.method = "POST".into();
        r.headers.insert(
            "content-type".into(),
            "application/x-www-form-urlencoded".into(),
        );
        r.body = b"text=hi".to_vec();
        assert_eq!(
            SocialService
                .handle(&mut state, &ctx("bob"), &r)
                .unwrap()
                .status,
            400,
            "bob has no account on this instance"
        );
    }
    #[test]
    fn state_survives_a_snapshot_round_trip() {
        let mut state = live();
        post(&mut state, "alice", "http://x.com/posts", "text=Round+trip");
        post(
            &mut state,
            "alice",
            "http://x.com/posts/p-1001/like",
            "view=/",
        );
        let bytes = serde_json::to_vec(&state).unwrap();
        let mut restored: Value = serde_json::from_slice(&bytes).unwrap();
        let home = HttpRequest::get("http://x.com/");
        assert_eq!(
            SocialService
                .handle(&mut state, &ctx("alice"), &home)
                .unwrap(),
            SocialService
                .handle(&mut restored, &ctx("alice"), &home)
                .unwrap()
        );
        let s: SocialState = web::load(&restored).unwrap();
        assert!(s.posts.values().any(|p| p.text == "Round trip"));
        assert_eq!(s.likes_of("p-1001"), 413);
    }
    #[test]
    fn api_mirrors_the_pages_as_json() {
        let mut state = live();
        let r = SocialService
            .handle(
                &mut state,
                &ctx("alice"),
                &HttpRequest::get("http://x.com/api/search?q=atlas"),
            )
            .unwrap();
        assert_eq!(r.header("content-type"), Some("application/json"));
        assert_eq!(
            serde_json::from_slice::<Vec<String>>(&r.body).unwrap(),
            vec!["p-1001".to_owned()]
        );
        let mut like = HttpRequest::get("http://x.com/api/posts/p-1001/like");
        like.method = "POST".into();
        let r = SocialService
            .handle(&mut state, &ctx("alice"), &like)
            .unwrap();
        assert_eq!(r.header("content-type"), Some("application/json"));
        assert_eq!(
            serde_json::from_slice::<Value>(&r.body).unwrap()["likes"],
            json!(413)
        );
    }
    #[test]
    fn professional_mode_adds_connections_and_experience() {
        let seed = json!({
            "mode": "professional", "brand": "LinkedIn",
            "accounts": {
                "bob-martinez": {"handle": "bob-martinez", "name": "Bob Martinez", "actor": "bob",
                                 "headline": "Senior Engineer at Northstar",
                                 "experience": [{"title": "Senior Engineer", "company": "Northstar",
                                                 "period": "2023 - present"}]},
                "nadia-fischer": {"handle": "nadia-fischer", "name": "Nadia Fischer",
                                  "headline": "Talent at Meridian Labs"}
            }
        });
        let mut state = SocialService.initialize(seed, &ctx("bob")).unwrap();
        let page = SocialService
            .handle(
                &mut state,
                &ctx("bob"),
                &HttpRequest::get("http://linkedin.com/bob-martinez"),
            )
            .unwrap();
        let body = String::from_utf8(page.body).unwrap();
        assert!(body.contains("2023 - present"), "experience renders");
        let r = post(
            &mut state,
            "bob",
            "http://linkedin.com/accounts/nadia-fischer/connect",
            "view=/nadia-fischer",
        );
        assert_eq!(r.status, 200);
        assert!(String::from_utf8(r.body)
            .unwrap()
            .contains("Invitation sent"));
        assert!(
            web::load::<SocialState>(&state).unwrap().connections["bob"].contains("nadia-fischer")
        );
    }
}
