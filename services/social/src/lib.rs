//! Feeds: x.com and mastodon.social (`mode: microblog`), linkedin.com (`mode: professional`),
//! instagram.com and pinterest.com (`mode: photos`: every post is a picture with a caption).
//! Handles are world-wide identities, so every account maps to an OS actor or to nobody.
//!
//! One router renders every GET, and a successful form POST re-renders through the same router,
//! so a control always lands the caller on a real page instead of a JSON blob. `/api/*` mirrors
//! the mutating routes for agents that would rather read JSON.
use cw_protocol::{
    HttpRequest, HttpResponse, PageAction, PageElement, PageTheme, Result as SimResult, SimError,
};
use cw_sdk::{Registry, Service, ServiceContext};
use cw_service_common as web;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
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
/// Palette pulled once per render; a theme may fill none, some or all of it.
struct Palette {
    accent: String,
    ink: String,
    muted: String,
    surface: String,
    line: String,
}
impl Palette {
    fn of(theme: &PageTheme) -> Self {
        let muted = theme.muted.clone().unwrap_or_else(|| "#536471".into());
        Self {
            accent: theme.accent.clone().unwrap_or_else(|| "#1d9bf0".into()),
            ink: theme.ink.clone().unwrap_or_else(|| "#0f1419".into()),
            // A hairline drawn from the muted ink, so no theme needs a key for it. An already
            // translucent muted colour is used as-is rather than grown past nine characters.
            line: match muted.len() {
                7 => format!("{muted}44"),
                _ => muted.clone(),
            },
            muted,
            surface: theme.surface.clone().unwrap_or_else(|| "#ffffff".into()),
        }
    }
}
const TINTS: [&str; 6] = [
    "#1d9bf0", "#f91880", "#00ba7c", "#7856ff", "#ff7a00", "#e0245e",
];
/// Avatar colour is a pure FNV-1a over the handle: stable across runs, machines and snapshots,
/// and no seeded stream is reachable from a render.
fn tint(handle: &str) -> &'static str {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in handle.as_bytes() {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x100_0000_01b3);
    }
    TINTS[(h % TINTS.len() as u64) as usize]
}
fn initials(name: &str) -> String {
    let letters: String = name
        .split_whitespace()
        .filter_map(|w| w.chars().next())
        .take(2)
        .collect();
    if letters.is_empty() {
        "?".into()
    } else {
        letters.to_uppercase()
    }
}
/// Age in ticks, which is the only clock a service has.
fn ago(now: u64, tick: u64) -> String {
    match now.saturating_sub(tick) {
        0 => "now".into(),
        n => format!("{n}t"),
    }
}
/// A control that mutates and comes back to `view`; the browser posts the field set verbatim.
fn act(url: &str, view: &str) -> PageAction {
    PageAction {
        method: "POST".into(),
        url: url.into(),
        fields: BTreeMap::from([("view".into(), view.into())]),
    }
}
fn pill(id: &str, label: &str, on: bool, url: &str, view: &str, p: &Palette) -> PageElement {
    let style = web::style()
        .size(12)
        .medium()
        .padding(6)
        .radius(14)
        .background(if on {
            p.accent.clone()
        } else {
            p.surface.clone()
        })
        .border(p.line.clone());
    let ink = if on { "#ffffff" } else { p.muted.as_str() };
    web::card_action(
        id,
        style,
        act(url, view),
        vec![web::styled(
            &format!("{id}-label"),
            label,
            web::style().size(12).medium().color(ink).align("center"),
        )],
    )
}
fn avatar(id: &str, a: Option<&Account>, handle: &str, size: u32) -> PageElement {
    let name = a.map_or(handle, |a| a.name.as_str());
    web::thumbnail(
        id,
        initials(name),
        web::style()
            .width(size)
            .height(size)
            .radius(size / 2)
            .size(if size >= 44 { 15 } else { 12 })
            .background(tint(handle)),
    )
}
fn column(id: &str, gap: u32, children: Vec<PageElement>) -> PageElement {
    web::grid(id, 1, gap, children)
}
fn display(s: &SocialState, handle: &str) -> String {
    match s.accounts.get(handle) {
        Some(a) if !a.domain.is_empty() => format!("@{}@{}", a.handle, a.domain),
        _ => format!("@{handle}"),
    }
}
fn nav(s: &SocialState, p: &Palette, actor: &str, here: &str) -> Vec<PageElement> {
    let explore = if s.explore_title.is_empty() {
        "Explore"
    } else {
        s.explore_title.as_str()
    };
    let inbox = if s.professional() {
        "/messaging"
    } else {
        "/messages"
    };
    let mut items = vec![
        ("nav-home", "Home", "/".to_owned()),
        ("nav-explore", explore, "/explore".to_owned()),
        ("nav-search", "Search", "/search".to_owned()),
        ("nav-inbox", "Messages", inbox.to_owned()),
    ];
    if let Some(a) = s.account_of(actor) {
        items.push(("nav-me", "Profile", format!("/{}", a.handle)));
    }
    let links: Vec<PageElement> = items
        .into_iter()
        .map(|(id, label, url)| {
            let current = url == here;
            web::card_action(
                id,
                web::style()
                    .padding(8)
                    .radius(16)
                    .background(if current {
                        p.accent.clone()
                    } else {
                        p.surface.clone()
                    })
                    .border(p.line.clone()),
                web::visit(url),
                vec![web::styled(
                    &format!("{id}-text"),
                    label,
                    web::style()
                        .size(13)
                        .medium()
                        .align("center")
                        .color(if current { "#ffffff" } else { p.ink.as_str() }),
                )],
            )
        })
        .collect();
    vec![
        web::styled(
            "brand",
            &s.brand,
            web::style().size(26).bold().color(p.accent.clone()),
        ),
        web::grid("nav", links.len().min(5) as u32, 8, links),
        web::divider("nav-rule"),
    ]
}
/// One timeline entry. The whole card opens the thread; the pills inside it are their own
/// controls, so a like never costs a navigation.
fn post_card(
    s: &SocialState,
    ctx: &ServiceContext,
    p: &Palette,
    id: &str,
    here: &str,
) -> PageElement {
    let post = &s.posts[id];
    let a = s.accounts.get(&post.author);
    let liked = s.likes.get(id).is_some_and(|l| l.contains(&ctx.actor));
    let boosted = s.reposts.get(id).is_some_and(|r| r.contains(&ctx.actor));
    let mut head = vec![
        web::styled(
            &format!("{id}-name"),
            a.map_or(post.author.clone(), |a| a.name.clone()),
            web::style().size(15).bold().color(p.ink.clone()).width(160),
        ),
        web::styled(
            &format!("{id}-handle"),
            if s.professional() {
                a.map_or(String::new(), |a| a.headline.clone())
            } else {
                format!(
                    "{} · {}",
                    display(s, &post.author),
                    ago(ctx.tick, post.tick)
                )
            },
            web::style().size(13).color(p.muted.clone()).one_line(),
        ),
    ];
    if a.is_some_and(|a| a.verified) {
        head.push(web::badge(
            &format!("{id}-verified"),
            "Verified",
            web::style().size(10).width(64).background(p.accent.clone()),
        ));
    }
    let mut body = vec![web::row(&format!("{id}-head"), 8, "center", head)];
    if s.photos() {
        // The picture itself: a tile in the author's tint, named after what it shows.
        body.push(web::thumbnail(
            &format!("{id}-photo"),
            if post.image.is_empty() {
                "Photo"
            } else {
                post.image.as_str()
            },
            web::style()
                .height(180)
                .radius(10)
                .background(tint(&post.author))
                .color("#ffffff")
                .align("center"),
        ));
    }
    body.push(web::styled(
        &format!("{id}-text"),
        &post.text,
        web::style().size(15).color(p.ink.clone()),
    ));
    if let Some(q) = post.quoted.as_ref().filter(|q| s.posts.contains_key(*q)) {
        let quoted = &s.posts[q];
        body.push(web::card(
            &format!("{id}-quote"),
            web::style()
                .padding(10)
                .radius(12)
                .background(p.surface.clone())
                .border(p.line.clone()),
            vec![
                web::styled(
                    &format!("{id}-quote-by"),
                    display(s, &quoted.author),
                    web::style().size(12).medium().color(p.muted.clone()),
                ),
                web::styled(
                    &format!("{id}-quote-text"),
                    &quoted.text,
                    web::style().size(13).color(p.ink.clone()),
                ),
            ],
        ));
    }
    body.push(web::row(
        &format!("{id}-acts"),
        8,
        "center",
        vec![
            pill(
                &format!("{id}-like"),
                &format!(
                    "{} {}",
                    if liked { "Liked" } else { "Like" },
                    s.likes_of(id)
                ),
                liked,
                &format!("/posts/{id}/like"),
                here,
                p,
            ),
            pill(
                &format!("{id}-repost"),
                &format!(
                    "{} {}",
                    if boosted { "Reposted" } else { "Repost" },
                    s.reposts_of(id)
                ),
                boosted,
                &format!("/posts/{id}/repost"),
                here,
                p,
            ),
            web::link(
                &format!("{id}-replies"),
                format!("{} replies", s.replies_to(id).len()),
                format!("/{}/status/{id}", post.author),
            ),
        ],
    ));
    web::card_action(
        &format!("post-{id}"),
        web::style()
            .padding(14)
            .radius(14)
            .background(p.surface.clone())
            .border(p.line.clone()),
        web::visit(format!("/{}/status/{id}", post.author)),
        vec![web::row(
            &format!("{id}-row"),
            12,
            "start",
            vec![
                avatar(&format!("{id}-avatar"), a, &post.author, 44),
                column(&format!("{id}-body"), 6, body),
            ],
        )],
    )
}
fn feed(
    s: &SocialState,
    ctx: &ServiceContext,
    p: &Palette,
    title: &str,
    ids: &[String],
    here: &str,
) -> Vec<PageElement> {
    let mut e = vec![web::styled(
        "feed-title",
        title,
        web::style().size(19).bold().color(p.ink.clone()),
    )];
    if ids.is_empty() {
        e.push(web::styled(
            "feed-empty",
            "Nothing here yet.",
            web::style().size(14).color(p.muted.clone()),
        ));
    }
    let cards = ids.iter().map(|id| post_card(s, ctx, p, id, here));
    if s.photos() {
        // A photo feed is a wall of tiles, two across, rather than a single column.
        e.push(web::grid("feed-grid", 2, 12, cards.collect()));
    } else {
        e.extend(cards);
    }
    e
}
fn timeline(s: &SocialState, ctx: &ServiceContext, home: bool) -> SimResult<HttpResponse> {
    let p = Palette::of(&s.theme);
    let here = if home { "/" } else { "/explore" };
    let (title, ids) = if home {
        (
            if s.professional() { "Feed" } else { "Home" },
            s.home(&ctx.actor),
        )
    } else if s.explore_title.is_empty() {
        ("Explore", s.explore())
    } else {
        (s.explore_title.as_str(), s.explore())
    };
    let mut e = nav(s, &p, &ctx.actor, here);
    if s.account_of(&ctx.actor).is_some() {
        e.push(web::card(
            "compose-box",
            web::style()
                .padding(14)
                .radius(14)
                .background(p.surface.clone())
                .border(p.line.clone()),
            vec![
                web::styled(
                    "compose-title",
                    if s.professional() {
                        "Share an update"
                    } else if s.photos() {
                        "Share a photo"
                    } else {
                        "What is happening?"
                    },
                    web::style().size(14).medium().color(p.ink.clone()),
                ),
                web::form("compose", "/posts", &[("text", "Post", "")]),
            ],
        ));
    }
    e.extend(feed(s, ctx, &p, title, &ids, here));
    web::themed_page(&format!("{} / {title}", s.brand), s.theme.clone(), e)
}
fn profile(s: &SocialState, ctx: &ServiceContext, handle: &str) -> SimResult<HttpResponse> {
    let Some(a) = s.accounts.get(handle) else {
        return web::error(404, "no such account");
    };
    let p = Palette::of(&s.theme);
    let here = format!("/{handle}");
    let following = s
        .follows
        .get(&ctx.actor)
        .is_some_and(|f| f.contains(handle));
    let connected = s
        .connections
        .get(&ctx.actor)
        .is_some_and(|c| c.contains(handle));
    let mine = s
        .account_of(&ctx.actor)
        .is_some_and(|m| m.handle == *handle);
    let mut facts = vec![web::styled(
        "bio",
        &a.bio,
        web::style().size(14).color(p.ink.clone()),
    )];
    if !a.headline.is_empty() {
        facts.insert(
            0,
            web::styled(
                "headline",
                &a.headline,
                web::style().size(15).medium().color(p.ink.clone()),
            ),
        );
    }
    let mut meta = vec![
        web::badge(
            "followers",
            format!("{} followers", a.followers),
            web::style().width(130).background(p.accent.clone()),
        ),
        web::badge(
            "following-count",
            format!("{} following", a.following),
            web::style()
                .width(130)
                .background(p.surface.clone())
                .color(p.muted.clone())
                .border(p.line.clone()),
        ),
    ];
    if !a.location.is_empty() {
        meta.push(web::styled(
            "location",
            &a.location,
            web::style().size(13).color(p.muted.clone()),
        ));
    }
    facts.push(web::row("meta", 8, "center", meta));
    if !a.site.is_empty() {
        facts.push(web::link("site", &a.site, &a.site));
    }
    let mut actions = vec![];
    if !mine {
        actions.push(pill(
            "follow",
            if following { "Following" } else { "Follow" },
            following,
            &format!("/accounts/{handle}/follow"),
            &here,
            &p,
        ));
        if s.professional() {
            actions.push(pill(
                "connect",
                if connected {
                    "Invitation sent"
                } else {
                    "Connect"
                },
                connected,
                &format!("/accounts/{handle}/connect"),
                &here,
                &p,
            ));
        }
    }
    if !actions.is_empty() {
        facts.push(web::row("profile-acts", 8, "center", actions));
    }
    let mut e = nav(s, &p, &ctx.actor, &here);
    e.push(web::card(
        "profile",
        web::style()
            .padding(16)
            .radius(14)
            .background(p.surface.clone())
            .border(p.line.clone()),
        vec![web::row(
            "profile-row",
            14,
            "start",
            vec![
                avatar("profile-avatar", Some(a), handle, 64),
                column(
                    "profile-body",
                    8,
                    [
                        vec![
                            web::styled(
                                "name",
                                &a.name,
                                web::style().size(22).bold().color(p.ink.clone()),
                            ),
                            web::styled(
                                "handle",
                                display(s, handle),
                                web::style().size(14).color(p.muted.clone()),
                            ),
                        ],
                        facts,
                    ]
                    .concat(),
                ),
            ],
        )],
    ));
    if !a.experience.is_empty() {
        e.push(web::styled(
            "exp-title",
            "Experience",
            web::style().size(17).bold().color(p.ink.clone()),
        ));
        for (i, r) in a.experience.iter().enumerate() {
            e.push(web::card(
                &format!("exp-{i}"),
                web::style()
                    .padding(12)
                    .radius(12)
                    .background(p.surface.clone())
                    .border(p.line.clone()),
                vec![web::row(
                    &format!("exp-{i}-row"),
                    12,
                    "center",
                    vec![
                        web::thumbnail(
                            &format!("exp-{i}-logo"),
                            initials(&r.company),
                            web::style()
                                .width(40)
                                .height(40)
                                .radius(8)
                                .size(12)
                                .background(tint(&r.company)),
                        ),
                        column(
                            &format!("exp-{i}-body"),
                            4,
                            vec![
                                web::styled(
                                    &format!("exp-{i}-title"),
                                    format!("{} · {}", r.title, r.company),
                                    web::style().size(14).medium().color(p.ink.clone()),
                                ),
                                web::styled(
                                    &format!("exp-{i}-period"),
                                    &r.period,
                                    web::style().size(12).color(p.muted.clone()),
                                ),
                            ],
                        ),
                    ],
                )],
            ));
        }
    }
    let ids = s.by_author(handle);
    e.extend(feed(s, ctx, &p, "Posts", &ids, &here));
    web::themed_page(
        &format!("{} ({}) / {}", a.name, display(s, handle), s.brand),
        s.theme.clone(),
        e,
    )
}
fn thread(
    s: &SocialState,
    ctx: &ServiceContext,
    handle: &str,
    id: &str,
) -> SimResult<HttpResponse> {
    let Some(post) = s.posts.get(id) else {
        return web::error(404, "no such post");
    };
    if post.author != handle {
        return web::error(404, "no such post");
    }
    let p = Palette::of(&s.theme);
    let here = format!("/{handle}/status/{id}");
    let mut e = nav(s, &p, &ctx.actor, &here);
    // Walk up to the conversation root so a reply is never read out of context.
    let mut chain = vec![];
    let mut cursor = post.reply_to.clone();
    while let Some(parent) = cursor {
        let Some(up) = s.posts.get(&parent) else {
            break;
        };
        chain.push(up.id.clone());
        cursor = up.reply_to.clone();
    }
    chain.reverse();
    for up in &chain {
        e.push(post_card(s, ctx, &p, up, &here));
    }
    e.push(post_card(s, ctx, &p, id, &here));
    e.push(web::card(
        "reply-box",
        web::style()
            .padding(14)
            .radius(14)
            .background(p.surface.clone())
            .border(p.line.clone()),
        vec![web::form(
            "reply",
            &format!("/posts/{id}/replies"),
            &[("text", "Reply", "")],
        )],
    ));
    let replies = s.replies_to(id);
    e.push(web::styled(
        "replies-title",
        format!("{} replies", replies.len()),
        web::style().size(15).medium().color(p.muted.clone()),
    ));
    e.extend(replies.iter().map(|r| post_card(s, ctx, &p, r, &here)));
    web::themed_page(
        &format!(
            "{} on {}",
            s.accounts.get(handle).map_or(handle, |a| a.name.as_str()),
            s.brand
        ),
        s.theme.clone(),
        e,
    )
}
fn search_page(s: &SocialState, ctx: &ServiceContext, q: &str) -> SimResult<HttpResponse> {
    let p = Palette::of(&s.theme);
    let mut e = nav(s, &p, &ctx.actor, "/search");
    e.push(web::form("search", "/search", &[("q", "Search", q)]));
    let ids = s.search(q);
    let title = if q.trim().is_empty() {
        "Search".to_owned()
    } else {
        format!("{} results for \"{q}\"", ids.len())
    };
    e.extend(feed(s, ctx, &p, &title, &ids, "/search"));
    web::themed_page(&format!("{} / Search", s.brand), s.theme.clone(), e)
}
fn inbox_page(
    s: &SocialState,
    ctx: &ServiceContext,
    root: &str,
    open: Option<&str>,
) -> SimResult<HttpResponse> {
    let p = Palette::of(&s.theme);
    let mut e = nav(s, &p, &ctx.actor, root);
    e.push(web::styled(
        "inbox-title",
        "Messages",
        web::style().size(19).bold().color(p.ink.clone()),
    ));
    let threads = s.inbox(&ctx.actor);
    if threads.is_empty() {
        e.push(web::styled(
            "inbox-empty",
            "No conversations.",
            web::style().size(14).color(p.muted.clone()),
        ));
    }
    for c in &threads {
        let last = c.messages.last();
        e.push(web::card_action(
            &format!("thread-{}", c.id),
            web::style()
                .padding(12)
                .radius(12)
                .background(p.surface.clone())
                .border(p.line.clone()),
            web::visit(format!("{root}/{}", c.id)),
            vec![web::row(
                &format!("thread-{}-row", c.id),
                12,
                "center",
                vec![
                    avatar(
                        &format!("thread-{}-avatar", c.id),
                        None,
                        c.members.iter().next().map_or("", String::as_str),
                        36,
                    ),
                    column(
                        &format!("thread-{}-body", c.id),
                        4,
                        vec![
                            web::styled(
                                &format!("thread-{}-title", c.id),
                                &c.title,
                                web::style().size(14).medium().color(p.ink.clone()),
                            ),
                            web::styled(
                                &format!("thread-{}-last", c.id),
                                last.map_or(String::new(), |m| {
                                    format!("{}: {}", display(s, &m.from), m.text)
                                }),
                                web::style().size(12).color(p.muted.clone()).one_line(),
                            ),
                        ],
                    ),
                ],
            )],
        ));
    }
    if let Some(id) = open {
        let c = match s.conversation(&ctx.actor, id) {
            Ok(c) => c,
            Err(e) => return web::error(403, e),
        };
        e.push(web::divider("thread-rule"));
        e.push(web::styled(
            "open-title",
            &c.title,
            web::style().size(17).bold().color(p.ink.clone()),
        ));
        for m in &c.messages {
            let own = s.account_of(&ctx.actor).is_some_and(|a| a.handle == m.from);
            e.push(web::card(
                &format!("dm-{}", m.id),
                web::style()
                    .padding(12)
                    .radius(12)
                    .background(if own {
                        p.accent.clone()
                    } else {
                        p.surface.clone()
                    })
                    .border(p.line.clone()),
                vec![
                    web::styled(
                        &format!("dm-{}-from", m.id),
                        format!("{} · {}", display(s, &m.from), ago(ctx.tick, m.tick)),
                        web::style().size(12).medium().color(if own {
                            "#ffffff"
                        } else {
                            p.muted.as_str()
                        }),
                    ),
                    web::styled(
                        &format!("dm-{}-text", m.id),
                        &m.text,
                        web::style()
                            .size(14)
                            .color(if own { "#ffffff" } else { p.ink.as_str() }),
                    ),
                ],
            ));
        }
        e.push(web::form(
            "dm",
            &format!("{root}/{id}/messages"),
            &[("text", "Message", "")],
        ));
    }
    web::themed_page(&format!("{} / Messages", s.brand), s.theme.clone(), e)
}
/// The one router: every GET and every re-render after a successful form POST goes through here.
fn view(
    s: &SocialState,
    ctx: &ServiceContext,
    path: &str,
    q: Option<&str>,
) -> SimResult<HttpResponse> {
    let parts: Vec<&str> = path.trim_matches('/').split('/').collect();
    match parts.as_slice() {
        [""] => timeline(s, ctx, true),
        ["explore"] | ["local"] => timeline(s, ctx, false),
        ["search"] => search_page(s, ctx, q.unwrap_or_default()),
        ["messages"] => inbox_page(s, ctx, "/messages", None),
        ["messaging"] => inbox_page(s, ctx, "/messaging", None),
        ["messages", id] => inbox_page(s, ctx, "/messages", Some(id)),
        ["messaging", id] => inbox_page(s, ctx, "/messaging", Some(id)),
        [handle] => profile(s, ctx, handle),
        [handle, "status", id] => thread(s, ctx, handle, id),
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
    /// Every page an agent can reach must survive `Page::validate`: unique ids, legal colours,
    /// legal spans. A duplicate id would make a control ambiguous to click.
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
            let page: cw_protocol::Page = serde_json::from_slice(&r.body).unwrap();
            page.validate().unwrap_or_else(|e| panic!("{url}: {e}"));
        }
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
            body.contains("\"method\":\"POST\""),
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
