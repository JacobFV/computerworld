//! Discussion sites: reddit.com (`subreddits`), stackoverflow.com (`qa`) and
//! news.ycombinator.com (`linkfeed`). `mode` picks the ranking, the permalink shape and the
//! thread layout, so one crate renders a subreddit tree, an accepted-answer page and a ranked
//! link feed without any of the three looking like the others.
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
pub use view::{PAGE_SIZE, SKINS};
use view::View;
pub struct ForumService;
pub fn register(registry: &mut Registry) -> SimResult<()> {
    registry.register(ForumService)
}
/// Documented seed keys, gated by container type before the typed load so an authoring typo is a
/// load error rather than a silently empty board.
const OBJECTS: &[&str] = &[
    "theme",
    "members",
    "boards",
    "threads",
    "votes",
    "subscriptions",
];
const ARRAYS: &[&str] = &[];
/// `mode` is the documented discriminant; an unlisted value is a seed typo, not a fallback.
pub const MODES: &[&str] = &["subreddits", "qa", "linkfeed"];
pub const MAX_TITLE: usize = 300;
pub const MAX_BODY: usize = 8000;
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct ForumState {
    pub mode: String,
    /// Which product the pages look like: one of [`SKINS`]. Absent, the brand decides
    /// (`Hacker News`, `Yelp`), and failing that the mode.
    pub skin: String,
    pub brand: String,
    pub tagline: String,
    /// What a top-level entry is called here: "post", "question", "story".
    pub item_word: String,
    /// What a reply is called here: "comment" on Reddit and HN, "answer" on Stack Overflow.
    pub reply_word: String,
    pub theme: PageTheme,
    pub members: BTreeMap<String, Member>,
    pub boards: BTreeMap<String, Board>,
    pub threads: BTreeMap<String, Thread>,
    /// Target id (thread or reply) to the actors who voted on it and which way. The seeded
    /// `score` is the crowd underneath; these are the votes this world actually cast.
    pub votes: BTreeMap<String, BTreeMap<String, i64>>,
    pub subscriptions: BTreeMap<String, BTreeSet<String>>,
    pub next_id: u64,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Member {
    pub handle: String,
    pub name: String,
    /// The OS user this account belongs to, or `null` for someone with no machine in this world.
    pub actor: Option<String>,
    pub reputation: u64,
    /// Subreddit flair or a Stack Overflow gold-tag line; rendered as a badge when present.
    pub flair: String,
    pub bio: String,
    pub site: String,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Board {
    /// `""` is the single implicit board that `qa` and `linkfeed` sites live on.
    pub id: String,
    pub title: String,
    pub description: String,
    pub members: u64,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Thread {
    pub id: String,
    pub board: String,
    pub author: String,
    pub title: String,
    pub body: String,
    /// An outbound link: a `linkfeed` story points off-site and the title card navigates there.
    pub url: Option<String>,
    pub tags: Vec<String>,
    pub tick: u64,
    pub score: i64,
    pub views: u64,
    /// `qa` only, and only the asker may set it.
    pub accepted: Option<String>,
    pub replies: Vec<Reply>,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Reply {
    pub id: String,
    pub author: String,
    /// Another reply in the same thread, or `null` for a direct reply to the thread.
    pub parent: Option<String>,
    pub body: String,
    pub tick: u64,
    pub score: i64,
    pub comments: Vec<Comment>,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Comment {
    pub id: String,
    pub author: String,
    pub body: String,
    pub tick: u64,
}
/// What a submit form carries. One value crosses the boundary instead of five loose strings,
/// and the field names are the form field names.
#[derive(Clone, Copy, Debug, Default)]
pub struct Draft<'a> {
    pub board: &'a str,
    pub title: &'a str,
    pub body: &'a str,
    pub url: Option<&'a str>,
    pub tags: &'a [String],
}
impl ForumState {
    pub fn qa(&self) -> bool {
        self.mode == "qa"
    }
    pub fn linkfeed(&self) -> bool {
        self.mode == "linkfeed"
    }
    pub fn subreddits(&self) -> bool {
        self.mode == "subreddits"
    }
    /// The skin the pages render in: the seeded `skin`, else the brand's own, else the mode's.
    pub fn skin(&self) -> &'static str {
        let brand: String = self
            .brand
            .chars()
            .filter(|c| c.is_ascii_alphanumeric())
            .collect::<String>()
            .to_ascii_lowercase();
        SKINS
            .iter()
            .find(|k| **k == self.skin)
            .or_else(|| SKINS.iter().find(|k| **k == brand))
            .copied()
            .unwrap_or(match self.mode.as_str() {
                "qa" => "stackoverflow",
                "linkfeed" => "hackernews",
                _ => "reddit",
            })
    }
    fn item_word(&self) -> &str {
        match self.item_word.as_str() {
            "" if self.qa() => "question",
            "" if self.linkfeed() => "story",
            "" => "post",
            w => w,
        }
    }
    fn reply_word(&self) -> &str {
        match self.reply_word.as_str() {
            "" if self.qa() => "answer",
            "" => "comment",
            w => w,
        }
    }
    pub fn member_of(&self, actor: &str) -> Option<&Member> {
        self.members
            .values()
            .find(|m| m.actor.as_deref() == Some(actor))
    }
    fn handle_of(&self, actor: &str) -> Result<String, String> {
        self.member_of(actor)
            .map(|m| m.handle.clone())
            .ok_or_else(|| format!("no account on this site for {actor}"))
    }
    pub fn reply(&self, rid: &str) -> Option<(&Thread, &Reply)> {
        self.threads
            .values()
            .find_map(|t| t.replies.iter().find(|r| r.id == rid).map(|r| (t, r)))
    }
    /// Net of the seeded crowd and every vote this world has cast on the target.
    pub fn score_of(&self, id: &str) -> i64 {
        let seeded = self
            .threads
            .get(id)
            .map(|t| t.score)
            .or_else(|| self.reply(id).map(|(_, r)| r.score))
            .unwrap_or(0);
        seeded + self.votes.get(id).map_or(0, |v| v.values().sum::<i64>())
    }
    pub fn my_vote(&self, actor: &str, id: &str) -> i64 {
        self.votes
            .get(id)
            .and_then(|v| v.get(actor))
            .copied()
            .unwrap_or(0)
    }
    /// Every comment under every reply, which is what a "N comments" counter means here.
    pub fn conversation_size(&self, t: &Thread) -> usize {
        t.replies.len() + t.replies.iter().map(|r| r.comments.len()).sum::<usize>()
    }
    /// The canonical URL of a thread, which differs per mode because the three sites do.
    pub fn permalink(&self, t: &Thread) -> String {
        match self.mode.as_str() {
            "qa" => format!("/questions/{}", t.id),
            "linkfeed" => format!("/item?id={}", t.id),
            _ => format!("/r/{}/comments/{}", t.board, t.id),
        }
    }
    fn used(&self, id: &str) -> bool {
        self.threads.contains_key(id)
            || self.threads.values().any(|t| {
                t.replies
                    .iter()
                    .any(|r| r.id == id || r.comments.iter().any(|c| c.id == id))
            })
    }
    fn mint(&mut self, prefix: &str) -> Result<String, String> {
        loop {
            self.next_id = self.next_id.checked_add(1).ok_or("ID space exhausted")?;
            let id = format!("{prefix}-{}", self.next_id);
            if !self.used(&id) {
                return Ok(id);
            }
        }
    }
    /// HN-style decay in integers: points over `(age + 2)^1.5`, scaled so the quotient still
    /// separates neighbours. No floats, so the ordering is bit-identical on every machine.
    fn hotness(&self, id: &str, now: u64) -> i64 {
        let age = self
            .threads
            .get(id)
            .map_or(0, |t| now.saturating_sub(t.tick));
        let base = (age + 2).min(100_000);
        let gravity = (base * base * base).isqrt().max(1) as i64;
        self.score_of(id).saturating_mul(10_000) / gravity
    }
    fn sort_ranked(&self, ids: &mut [String], now: u64) {
        let key = |id: &str| {
            if self.linkfeed() {
                self.hotness(id, now)
            } else {
                self.score_of(id)
            }
        };
        ids.sort_by(|a, b| {
            key(b).cmp(&key(a)).then_with(|| {
                let (x, y) = (&self.threads[a], &self.threads[b]);
                y.tick.cmp(&x.tick).then_with(|| x.id.cmp(&y.id))
            })
        });
    }
    /// The front page. `subreddits` narrows to what this actor subscribes to; an actor who
    /// subscribes to nothing gets the whole site rather than an empty page.
    pub fn front(&self, actor: &str, now: u64) -> Vec<String> {
        let mut ids: Vec<String> = if self.subreddits() {
            let subs = self.subscriptions.get(actor);
            let mine: Vec<String> = self
                .threads
                .values()
                .filter(|t| subs.is_some_and(|s| s.contains(&t.board)))
                .map(|t| t.id.clone())
                .collect();
            if mine.is_empty() {
                self.threads.keys().cloned().collect()
            } else {
                mine
            }
        } else {
            self.threads.keys().cloned().collect()
        };
        self.sort_ranked(&mut ids, now);
        ids
    }
    pub fn newest(&self) -> Vec<String> {
        let mut ids: Vec<String> = self.threads.keys().cloned().collect();
        ids.sort_by(|a, b| {
            let (x, y) = (&self.threads[a], &self.threads[b]);
            y.tick.cmp(&x.tick).then_with(|| y.id.cmp(&x.id))
        });
        ids
    }
    pub fn in_board(&self, board: &str, now: u64) -> Vec<String> {
        let mut ids: Vec<String> = self
            .threads
            .values()
            .filter(|t| t.board == board)
            .map(|t| t.id.clone())
            .collect();
        self.sort_ranked(&mut ids, now);
        ids
    }
    pub fn tagged(&self, tag: &str, now: u64) -> Vec<String> {
        let needle = tag.to_lowercase();
        let mut ids: Vec<String> = self
            .threads
            .values()
            .filter(|t| t.tags.iter().any(|x| x.to_lowercase() == needle))
            .map(|t| t.id.clone())
            .collect();
        self.sort_ranked(&mut ids, now);
        ids
    }
    pub fn by_author(&self, handle: &str) -> Vec<String> {
        let mut ids: Vec<String> = self
            .threads
            .values()
            .filter(|t| t.author == handle)
            .map(|t| t.id.clone())
            .collect();
        ids.sort();
        ids
    }
    /// Titles, bodies, tags and the replies underneath, matched case-insensitively.
    pub fn search(&self, q: &str, now: u64) -> Vec<String> {
        let needle = q.trim().to_lowercase();
        if needle.is_empty() {
            return vec![];
        }
        let mut ids: Vec<String> = self
            .threads
            .values()
            .filter(|t| {
                t.title.to_lowercase().contains(&needle)
                    || t.body.to_lowercase().contains(&needle)
                    || t.author.to_lowercase().contains(&needle)
                    || t.tags.iter().any(|x| x.to_lowercase().contains(&needle))
                    || t.replies.iter().any(|r| {
                        r.body.to_lowercase().contains(&needle)
                            || r.comments
                                .iter()
                                .any(|c| c.body.to_lowercase().contains(&needle))
                    })
            })
            .map(|t| t.id.clone())
            .collect();
        self.sort_ranked(&mut ids, now);
        ids
    }
    /// Direct children of `parent` (or of the thread itself), best first. `qa` puts the accepted
    /// answer at the top no matter what it scored, because that is the whole point of the page.
    pub fn children(&self, t: &Thread, parent: Option<&str>) -> Vec<String> {
        let mut ids: Vec<String> = t
            .replies
            .iter()
            .filter(|r| r.parent.as_deref() == parent)
            .map(|r| r.id.clone())
            .collect();
        let accepted = t.accepted.clone().unwrap_or_default();
        ids.sort_by(|a, b| {
            (*b == accepted)
                .cmp(&(*a == accepted))
                .then_with(|| self.score_of(b).cmp(&self.score_of(a)))
                .then_with(|| {
                    let pos = |id: &String| t.replies.iter().position(|r| r.id == *id);
                    pos(a).cmp(&pos(b))
                })
        });
        ids
    }
    pub fn submit(&mut self, actor: &str, draft: Draft<'_>, tick: u64) -> Result<Thread, String> {
        let Draft {
            board,
            title,
            body,
            url,
            tags,
        } = draft;
        let author = self.handle_of(actor)?;
        let title = title.trim();
        if title.is_empty() {
            return Err(format!("a {} needs a title", self.item_word()));
        }
        if title.chars().count() > MAX_TITLE {
            return Err(format!("title is longer than {MAX_TITLE} characters"));
        }
        if body.chars().count() > MAX_BODY {
            return Err(format!("body is longer than {MAX_BODY} characters"));
        }
        // An unknown board would strand the post on a page nothing links to.
        let board = if self.boards.contains_key(board) {
            board.to_owned()
        } else if board.is_empty() && !self.boards.is_empty() {
            return Err("pick a board".into());
        } else if board.is_empty() {
            String::new()
        } else {
            return Err(format!("no board {board}"));
        };
        let url = url.map(str::trim).filter(|u| !u.is_empty());
        if url.is_some_and(|u| !u.starts_with("http://") && !u.starts_with("https://")) {
            return Err("a link must be http or https".into());
        }
        if self.linkfeed() && url.is_none() && body.trim().is_empty() {
            return Err("a story needs a link or a text body".into());
        }
        let thread = Thread {
            id: self.mint("t")?,
            board,
            author,
            title: title.into(),
            body: body.trim().into(),
            url: url.map(str::to_owned),
            tags: tags.iter().map(|t| t.trim().to_lowercase()).collect(),
            tick,
            score: 1,
            views: 0,
            accepted: None,
            replies: vec![],
        };
        self.threads.insert(thread.id.clone(), thread.clone());
        Ok(thread)
    }
    pub fn reply_to(
        &mut self,
        actor: &str,
        thread: &str,
        parent: Option<&str>,
        body: &str,
        tick: u64,
    ) -> Result<Reply, String> {
        let author = self.handle_of(actor)?;
        let body = body.trim();
        if body.is_empty() {
            return Err(format!("a {} needs a body", self.reply_word()));
        }
        if body.chars().count() > MAX_BODY {
            return Err(format!("body is longer than {MAX_BODY} characters"));
        }
        let t = self
            .threads
            .get(thread)
            .ok_or_else(|| format!("no thread {thread}"))?;
        // A parent from another thread would render as an orphan, so it is refused here.
        if parent.is_some_and(|p| !t.replies.iter().any(|r| r.id == p)) {
            return Err(format!("no reply {} in {thread}", parent.unwrap_or("")));
        }
        let reply = Reply {
            id: self.mint("r")?,
            author,
            parent: parent.map(str::to_owned),
            body: body.into(),
            tick,
            score: 1,
            comments: vec![],
        };
        self.threads
            .get_mut(thread)
            .ok_or("thread vanished")?
            .replies
            .push(reply.clone());
        Ok(reply)
    }
    pub fn comment(
        &mut self,
        actor: &str,
        rid: &str,
        body: &str,
        tick: u64,
    ) -> Result<Comment, String> {
        let author = self.handle_of(actor)?;
        let body = body.trim();
        if body.is_empty() {
            return Err("a comment needs a body".into());
        }
        if self.reply(rid).is_none() {
            return Err(format!("no reply {rid}"));
        }
        let comment = Comment {
            id: self.mint("c")?,
            author,
            body: body.into(),
            tick,
        };
        for t in self.threads.values_mut() {
            if let Some(r) = t.replies.iter_mut().find(|r| r.id == rid) {
                r.comments.push(comment.clone());
                return Ok(comment);
            }
        }
        Err(format!("no reply {rid}"))
    }
    /// One vote per actor per target. Voting the same way twice takes the vote back, which is
    /// what the arrow does on all three of these sites.
    pub fn vote(&mut self, actor: &str, id: &str, dir: i64) -> Result<Value, String> {
        if !(-1..=1).contains(&dir) {
            return Err("vote direction must be -1, 0 or 1".into());
        }
        if dir < 0 && self.linkfeed() {
            return Err("this site has no downvote".into());
        }
        if !self.threads.contains_key(id) && self.reply(id).is_none() {
            return Err(format!("nothing to vote on at {id}"));
        }
        self.handle_of(actor)?;
        let slot = self.votes.entry(id.into()).or_default();
        let now = slot.get(actor).copied().unwrap_or(0);
        let next = if dir == 0 || dir == now { 0 } else { dir };
        if next == 0 {
            slot.remove(actor);
        } else {
            slot.insert(actor.into(), next);
        }
        if slot.is_empty() {
            self.votes.remove(id);
        }
        Ok(json!({"id": id, "vote": next, "score": self.score_of(id)}))
    }
    /// Only the asker accepts, and only on a Q&A site. Accepting the accepted answer clears it.
    pub fn accept(&mut self, actor: &str, thread: &str, rid: &str) -> Result<Value, String> {
        if !self.qa() {
            return Err("accepting an answer is a Q&A feature".into());
        }
        let handle = self.handle_of(actor)?;
        let t = self
            .threads
            .get(thread)
            .ok_or_else(|| format!("no thread {thread}"))?;
        if t.author != handle {
            return Err("only the asker may accept an answer".into());
        }
        if !t.replies.iter().any(|r| r.id == rid) {
            return Err(format!("no answer {rid} on {thread}"));
        }
        let clear = t.accepted.as_deref() == Some(rid);
        let t = self.threads.get_mut(thread).ok_or("thread vanished")?;
        t.accepted = (!clear).then(|| rid.to_owned());
        Ok(json!({"thread": thread, "accepted": t.accepted}))
    }
    pub fn subscribe(&mut self, actor: &str, board: &str) -> Result<Value, String> {
        if !self.boards.contains_key(board) {
            return Err(format!("no board {board}"));
        }
        let set = self.subscriptions.entry(actor.into()).or_default();
        let on = set.insert(board.into());
        if !on {
            set.remove(board);
        }
        if set.is_empty() {
            self.subscriptions.remove(actor);
        }
        // The member counter is on the page, so it has to move with the edge.
        if let Some(b) = self.boards.get_mut(board) {
            b.members = if on {
                b.members.saturating_add(1)
            } else {
                b.members.saturating_sub(1)
            };
        }
        Ok(json!({"board": board, "subscribed": on}))
    }
}
impl ForumState {
    /// A thread by its stored id, or by the bare number a real question URL carries.
    fn thread_by_id(&self, id: &str) -> Option<&Thread> {
        self.threads
            .get(id)
            .or_else(|| self.threads.get(&format!("t-{id}")))
    }
}
/// What a GET carries besides its path: the search text, the link-feed item id, the list page.
#[derive(Clone, Copy, Default)]
struct Params<'a> {
    q: Option<&'a str>,
    id: Option<&'a str>,
    page: usize,
}
/// The one router: every GET and every re-render after a successful form POST goes through here.
fn view(s: &ForumState, ctx: &ServiceContext, path: &str, params: Params<'_>) -> SimResult<HttpResponse> {
    let parts: Vec<&str> = path.trim_matches('/').split('/').collect();
    let page = params.page.max(1);
    // The page a control comes back to is the page it is on, list page included.
    let at = |base: &str| {
        let here = if page > 1 {
            web::html::href(base, &[("p", &page.to_string())])
        } else {
            base.to_owned()
        };
        View::new(s, ctx, base, &here)
    };
    let thread = |id: &str| match s.thread_by_id(id) {
        Some(t) => {
            let here = s.permalink(t);
            View::new(s, ctx, &here, &here).thread(t)
        }
        None => web::error(404, "no such thread"),
    };
    match parts.as_slice() {
        [""] => at("/").front(page),
        ["newest"] => at("/newest").simple_list("Newest", s.newest(), page),
        ["submit"] | ["ask"] => at("/submit").submit(),
        ["search"] => at("/search").search(params.q.unwrap_or_default(), page),
        ["r"] | ["boards"] => at("/r").boards(),
        ["item"] => match params.id {
            Some(id) => thread(id),
            None => web::error(404, "no item id"),
        },
        ["questions"] => {
            at("/questions").simple_list("All questions", s.front(&ctx.actor, ctx.tick), page)
        }
        ["questions", "tagged", tag] => {
            let tag = decode(tag);
            let title = match s.skin() {
                "craigslist" | "yelp" => tag.clone(),
                _ => format!("Questions tagged [{tag}]"),
            };
            at(&format!("/questions/tagged/{}", parts[2])).simple_list(
                &title,
                s.tagged(&tag, ctx.tick),
                page,
            )
        }
        ["questions", id] => thread(id),
        ["u", handle] | ["users", handle] => at(&format!("/u/{handle}")).member(handle, page),
        ["r", board] => at(&format!("/r/{board}")).board(board, page),
        ["r", _, "comments", id] => thread(id),
        _ => web::error(404, "route not found"),
    }
}
/// Minimal query decoding for the `view=` round trip; the real request path uses `web::query`.
fn params(qs: &str) -> BTreeMap<String, String> {
    qs.split('&')
        .filter(|p| !p.is_empty())
        .filter_map(|pair| {
            let (k, v) = pair.split_once('=')?;
            Some((decode(k), decode(v)))
        })
        .collect()
}
fn decode(raw: &str) -> String {
    let bytes = raw.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => out.push(b' '),
            b'%' if i + 2 < bytes.len() => {
                let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or("");
                match u8::from_str_radix(hex, 16) {
                    Ok(b) => {
                        out.push(b);
                        i += 2;
                    }
                    Err(_) => out.push(b'%'),
                }
            }
            b => out.push(b),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}
impl Service for ForumService {
    fn kind(&self) -> &str {
        "forum"
    }
    fn initialize(&self, initial: Value, _: &ServiceContext) -> SimResult<Value> {
        let gated = web::shape(initial, OBJECTS, ARRAYS)?;
        let mode = web::variant(&gated, "mode", MODES)?;
        // An absent skin follows the brand; a misspelt one is a seed typo.
        web::variant(&gated, "skin", SKINS)?;
        let mut s: ForumState = web::load(&gated)?;
        s.mode = mode;
        for (key, m) in &s.members {
            if m.handle != *key {
                return Err(SimError::invalid(format!(
                    "member {key} carries handle {}",
                    m.handle
                )));
            }
        }
        for (key, b) in &s.boards {
            if b.id != *key {
                return Err(SimError::invalid(format!(
                    "board {key} carries id {}",
                    b.id
                )));
            }
        }
        let mut ids: BTreeSet<&str> = BTreeSet::new();
        for (key, t) in &s.threads {
            if t.id != *key {
                return Err(SimError::invalid(format!(
                    "thread {key} carries id {}",
                    t.id
                )));
            }
            if !t.board.is_empty() && !s.boards.contains_key(&t.board) {
                return Err(SimError::invalid(format!(
                    "thread {key} is on unknown board {}",
                    t.board
                )));
            }
            if !s.members.contains_key(&t.author) {
                return Err(SimError::invalid(format!(
                    "thread {key} is by unknown member {}",
                    t.author
                )));
            }
            if !ids.insert(t.id.as_str()) {
                return Err(SimError::invalid(format!("duplicate id {key}")));
            }
            for r in &t.replies {
                if !s.members.contains_key(&r.author) {
                    return Err(SimError::invalid(format!(
                        "reply {} is by unknown member {}",
                        r.id, r.author
                    )));
                }
                if r.parent
                    .as_deref()
                    .is_some_and(|p| !t.replies.iter().any(|x| x.id == p))
                {
                    return Err(SimError::invalid(format!(
                        "reply {} points outside thread {key}",
                        r.id
                    )));
                }
                if !ids.insert(r.id.as_str()) {
                    return Err(SimError::invalid(format!("duplicate id {}", r.id)));
                }
                for c in &r.comments {
                    if !s.members.contains_key(&c.author) {
                        return Err(SimError::invalid(format!(
                            "comment {} is by unknown member {}",
                            c.id, c.author
                        )));
                    }
                    if !ids.insert(c.id.as_str()) {
                        return Err(SimError::invalid(format!("duplicate id {}", c.id)));
                    }
                }
            }
            if t.accepted
                .as_deref()
                .is_some_and(|a| !t.replies.iter().any(|r| r.id == a))
            {
                return Err(SimError::invalid(format!(
                    "thread {key} accepts an answer it does not hold"
                )));
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
        let mut s: ForumState = web::load(state)?;
        let p = web::path(r);
        let api = p.starts_with("/api/") || p == "/api";
        let routed = p.strip_prefix("/api").unwrap_or(&p);
        let parts: Vec<&str> = routed.trim_matches('/').split('/').collect();
        let method = r.method.to_ascii_uppercase();
        if method == "GET" {
            let q = web::query(r, "q");
            let item = web::query(r, "id");
            if !api {
                let page = web::query(r, "p").and_then(|p| p.parse().ok()).unwrap_or(1);
                return view(
                    &s,
                    ctx,
                    &p,
                    Params {
                        q: q.as_deref(),
                        id: item.as_deref(),
                        page,
                    },
                );
            }
            return match parts.as_slice() {
                ["threads"] => HttpResponse::json(200, &s.front(&ctx.actor, ctx.tick)),
                ["threads", id] => web::domain(
                    s.threads
                        .get(*id)
                        .ok_or_else(|| format!("no thread {id}"))
                        .map(|t| {
                            json!({"thread": t, "score": s.score_of(id),
                                   "url": s.permalink(t), "replies": s.children(t, None)})
                        }),
                ),
                ["search"] => HttpResponse::json(200, &s.search(&q.unwrap_or_default(), ctx.tick)),
                ["boards"] => HttpResponse::json(200, &s.boards),
                ["members"] => HttpResponse::json(200, &s.members),
                ["subscriptions"] => HttpResponse::json(
                    200,
                    &s.subscriptions.get(&ctx.actor).cloned().unwrap_or_default(),
                ),
                _ => web::error(404, "route not found"),
            };
        }
        if method != "POST" {
            return web::error(405, "method not allowed");
        }
        let mut b = web::body(r)?;
        // A textarea submits its line breaks as CRLF; the stored text keeps plain newlines.
        if let Some(Value::String(text)) = b.get_mut("body") {
            *text = text.replace("\r\n", "\n");
        }
        // The search form is a POST that reads; nothing else on this site is.
        if !api && parts.as_slice() == ["search"] {
            let q = web::text(&b, "q");
            return view(
                &s,
                ctx,
                "/search",
                Params {
                    q: Some(&q),
                    ..Params::default()
                },
            );
        }
        let back = match web::text(&b, "view") {
            v if v.is_empty() => None,
            v => Some(v),
        };
        let (result, fallback) = match parts.as_slice() {
            ["threads"] => {
                let url = web::text(&b, "url");
                let (board, title, body) = (
                    web::text(&b, "board"),
                    web::text(&b, "title"),
                    web::text(&b, "body"),
                );
                let tags = web::strings(&b, "tags");
                let r = s.submit(
                    &ctx.actor,
                    Draft {
                        board: &board,
                        title: &title,
                        body: &body,
                        url: (!url.is_empty()).then_some(url.as_str()),
                        tags: &tags,
                    },
                    ctx.tick,
                );
                let at = r
                    .as_ref()
                    .map_or_else(|_| "/".to_owned(), |t| s.permalink(t));
                (r.map(|t| json!(t)), at)
            }
            ["threads", id, "replies"] => {
                let parent = web::text(&b, "parent");
                let at = s
                    .threads
                    .get(*id)
                    .map_or("/".to_owned(), |t| s.permalink(t));
                (
                    s.reply_to(
                        &ctx.actor,
                        id,
                        (!parent.is_empty()).then_some(parent.as_str()),
                        &web::text(&b, "body"),
                        ctx.tick,
                    )
                    .map(|r| json!(r)),
                    at,
                )
            }
            ["replies", id, "comments"] => {
                let at = s.reply(id).map_or("/".to_owned(), |(t, _)| s.permalink(t));
                (
                    s.comment(&ctx.actor, id, &web::text(&b, "body"), ctx.tick)
                        .map(|c| json!(c)),
                    at,
                )
            }
            ["threads", id, "vote"] => {
                let at = s
                    .threads
                    .get(*id)
                    .map_or("/".to_owned(), |t| s.permalink(t));
                (s.vote(&ctx.actor, id, direction(&b)), at)
            }
            ["replies", id, "vote"] => {
                let at = s.reply(id).map_or("/".to_owned(), |(t, _)| s.permalink(t));
                (s.vote(&ctx.actor, id, direction(&b)), at)
            }
            ["threads", id, "accept"] => {
                let at = s
                    .threads
                    .get(*id)
                    .map_or("/".to_owned(), |t| s.permalink(t));
                (s.accept(&ctx.actor, id, &web::text(&b, "reply")), at)
            }
            ["boards", board, "subscribe"] => {
                (s.subscribe(&ctx.actor, board), format!("/r/{board}"))
            }
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
        let (path, rest) = match target.split_once('?') {
            Some((path, rest)) => (path.to_owned(), params(rest)),
            None => (target, BTreeMap::new()),
        };
        view(
            &s,
            ctx,
            &path,
            Params {
                q: rest.get("q").map(String::as_str),
                id: rest.get("id").map(String::as_str),
                page: rest.get("p").and_then(|p| p.parse().ok()).unwrap_or(1),
            },
        )
    }
}
/// `dir` arrives as a form string on a page control and as a number over the API.
fn direction(b: &Value) -> i64 {
    b.get("dir")
        .and_then(|v| v.as_i64().or_else(|| v.as_str()?.parse().ok()))
        .unwrap_or(1)
}
#[cfg(test)]
mod tests {
    use super::*;
    fn ctx(actor: &str, tick: u64) -> ServiceContext {
        ServiceContext {
            actor: actor.into(),
            source: "bob-linux".into(),
            tick,
            seed: 1,
            instance: "forum".into(),
        }
    }
    #[test]
    fn a_question_resolves_by_its_bare_number_as_well_as_its_stored_id() {
        // Real question URLs carry a bare number, and other sites link them that way, so
        // a link that reads correctly in prose must not 404.
        let state: ForumState = serde_json::from_value(qa_seed()).unwrap();
        assert!(state.threads.contains_key("t-4411"));
        assert_eq!(
            state.thread_by_id("4411").map(|t| t.id.clone()),
            Some("t-4411".to_owned()),
            "the bare number did not reach the thread"
        );
        assert_eq!(
            state.thread_by_id("t-4411").map(|t| t.id.clone()),
            Some("t-4411".to_owned())
        );
        assert!(state.thread_by_id("9999").is_none());
        assert!(state.thread_by_id("not-a-thread").is_none());
    }
    fn qa_seed() -> Value {
        json!({
            "mode": "qa",
            "brand": "Stack Overflow",
            "theme": {"accent": "#f48024"},
            "members": {
                "bmartinez": {"handle": "bmartinez", "name": "Bob Martinez", "actor": "bob",
                              "reputation": 3190},
                "praman": {"handle": "praman", "name": "Priya Raman", "reputation": 58402},
                "alicechen": {"handle": "alicechen", "name": "Alice Chen", "actor": "alice",
                              "reputation": 12401}
            },
            "boards": {"": {"id": "", "title": "All questions", "description": ""}},
            "threads": {
                "t-4411": {
                    "id": "t-4411", "board": "", "author": "bmartinez",
                    "title": "Why does my BFS shortest-path test fail only on Windows?",
                    "body": "Repro is in http://github.com/northstar/atlas/issues/14",
                    "tags": ["rust", "determinism"], "tick": 2, "score": 37, "views": 2104,
                    "accepted": "r-9002",
                    "replies": [
                        {"id": "r-9002", "author": "praman", "body": "Your test iterates a HashMap.",
                         "tick": 3, "score": 52,
                         "comments": [{"id": "c-1", "author": "bmartinez", "body": "That was it.",
                                       "tick": 4}]},
                        {"id": "r-9003", "author": "alicechen", "body": "Sort the frontier.",
                         "tick": 4, "score": 8}
                    ]
                }
            },
            "next_id": 4411
        })
    }
    /// A parsed page, checked against the engine's strict pipeline on the way in.
    struct Dom(cw_web::dom::Document);
    impl Dom {
        fn of(r: &HttpResponse) -> Dom {
            assert_eq!(r.header("content-type"), Some(web::html::HTML_MEDIA_TYPE));
            let html = std::str::from_utf8(&r.body).unwrap();
            web::html::validate_strict(html).unwrap_or_else(|e| panic!("{e:?}"));
            Dom(cw_web::html::parse(html))
        }
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
        fn body(&self) -> String {
            self.0.body().map(|b| self.0.text_content(b)).unwrap_or_default()
        }
        /// The hidden and typed fields of a form, by wire name.
        fn fields(&self, form: &str) -> BTreeMap<String, String> {
            let f = self.node(form);
            assert_eq!(self.0.tag(f), Some("form"), "#{form}");
            self.0
                .descendants(f)
                .filter(|n| self.0.is(*n, "input") || self.0.is(*n, "textarea"))
                .filter_map(|n| Some((self.0.attr(n, "name")?.to_owned(), self.0.attr(n, "value").unwrap_or_default().to_owned())))
                .collect()
        }
        /// The form a submit button belongs to: `(action, method)`.
        fn form_of(&self, button: &str) -> (String, String) {
            let b = self.node(button);
            assert_eq!(self.0.tag(b), Some("button"), "#{button}");
            let f = self.0.ancestors(b).find(|a| self.0.is(*a, "form")).unwrap_or_else(|| panic!("#{button} is outside a form"));
            (
                self.0.attr(f, "action").unwrap_or_default().to_owned(),
                self.0.attr(f, "method").unwrap_or_default().to_owned(),
            )
        }
    }
    fn live(seed: Value, actor: &str) -> Value {
        ForumService.initialize(seed, &ctx(actor, 0)).unwrap()
    }
    fn post(state: &mut Value, actor: &str, url: &str, body: &str) -> HttpResponse {
        let mut r = HttpRequest::get(url);
        r.method = "POST".into();
        r.headers.insert(
            "content-type".into(),
            "application/x-www-form-urlencoded".into(),
        );
        r.body = body.as_bytes().to_vec();
        ForumService.handle(state, &ctx(actor, 9), &r).unwrap()
    }
    fn get(state: &mut Value, actor: &str, url: &str) -> HttpResponse {
        ForumService
            .handle(state, &ctx(actor, 9), &HttpRequest::get(url))
            .unwrap()
    }
    fn reddit_seed() -> Value {
        json!({
            "mode": "subreddits",
            "brand": "reddit",
            "members": {
                "alice_c": {"handle": "alice_c", "name": "Alice Chen", "actor": "alice",
                            "flair": "Northstar"},
                "bobm": {"handle": "bobm", "name": "Bob Martinez", "actor": "bob"}
            },
            "boards": {
                "programming": {"id": "programming", "title": "Programming",
                                "description": "Code talk.", "members": 4100000},
                "rust": {"id": "rust", "title": "Rust", "description": "Rustaceans.",
                         "members": 310000}
            },
            "threads": {
                "t-5120": {"id": "t-5120", "board": "programming", "author": "bobm",
                           "title": "The Verge on deterministic simulation", "body": "Good read.",
                           "tick": 3, "score": 812,
                           "replies": [{"id": "r-1", "author": "alice_c", "body": "Thanks!",
                                        "tick": 4, "score": 61}]},
                "t-5121": {"id": "t-5121", "board": "rust", "author": "alice_c",
                           "title": "Sorting a frontier for determinism", "body": "BTreeMap.",
                           "tick": 4, "score": 120, "replies": []}
            },
            "subscriptions": {"alice": ["rust"]},
            "next_id": 5121
        })
    }
    fn hn_seed() -> Value {
        json!({
            "mode": "linkfeed",
            "brand": "Hacker News",
            "members": {
                "tweber": {"handle": "tweber", "name": "tweber"},
                "bobm": {"handle": "bobm", "name": "bobm", "actor": "bob"}
            },
            "threads": {
                "t-9001": {"id": "t-9001", "board": "", "author": "tweber",
                           "title": "Northstar's Atlas bets everything on determinism",
                           "url": "http://theverge.com/2026/atlas-determinism",
                           "tick": 4, "score": 147, "replies": []},
                "t-9002": {"id": "t-9002", "board": "", "author": "bobm",
                           "title": "Northstar down", "url": "http://status.northstar.example/",
                           "tick": 1, "score": 61, "replies": []}
            },
            "next_id": 9002
        })
    }
    /// Every page an agent can reach must survive the strict validator: unique ids and only
    /// HTML and CSS the engine renders. A duplicate id would make a control ambiguous to click.
    #[test]
    fn every_route_renders_a_valid_page() {
        for (seed, actor, urls) in [
            (
                qa_seed(),
                "bob",
                vec![
                    "http://stackoverflow.com/",
                    "http://stackoverflow.com/newest",
                    "http://stackoverflow.com/submit",
                    "http://stackoverflow.com/search?q=windows",
                    "http://stackoverflow.com/questions",
                    "http://stackoverflow.com/questions/t-4411",
                    "http://stackoverflow.com/questions/tagged/rust",
                    "http://stackoverflow.com/u/praman",
                ],
            ),
            (
                reddit_seed(),
                "alice",
                vec![
                    "http://reddit.com/",
                    "http://reddit.com/r",
                    "http://reddit.com/r/programming",
                    "http://reddit.com/r/programming/comments/t-5120",
                    "http://reddit.com/submit",
                    "http://reddit.com/u/alice_c",
                ],
            ),
            (
                hn_seed(),
                "bob",
                vec![
                    "http://news.ycombinator.com/",
                    "http://news.ycombinator.com/newest",
                    "http://news.ycombinator.com/item?id=t-9001",
                    "http://news.ycombinator.com/submit",
                ],
            ),
        ] {
            let mut state = live(seed, actor);
            for url in urls {
                let r = get(&mut state, actor, url);
                assert_eq!(r.status, 200, "{url}");
                let page = Dom::of(&r);
                for id in ["masthead", "nav", "nav-home", "nav-new", "nav-search", "nav-submit", "nav-me", "search", "search-q", "search-submit", "content"] {
                    assert!(page.has(id), "{url}: no #{id}");
                }
                assert_eq!(page.attr("search", "action"), "/search", "{url}");
                assert_eq!(page.attr("search", "method"), "post", "{url}");
                assert_eq!(page.attr("search-q", "name"), "q", "{url}");
            }
            assert_eq!(get(&mut state, actor, "http://x/nope/nope").status, 404);
        }
    }
    #[test]
    fn seed_shape_and_references_are_gated_at_load() {
        assert!(ForumService.initialize(json!([]), &ctx("bob", 0)).is_err());
        assert!(ForumService
            .initialize(json!({"boards": []}), &ctx("bob", 0))
            .is_err());
        assert!(ForumService
            .initialize(json!({"mode": "nonesuch"}), &ctx("bob", 0))
            .is_err());
        assert!(ForumService
            .initialize(
                json!({"threads": {"t-1": {"id": "t-1", "author": "ghost"}}}),
                &ctx("bob", 0)
            )
            .is_err());
        assert!(ForumService
            .initialize(
                json!({"members": {"a": {"handle": "a"}},
                       "threads": {"t-1": {"id": "t-1", "author": "a", "accepted": "r-9"}}}),
                &ctx("bob", 0)
            )
            .is_err());
        assert!(ForumService.initialize(Value::Null, &ctx("bob", 0)).is_ok());
    }
    #[test]
    fn rendering_never_mutates_and_is_pure() {
        let mut state = live(qa_seed(), "bob");
        let before = state.clone();
        let a = get(
            &mut state,
            "bob",
            "http://stackoverflow.com/questions/t-4411",
        );
        let b = get(
            &mut state,
            "bob",
            "http://stackoverflow.com/questions/t-4411",
        );
        assert_eq!(a, b);
        assert_eq!(before, state, "a render must not mutate state");
        let page = Dom::of(&a);
        assert_eq!(page.text("r-9002-accepted"), "Accepted", "the accepted answer is marked");
        assert_eq!(page.text("r-9002-body"), "Your test iterates a HashMap.");
        assert!(page.text("c-1-text").starts_with("That was it."), "answer comments render");
    }
    #[test]
    fn voting_is_one_per_actor_and_toggles() {
        let mut state = live(qa_seed(), "bob");
        post(
            &mut state,
            "alice",
            "http://stackoverflow.com/threads/t-4411/vote",
            "dir=1&view=/questions/t-4411",
        );
        assert_eq!(
            web::load::<ForumState>(&state).unwrap().score_of("t-4411"),
            38
        );
        // The same arrow again takes the vote back; the other arrow flips it.
        post(
            &mut state,
            "alice",
            "http://stackoverflow.com/threads/t-4411/vote",
            "dir=1&view=/questions/t-4411",
        );
        assert_eq!(
            web::load::<ForumState>(&state).unwrap().score_of("t-4411"),
            37
        );
        post(
            &mut state,
            "alice",
            "http://stackoverflow.com/threads/t-4411/vote",
            "dir=-1&view=/questions/t-4411",
        );
        let s: ForumState = web::load(&state).unwrap();
        assert_eq!(s.score_of("t-4411"), 36);
        assert_eq!(s.my_vote("alice", "t-4411"), -1);
        post(
            &mut state,
            "bob",
            "http://stackoverflow.com/replies/r-9003/vote",
            "dir=1&view=/questions/t-4411",
        );
        assert_eq!(
            web::load::<ForumState>(&state).unwrap().score_of("r-9003"),
            9
        );
    }
    #[test]
    fn only_the_asker_accepts_an_answer() {
        let mut state = live(qa_seed(), "bob");
        assert_eq!(
            post(
                &mut state,
                "alice",
                "http://stackoverflow.com/threads/t-4411/accept",
                "reply=r-9003"
            )
            .status,
            400,
            "an answerer cannot accept their own answer"
        );
        let page = post(
            &mut state,
            "bob",
            "http://stackoverflow.com/threads/t-4411/accept",
            "reply=r-9003&view=/questions/t-4411",
        );
        assert_eq!(page.status, 200);
        let s: ForumState = web::load(&state).unwrap();
        assert_eq!(s.threads["t-4411"].accepted.as_deref(), Some("r-9003"));
        // The accepted answer is hoisted above the higher-scoring one.
        assert_eq!(s.children(&s.threads["t-4411"], None)[0], "r-9003");
    }
    #[test]
    fn answering_and_commenting_mutate_and_land_on_the_question() {
        let mut state = live(qa_seed(), "bob");
        let page = post(
            &mut state,
            "alice",
            "http://stackoverflow.com/threads/t-4411/replies",
            "body=Use+a+BTreeMap&view=/questions/t-4411",
        );
        let page = Dom::of(&page);
        assert!(page.body().contains("Use a BTreeMap"));
        assert!(
            page.text("thread-title").starts_with("Why does my BFS"),
            "we are on the question page"
        );
        let s: ForumState = web::load(&state).unwrap();
        assert_eq!(s.threads["t-4411"].replies.len(), 3);
        let new = s.threads["t-4411"].replies.last().unwrap().id.clone();
        post(
            &mut state,
            "bob",
            &format!("http://stackoverflow.com/replies/{new}/comments"),
            "body=Thanks&view=/questions/t-4411",
        );
        let s: ForumState = web::load(&state).unwrap();
        assert_eq!(
            s.threads["t-4411"]
                .replies
                .iter()
                .find(|r| r.id == new)
                .unwrap()
                .comments
                .len(),
            1
        );
    }
    #[test]
    fn refusals_are_real_and_leave_state_alone() {
        let mut state = live(qa_seed(), "bob");
        let before = state.clone();
        assert_eq!(
            post(
                &mut state,
                "bob",
                "http://stackoverflow.com/threads",
                "title=+"
            )
            .status,
            400
        );
        assert_eq!(
            post(
                &mut state,
                "carol",
                "http://stackoverflow.com/threads",
                "title=Hi"
            )
            .status,
            400,
            "an actor with no account here cannot post"
        );
        assert_eq!(
            post(
                &mut state,
                "bob",
                "http://stackoverflow.com/threads/t-9999/vote",
                "dir=1"
            )
            .status,
            400
        );
        assert_eq!(
            post(
                &mut state,
                "bob",
                "http://stackoverflow.com/threads/t-4411/replies",
                "body=x&parent=r-nope"
            )
            .status,
            400,
            "a reply cannot point outside its thread"
        );
        assert_eq!(before, state, "a refusal writes nothing");
    }
    #[test]
    fn subreddit_home_follows_subscriptions_and_join_toggles() {
        let mut state = live(reddit_seed(), "alice");
        let s: ForumState = web::load(&state).unwrap();
        assert_eq!(
            s.front("alice", 9),
            vec!["t-5121"],
            "only r/rust is subscribed"
        );
        assert_eq!(
            s.front("bob", 9).len(),
            2,
            "no subscriptions means the whole site"
        );
        let page = post(
            &mut state,
            "alice",
            "http://reddit.com/boards/programming/subscribe",
            "view=/r/programming",
        );
        let page = Dom::of(&page);
        assert_eq!(page.text("board-head-sub"), "Joined");
        assert_eq!(page.text("board-head-members"), "4100001 members");
        let s: ForumState = web::load(&state).unwrap();
        assert_eq!(s.boards["programming"].members, 4_100_001);
        assert_eq!(s.front("alice", 9).len(), 2);
        post(
            &mut state,
            "alice",
            "http://reddit.com/boards/programming/subscribe",
            "view=/r/programming",
        );
        let s: ForumState = web::load(&state).unwrap();
        assert_eq!(s.boards["programming"].members, 4_100_000);
        assert!(!s.subscriptions["alice"].contains("programming"));
    }
    #[test]
    fn nested_replies_build_a_tree() {
        let mut state = live(reddit_seed(), "alice");
        let page = post(
            &mut state,
            "bob",
            "http://reddit.com/threads/t-5120/replies",
            "body=Nested+here&parent=r-1&view=/r/programming/comments/t-5120",
        );
        assert_eq!(page.status, 200);
        let body = Dom::of(&page).body();
        assert!(body.contains("Nested here") && body.contains("Thanks!"));
        let s: ForumState = web::load(&state).unwrap();
        let t = &s.threads["t-5120"];
        let child = t.replies.iter().find(|r| r.body == "Nested here").unwrap();
        assert_eq!(child.parent.as_deref(), Some("r-1"));
        assert_eq!(s.children(t, Some("r-1")), vec![child.id.clone()]);
        assert_eq!(s.conversation_size(t), 2);
    }
    #[test]
    fn submitting_a_post_requires_a_real_board() {
        let mut state = live(reddit_seed(), "alice");
        assert_eq!(
            post(
                &mut state,
                "alice",
                "http://reddit.com/threads",
                "title=Hello&board=nosuch"
            )
            .status,
            400
        );
        let page = post(
            &mut state,
            "alice",
            "http://reddit.com/threads",
            "title=Determinism+notes&board=rust&body=Sorted+iteration",
        );
        assert_eq!(page.status, 200);
        assert_eq!(
            Dom::of(&page).text("thread-title"),
            "Determinism notes",
            "we land on the new post"
        );
        let s: ForumState = web::load(&state).unwrap();
        let t = s
            .threads
            .values()
            .find(|t| t.title == "Determinism notes")
            .unwrap();
        assert_eq!(t.board, "rust");
        assert_eq!(s.permalink(t), format!("/r/rust/comments/{}", t.id));
    }
    #[test]
    fn a_link_feed_decays_with_age_and_refuses_downvotes() {
        let mut state = live(hn_seed(), "bob");
        let s: ForumState = web::load(&state).unwrap();
        // Fresher and higher scoring outranks older and lower, and the gap is the decay.
        assert_eq!(s.front("bob", 10), vec!["t-9001", "t-9002"]);
        assert!(s.hotness("t-9001", 10) > s.hotness("t-9001", 400));
        assert_eq!(
            post(
                &mut state,
                "bob",
                "http://news.ycombinator.com/threads/t-9001/vote",
                "dir=-1"
            )
            .status,
            400,
            "a link feed has no downvote"
        );
        let page = post(
            &mut state,
            "bob",
            "http://news.ycombinator.com/threads/t-9001/vote",
            "dir=1&view=/",
        );
        let page = Dom::of(&page);
        assert!(page.text("t-9001-by").starts_with("148 points by tweber"));
        assert_eq!(page.text("t-9001-host"), "(theverge.com)", "the outbound host is on the row");
        assert_eq!(page.attr("t-9001-out", "href"), "http://theverge.com/2026/atlas-determinism");
    }
    #[test]
    fn a_submitted_link_must_be_http() {
        let mut state = live(hn_seed(), "bob");
        assert_eq!(
            post(
                &mut state,
                "bob",
                "http://news.ycombinator.com/threads",
                "title=Nope&url=javascript:alert(1)"
            )
            .status,
            400
        );
        let page = post(
            &mut state,
            "bob",
            "http://news.ycombinator.com/threads",
            "title=Atlas+walkthrough&url=http%3A%2F%2Fyoutube.com%2Fwatch%3Fv%3Datlas-walkthrough",
        );
        assert_eq!(page.status, 200);
        let s: ForumState = web::load(&state).unwrap();
        assert!(s
            .threads
            .values()
            .any(|t| t.url.as_deref() == Some("http://youtube.com/watch?v=atlas-walkthrough")));
    }
    #[test]
    fn search_reaches_titles_bodies_tags_and_answers() {
        let mut state = live(qa_seed(), "bob");
        let s: ForumState = web::load(&state).unwrap();
        for q in ["windows", "hashmap", "determinism", "bfs"] {
            assert_eq!(s.search(q, 9), vec!["t-4411".to_owned()], "{q}");
        }
        assert!(s.search("nothing here", 9).is_empty());
        let page = post(
            &mut state,
            "bob",
            "http://stackoverflow.com/search",
            "q=hashmap",
        );
        let page = Dom::of(&page);
        assert_eq!(page.text("list-title"), "1 results for \"hashmap\"");
        assert_eq!(page.attr("search-q", "value"), "hashmap", "the box keeps the query");
        assert_eq!(page.attr("row-t-4411", "href"), "/questions/t-4411");
    }
    #[test]
    fn state_survives_a_snapshot_round_trip() {
        let mut state = live(reddit_seed(), "alice");
        post(
            &mut state,
            "alice",
            "http://reddit.com/threads/t-5120/replies",
            "body=Round+trip&view=/",
        );
        post(
            &mut state,
            "alice",
            "http://reddit.com/threads/t-5120/vote",
            "dir=1&view=/",
        );
        let bytes = serde_json::to_vec(&state).unwrap();
        let mut restored: Value = serde_json::from_slice(&bytes).unwrap();
        let home = HttpRequest::get("http://reddit.com/");
        assert_eq!(
            ForumService
                .handle(&mut state, &ctx("alice", 9), &home)
                .unwrap(),
            ForumService
                .handle(&mut restored, &ctx("alice", 9), &home)
                .unwrap()
        );
        let s: ForumState = web::load(&restored).unwrap();
        assert_eq!(s.score_of("t-5120"), 813);
        assert!(s.threads["t-5120"]
            .replies
            .iter()
            .any(|r| r.body == "Round trip"));
    }
    #[test]
    fn api_mirrors_the_pages_as_json() {
        let mut state = live(qa_seed(), "bob");
        let r = get(
            &mut state,
            "bob",
            "http://stackoverflow.com/api/search?q=hashmap",
        );
        assert_eq!(r.header("content-type"), Some("application/json"));
        assert_eq!(
            serde_json::from_slice::<Vec<String>>(&r.body).unwrap(),
            vec!["t-4411".to_owned()]
        );
        let mut vote = HttpRequest::get("http://stackoverflow.com/api/threads/t-4411/vote");
        vote.method = "POST".into();
        vote.headers
            .insert("content-type".into(), "application/json".into());
        vote.body = b"{\"dir\":1}".to_vec();
        let r = ForumService
            .handle(&mut state, &ctx("bob", 9), &vote)
            .unwrap();
        assert_eq!(r.header("content-type"), Some("application/json"));
        assert_eq!(
            serde_json::from_slice::<Value>(&r.body).unwrap()["score"],
            json!(38)
        );
        let r = get(
            &mut state,
            "bob",
            "http://stackoverflow.com/api/threads/t-4411",
        );
        assert_eq!(
            serde_json::from_slice::<Value>(&r.body).unwrap()["url"],
            json!("/questions/t-4411")
        );
    }
    /// The ids, forms and links the `Page` version exposed are the agent API; each is on the
    /// element that plays the same role, with the same action, method and field names.
    #[test]
    fn the_controls_keep_their_ids_routes_and_field_names() {
        // A Q&A question page: vote arrows, accept, the comment box, the answer box, tags.
        let mut state = live(qa_seed(), "bob");
        let page = Dom::of(&get(&mut state, "bob", "http://stackoverflow.com/questions/t-4411"));
        assert_eq!(page.form_of("t-4411-up"), ("/threads/t-4411/vote".into(), "post".into()));
        assert_eq!(page.fields("t-4411-up-form")["dir"], "1");
        assert_eq!(page.fields("t-4411-down-form")["dir"], "-1");
        assert_eq!(page.fields("t-4411-up-form")["view"], "/questions/t-4411");
        assert_eq!(page.text("t-4411-score"), "37");
        assert_eq!(page.form_of("r-9003-up"), ("/replies/r-9003/vote".into(), "post".into()));
        assert_eq!(page.form_of("r-9003-accept"), ("/threads/t-4411/accept".into(), "post".into()));
        assert_eq!(page.fields("r-9003-accept-form")["reply"], "r-9003");
        assert_eq!(page.text("r-9003-accept"), "Accept");
        assert_eq!(page.text("r-9002-accept"), "Unaccept");
        assert_eq!(page.attr("r-9002-comment", "action"), "/replies/r-9002/comments");
        assert_eq!(page.attr("r-9002-comment-body", "name"), "body");
        assert_eq!(page.tag("r-9002-comment-submit"), "button");
        assert_eq!(page.attr("compose", "action"), "/threads/t-4411/replies");
        assert_eq!(page.attr("compose", "method"), "post");
        assert_eq!(page.tag("compose-body"), "textarea");
        assert_eq!(page.attr("compose-body", "name"), "body");
        assert_eq!(page.fields("compose")["view"], "/questions/t-4411");
        assert_eq!(page.attr("thread-tag-0", "href"), "/questions/tagged/rust");
        assert_eq!(page.text("r-9002-rep"), "58.4k");
        // The link in the body keeps the id its word index gives it.
        let link = (0..200).map(|i| format!("thread-src-link-{i}")).find(|id| page.has(id)).expect("the body link");
        assert_eq!(page.attr(&link, "href"), "http://github.com/northstar/atlas/issues/14");
        // Someone who did not ask gets no accept control.
        let other = Dom::of(&get(&mut state, "alice", "http://stackoverflow.com/questions/t-4411"));
        assert!(!other.has("r-9003-accept"));
        // The list row is one link to the question, with its stats and tags beside it.
        let home = Dom::of(&get(&mut state, "bob", "http://stackoverflow.com/"));
        assert_eq!(home.tag("row-t-4411"), "a");
        assert_eq!(home.attr("row-t-4411", "href"), "/questions/t-4411");
        assert_eq!(home.text("t-4411-st-votes-n"), "37");
        assert_eq!(home.text("t-4411-st-answers-n"), "2");
        assert_eq!(home.text("t-4411-tag-0"), "rust");
        assert_eq!(home.attr("nav-submit", "href"), "/submit");
        assert_eq!(home.attr("nav-me", "href"), "/u/bmartinez");
        // The ask form carries the mode's fields.
        let ask = Dom::of(&get(&mut state, "bob", "http://stackoverflow.com/submit"));
        assert_eq!(ask.attr("submit", "action"), "/threads");
        for (id, name) in [("submit-title", "title"), ("submit-tags", "tags"), ("submit-body", "body")] {
            assert_eq!(ask.attr(id, "name"), name);
        }
        assert!(!ask.has("submit-board") && !ask.has("submit-url"));
        assert_eq!(ask.tag("submit-submit"), "button");
        let anon = Dom::of(&get(&mut state, "carol", "http://stackoverflow.com/submit"));
        assert!(anon.has("submit-anon") && !anon.has("submit"));

        // Reddit: the join toggles, the open link, the board link, the reply box under a comment.
        let mut state = live(reddit_seed(), "alice");
        let home = Dom::of(&get(&mut state, "alice", "http://reddit.com/"));
        assert_eq!(home.attr("t-5121-open", "href"), "/r/rust/comments/t-5121");
        assert_eq!(home.attr("t-5121-board", "href"), "/r/rust");
        assert_eq!(home.form_of("board-rust-sub"), ("/boards/rust/subscribe".into(), "post".into()));
        assert_eq!(home.text("board-rust-sub"), "Joined");
        assert_eq!(home.text("board-programming-sub"), "Join");
        assert_eq!(home.attr("board-programming-link", "href"), "/r/programming");
        assert_eq!(home.attr("nav-boards", "href"), "/r");
        let post = Dom::of(&get(&mut state, "alice", "http://reddit.com/r/programming/comments/t-5120"));
        assert_eq!(post.attr("r-1-reply", "action"), "/threads/t-5120/replies");
        assert_eq!(post.fields("r-1-reply")["parent"], "r-1");
        assert_eq!(post.fields("r-1-reply")["view"], "/r/programming/comments/t-5120");
        assert_eq!(post.attr("r-1-reply-body", "name"), "body");
        assert_eq!(post.text("r-1-flair"), "Northstar");
        assert!(post.text("r-1-by").starts_with("u/alice_c"));
        let ask = Dom::of(&get(&mut state, "alice", "http://reddit.com/submit"));
        assert_eq!(ask.attr("submit-board", "name"), "board");

        // Hacker News: one arrow, the outbound title, the comments link, the url field.
        let mut state = live(hn_seed(), "bob");
        let home = Dom::of(&get(&mut state, "bob", "http://news.ycombinator.com/"));
        assert_eq!(home.text("t-9001-rank"), "1.");
        assert!(home.has("t-9001-up") && !home.has("t-9001-down"));
        assert_eq!(home.attr("t-9001-comments", "href"), "/item?id=t-9001");
        assert_eq!(home.text("t-9001-comments"), "0 comments");
        assert_eq!(home.tag("row-t-9001"), "tr");
        let ask = Dom::of(&get(&mut state, "bob", "http://news.ycombinator.com/submit"));
        assert_eq!(ask.attr("submit-url", "name"), "url");
    }
    /// The skin follows the seed, then the brand, then the mode; each has its own sheet.
    #[test]
    fn every_skin_renders_every_page_strictly() {
        for skin in SKINS {
            let mut seed = match *skin {
                "hackernews" | "craigslist" => hn_seed(),
                "stackoverflow" | "quora" => qa_seed(),
                _ => reddit_seed(),
            };
            seed["skin"] = json!(skin);
            let mut state = live(seed.clone(), "bob");
            assert_eq!(web::load::<ForumState>(&state).unwrap().skin(), *skin);
            let thread = match seed["mode"].as_str().unwrap() {
                "qa" => "/questions/t-4411",
                "linkfeed" => "/item?id=t-9001",
                _ => "/r/programming/comments/t-5120",
            };
            for path in ["/", "/newest", "/search?q=a", "/submit", "/r", "/questions/tagged/rust", "/u/bobm", "/u/bmartinez", "/r/rust", thread] {
                let r = get(&mut state, "bob", &format!("http://site.example{path}"));
                if r.status == 200 {
                    let page = Dom::of(&r);
                    assert!(page.0.body().is_some_and(|b| page.0.attr(b, "class").is_some_and(|c| c.starts_with(&format!("skin-{skin} ")))), "{skin} {path}");
                }
            }
            assert_eq!(get(&mut state, "bob", &format!("http://site.example{thread}")).status, 200);
        }
        let brand = |b: &str, mode: &str| ForumState { brand: b.into(), mode: mode.into(), ..ForumState::default() }.skin();
        assert_eq!(brand("Hacker News", "linkfeed"), "hackernews");
        assert_eq!(brand("craigslist", "linkfeed"), "craigslist");
        assert_eq!(brand("Yelp", "subreddits"), "yelp");
        assert_eq!(brand("Quora", "qa"), "quora");
        assert_eq!(brand("Northstar Answers", "qa"), "stackoverflow");
        assert_eq!(brand("", "subreddits"), "reddit");
        assert!(ForumService.initialize(json!({"skin": "myspace"}), &ctx("bob", 0)).is_err());
    }
    /// Lists longer than a page are paged, and a vote on page two comes back to page two.
    #[test]
    fn long_lists_are_paged() {
        let mut seed = hn_seed();
        for i in 0..(PAGE_SIZE + 5) {
            let id = format!("t-{}", 100 + i);
            seed["threads"][&id] = json!({"id": id, "board": "", "author": "tweber", "title": format!("Story {i}"),
                                          "url": "http://example.com/", "tick": 1, "score": 5, "replies": []});
        }
        let mut state = live(seed, "bob");
        let first = Dom::of(&get(&mut state, "bob", "http://news.ycombinator.com/newest"));
        assert_eq!(first.attr("page-next", "href"), "/newest?p=2");
        assert_eq!(first.attr("page-2", "href"), "/newest?p=2");
        assert!(!first.has("page-prev"));
        let second = Dom::of(&get(&mut state, "bob", "http://news.ycombinator.com/newest?p=2"));
        assert_eq!(second.attr("page-prev", "href"), "/newest");
        let rows: Vec<String> = (0..PAGE_SIZE + 7).map(|i| format!("row-t-{}", 100 + i)).filter(|id| second.has(id)).collect();
        assert_eq!(rows.len(), 7, "thirty of the thirty-seven are on page one: {rows:?}");
        let id = rows
            .iter()
            .map(|r| r.trim_start_matches("row-").to_owned())
            .find(|id| second.text(&format!("{id}-rank")) == format!("{}.", PAGE_SIZE + 1))
            .expect("page two starts at the next rank");
        assert_eq!(second.fields(&format!("{id}-up-form"))["view"], "/newest?p=2");
        let back = Dom::of(&post(&mut state, "bob", &format!("http://news.ycombinator.com/threads/{id}/vote"), "dir=1&view=%2Fnewest%3Fp%3D2"));
        assert_eq!(back.text(&format!("{id}-score")), "6");
        assert!(back.has("page-prev"), "we are still on page two");
    }
    /// Q&A bodies carry code; a Yelp review leads with its stars; a Craigslist title splits.
    #[test]
    fn bodies_render_as_prose_code_and_ratings() {
        let mut seed = qa_seed();
        seed["threads"]["t-4411"]["body"] = json!("This fails:\n\nlet x = 1;\nfoo(x);\n\nWhy does `foo` fail? See http://example.com/a.");
        let mut state = live(seed, "bob");
        let page = Dom::of(&get(&mut state, "bob", "http://stackoverflow.com/questions/t-4411"));
        let body = page.node("thread-body");
        let tags: Vec<&str> = page.0.descendants(body).filter_map(|n| page.0.tag(n)).collect();
        assert_eq!(tags, ["div", "p", "pre", "code", "p", "code", "a"]);
        assert_eq!(page.attr("thread-src-link-12", "href"), "http://example.com/a");

        let mut seed = reddit_seed();
        seed["skin"] = json!("yelp");
        seed["threads"]["t-5120"]["body"] = json!("Alder Kitchen\n1412 NW Alder St\nHours: Tue-Sun 17:00-22:00\nPrice: $$$   Rating: 4.5 stars (1 reviews)\n\nWood-fired.");
        seed["threads"]["t-5120"]["replies"][0]["body"] = json!("****. 4/5. Tight room.");
        let mut state = live(seed, "alice");
        let page = Dom::of(&get(&mut state, "alice", "http://yelp.com/r/programming/comments/t-5120"));
        assert_eq!(page.attr("thread-stars", "aria-label"), "4.5 star rating");
        assert_eq!(page.attr("r-1-stars", "aria-label"), "4.0 star rating");
        assert_eq!(page.text("r-1-body"), "Tight room.");
        assert_eq!(page.text("biz-address"), "1412 NW Alder St");
        assert_eq!(listing_parts_for_test("Oak table - $220 (Alder District)"), ("Oak table", "$220", "Alder District"));
        assert_eq!(listing_parts_for_test("Free firewood"), ("Free firewood", "", ""));
    }
    #[test]
    fn a_tag_page_lists_the_tagged_threads_in_every_mode() {
        let mut seed = hn_seed();
        seed["threads"]["t-9001"]["tags"] = json!(["for sale", "furniture"]);
        let mut state = live(seed, "bob");
        let page = Dom::of(&get(&mut state, "bob", "http://craigslist.org/questions/tagged/furniture"));
        assert!(page.has("row-t-9001"), "{}", page.body());
        let page = Dom::of(&get(&mut state, "bob", "http://craigslist.org/questions/tagged/for%20sale"));
        assert!(page.has("row-t-9001"), "{}", page.body());
    }
    fn listing_parts_for_test(title: &str) -> (&str, &str, &str) {
        view::listing_parts(title)
    }
}
