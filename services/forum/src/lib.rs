//! Discussion sites: reddit.com (`subreddits`), stackoverflow.com (`qa`) and
//! news.ycombinator.com (`linkfeed`). `mode` picks the ranking, the permalink shape and the
//! thread layout, so one crate renders a subreddit tree, an accepted-answer page and a ranked
//! link feed without any of the three looking like the others.
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
        let muted = theme.muted.clone().unwrap_or_else(|| "#6a737c".into());
        Self {
            accent: theme.accent.clone().unwrap_or_else(|| "#ff4500".into()),
            ink: theme.ink.clone().unwrap_or_else(|| "#1a1a1b".into()),
            // A hairline drawn from the muted ink, so no theme needs a key for it.
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
    "#ff4500", "#0079d3", "#46a35e", "#7856ff", "#f48024", "#d93a49",
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
        n => format!("{n}t ago"),
    }
}
/// The host of an outbound link, which is the grey suffix on a link-feed title.
fn host(url: &str) -> String {
    url.split("://")
        .nth(1)
        .unwrap_or(url)
        .split('/')
        .next()
        .unwrap_or("")
        .trim_start_matches("www.")
        .to_owned()
}
fn plural(n: usize, word: &str) -> String {
    if n == 1 {
        format!("{n} {word}")
    } else {
        format!("{n} {word}s")
    }
}
/// A control that mutates and comes back to `view`; the browser posts the field set verbatim.
fn act(url: &str, view: &str, extra: &[(&str, &str)]) -> PageAction {
    let mut fields = BTreeMap::from([("view".to_owned(), view.to_owned())]);
    fields.extend(
        extra
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned())),
    );
    PageAction {
        method: "POST".into(),
        url: url.into(),
        fields,
    }
}
fn pill(
    id: &str,
    label: &str,
    on: bool,
    url: &str,
    view: &str,
    extra: &[(&str, &str)],
    p: &Palette,
) -> PageElement {
    web::card_action(
        id,
        web::style()
            .padding(6)
            .radius(6)
            .background(if on {
                p.accent.clone()
            } else {
                p.surface.clone()
            })
            .border(p.line.clone()),
        act(url, view, extra),
        vec![web::styled(
            &format!("{id}-label"),
            label,
            web::style().size(12).medium().align("center").color(if on {
                "#ffffff"
            } else {
                p.muted.as_str()
            }),
        )],
    )
}
/// A form with typed fields plus fixed hidden values — the parent id, the board, the page to
/// return to. `web::form` only carries typed fields, and a reply box needs both.
fn form_with(
    id: &str,
    url: &str,
    fields: &[(&str, &str, &str)],
    fixed: &[(&str, &str)],
) -> PageElement {
    let action = PageAction {
        method: "POST".into(),
        url: url.into(),
        fields: fields
            .iter()
            .map(|(key, _, _)| ((*key).to_owned(), format!("${id}-{key}")))
            .chain(
                fixed
                    .iter()
                    .map(|(k, v)| ((*k).to_owned(), (*v).to_owned())),
            )
            .collect(),
    };
    PageElement::Form {
        id: id.into(),
        action: action.clone(),
        children: fields
            .iter()
            .map(|(key, label, value)| PageElement::Input {
                id: format!("{id}-{key}"),
                label: (*label).into(),
                value: (*value).into(),
                placeholder: String::new(),
            })
            .chain(std::iter::once(PageElement::Button {
                id: format!("{id}-submit"),
                text: "Submit".into(),
                action,
            }))
            .collect(),
    }
}
fn column(id: &str, gap: u32, children: Vec<PageElement>) -> PageElement {
    web::grid(id, 1, gap, children)
}
fn who(s: &ForumState, handle: &str) -> String {
    match s.mode.as_str() {
        "subreddits" => format!("u/{handle}"),
        _ => s
            .members
            .get(handle)
            .map_or(handle.to_owned(), |m| m.name.clone()),
    }
}
fn nav(s: &ForumState, p: &Palette, actor: &str, here: &str) -> Vec<PageElement> {
    let mut items = vec![
        ("nav-home", "Home".to_owned(), "/".to_owned()),
        ("nav-new", "Newest".to_owned(), "/newest".to_owned()),
        ("nav-search", "Search".to_owned(), "/search".to_owned()),
        (
            "nav-submit",
            match s.mode.as_str() {
                "qa" => "Ask".to_owned(),
                "linkfeed" => "Submit".to_owned(),
                _ => "New post".to_owned(),
            },
            "/submit".to_owned(),
        ),
    ];
    if s.subreddits() {
        items.insert(1, ("nav-boards", "Communities".to_owned(), "/r".to_owned()));
    }
    if let Some(m) = s.member_of(actor) {
        items.push(("nav-me", "Profile".to_owned(), format!("/u/{}", m.handle)));
    }
    let links: Vec<PageElement> = items
        .into_iter()
        .map(|(id, label, url)| {
            let current = url == here;
            web::card_action(
                id,
                web::style()
                    .padding(8)
                    .radius(6)
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
        web::row(
            "masthead",
            10,
            "center",
            vec![
                web::styled(
                    "brand",
                    &s.brand,
                    web::style().size(24).bold().color(p.accent.clone()),
                ),
                web::styled(
                    "tagline",
                    &s.tagline,
                    web::style().size(13).color(p.muted.clone()).one_line(),
                ),
            ],
        ),
        web::grid("nav", links.len().min(6) as u32, 8, links),
        web::divider("nav-rule"),
    ]
}
/// The up/score/down column that sits to the left of a Reddit post and a Stack Overflow answer.
/// A link feed has no downvote, so it gets one arrow and no empty slot pretending otherwise.
fn votes(
    s: &ForumState,
    actor: &str,
    p: &Palette,
    id: &str,
    kind: &str,
    here: &str,
) -> PageElement {
    let mine = s.my_vote(actor, id);
    let url = format!("/{kind}/{id}/vote");
    let mut stack = vec![
        pill(
            &format!("{id}-up"),
            "Up",
            mine > 0,
            &url,
            here,
            &[("dir", "1")],
            p,
        ),
        web::styled(
            &format!("{id}-score"),
            s.score_of(id).to_string(),
            web::style()
                .size(15)
                .bold()
                .align("center")
                .color(if mine != 0 {
                    p.accent.clone()
                } else {
                    p.ink.clone()
                }),
        ),
    ];
    if !s.linkfeed() {
        stack.push(pill(
            &format!("{id}-down"),
            "Down",
            mine < 0,
            &url,
            here,
            &[("dir", "-1")],
            p,
        ));
    }
    web::grid(&format!("{id}-votes"), 1, 4, stack)
}
/// One entry in a ranked list. The three modes put genuinely different furniture around the
/// same thread: a vote column and a subreddit line, a stats block and tags, or a rank number
/// and an outbound host.
fn entry(
    s: &ForumState,
    ctx: &ServiceContext,
    p: &Palette,
    id: &str,
    rank: usize,
    here: &str,
) -> PageElement {
    let t = &s.threads[id];
    let link = s.permalink(t);
    let comments = s.conversation_size(t);
    let title = web::styled(
        &format!("{id}-title"),
        &t.title,
        web::style().size(17).medium().color(p.ink.clone()),
    );
    let card_style = web::style()
        .padding(12)
        .radius(8)
        .background(p.surface.clone())
        .border(p.line.clone());
    if s.linkfeed() {
        let head = match t.url.as_deref() {
            Some(u) => web::row(
                &format!("{id}-head"),
                8,
                "center",
                vec![
                    web::link(&format!("{id}-out"), &t.title, u),
                    web::styled(
                        &format!("{id}-host"),
                        format!("({})", host(u)),
                        web::style().size(12).color(p.muted.clone()).width(180),
                    ),
                ],
            ),
            None => title,
        };
        return web::card(
            &format!("row-{id}"),
            card_style,
            vec![web::row(
                &format!("{id}-row"),
                10,
                "start",
                vec![
                    web::styled(
                        &format!("{id}-rank"),
                        format!("{rank}."),
                        web::style().size(15).color(p.muted.clone()).width(36),
                    ),
                    votes(s, &ctx.actor, p, id, "threads", here),
                    column(
                        &format!("{id}-body"),
                        4,
                        vec![
                            head,
                            web::row(
                                &format!("{id}-meta"),
                                8,
                                "center",
                                vec![
                                    web::styled(
                                        &format!("{id}-by"),
                                        format!(
                                            "{} points by {} {}",
                                            s.score_of(id),
                                            who(s, &t.author),
                                            ago(ctx.tick, t.tick)
                                        ),
                                        web::style().size(12).color(p.muted.clone()),
                                    ),
                                    web::link(
                                        &format!("{id}-comments"),
                                        plural(comments, "comment"),
                                        link,
                                    ),
                                ],
                            ),
                        ],
                    ),
                ],
            )],
        );
    }
    if s.qa() {
        let stat = |sid: String, n: String, label: &str, strong: bool| {
            web::card(
                &sid,
                web::style()
                    .padding(6)
                    .radius(6)
                    .width(78)
                    .background(if strong {
                        p.accent.clone()
                    } else {
                        p.surface.clone()
                    })
                    .border(p.line.clone()),
                vec![
                    web::styled(
                        &format!("{sid}-n"),
                        n,
                        web::style()
                            .size(15)
                            .bold()
                            .align("center")
                            .color(if strong { "#ffffff" } else { p.ink.as_str() }),
                    ),
                    web::styled(
                        &format!("{sid}-l"),
                        label,
                        web::style().size(11).align("center").color(if strong {
                            "#ffffff"
                        } else {
                            p.muted.as_str()
                        }),
                    ),
                ],
            )
        };
        let mut body = vec![
            title,
            web::styled(
                &format!("{id}-excerpt"),
                t.body.chars().take(160).collect::<String>(),
                web::style().size(13).color(p.muted.clone()).one_line(),
            ),
        ];
        let mut tail: Vec<PageElement> = t
            .tags
            .iter()
            .enumerate()
            .map(|(i, tag)| {
                web::badge(
                    &format!("{id}-tag-{i}"),
                    tag,
                    web::style()
                        .size(11)
                        .width(96)
                        .background(p.surface.clone())
                        .color(p.accent.clone())
                        .border(p.line.clone()),
                )
            })
            .collect();
        tail.push(web::styled(
            &format!("{id}-asked"),
            format!("asked by {} {}", who(s, &t.author), ago(ctx.tick, t.tick)),
            web::style().size(12).color(p.muted.clone()),
        ));
        body.push(web::row(&format!("{id}-tags"), 6, "center", tail));
        return web::card_action(
            &format!("row-{id}"),
            card_style,
            web::visit(link),
            vec![web::row(
                &format!("{id}-row"),
                12,
                "start",
                vec![
                    web::row(
                        &format!("{id}-stats"),
                        6,
                        "start",
                        vec![
                            stat(
                                format!("{id}-st-votes"),
                                s.score_of(id).to_string(),
                                "votes",
                                false,
                            ),
                            stat(
                                format!("{id}-st-answers"),
                                t.replies.len().to_string(),
                                "answers",
                                t.accepted.is_some(),
                            ),
                            stat(
                                format!("{id}-st-views"),
                                t.views.to_string(),
                                "views",
                                false,
                            ),
                        ],
                    ),
                    column(&format!("{id}-body"), 6, body),
                ],
            )],
        );
    }
    let board = s.boards.get(&t.board);
    web::card(
        &format!("row-{id}"),
        card_style,
        vec![web::row(
            &format!("{id}-row"),
            12,
            "start",
            vec![
                votes(s, &ctx.actor, p, id, "threads", here),
                web::thumbnail(
                    &format!("{id}-icon"),
                    initials(board.map_or(t.board.as_str(), |b| b.title.as_str())),
                    web::style()
                        .width(40)
                        .height(40)
                        .radius(20)
                        .size(12)
                        .background(tint(&t.board)),
                ),
                column(
                    &format!("{id}-body"),
                    6,
                    vec![
                        web::styled(
                            &format!("{id}-sub"),
                            format!(
                                "r/{} · posted by {} {}",
                                t.board,
                                who(s, &t.author),
                                ago(ctx.tick, t.tick)
                            ),
                            web::style().size(12).color(p.muted.clone()),
                        ),
                        web::card_action(
                            &format!("{id}-open"),
                            web::style().padding(2),
                            web::visit(link),
                            vec![title],
                        ),
                        web::row(
                            &format!("{id}-meta"),
                            8,
                            "center",
                            vec![
                                web::badge(
                                    &format!("{id}-count"),
                                    plural(comments, "comment"),
                                    web::style()
                                        .size(11)
                                        .width(120)
                                        .background(p.surface.clone())
                                        .color(p.muted.clone())
                                        .border(p.line.clone()),
                                ),
                                web::link(
                                    &format!("{id}-board"),
                                    format!("r/{}", t.board),
                                    format!("/r/{}", t.board),
                                ),
                            ],
                        ),
                    ],
                ),
            ],
        )],
    )
}
fn listing(
    s: &ForumState,
    ctx: &ServiceContext,
    p: &Palette,
    title: &str,
    ids: &[String],
    here: &str,
) -> Vec<PageElement> {
    let mut e = vec![web::styled(
        "list-title",
        title,
        web::style().size(19).bold().color(p.ink.clone()),
    )];
    if ids.is_empty() {
        e.push(web::styled(
            "list-empty",
            "Nothing here yet.",
            web::style().size(14).color(p.muted.clone()),
        ));
    }
    e.extend(
        ids.iter()
            .enumerate()
            .map(|(i, id)| entry(s, ctx, p, id, i + 1, here)),
    );
    e
}
/// The subscribe rail. Every board on it is a real page and a real toggle.
fn board_rail(s: &ForumState, actor: &str, p: &Palette, here: &str) -> Vec<PageElement> {
    if !s.subreddits() || s.boards.is_empty() {
        return vec![];
    }
    let subs = s.subscriptions.get(actor);
    let cards: Vec<PageElement> = s
        .boards
        .values()
        .map(|b| {
            let on = subs.is_some_and(|x| x.contains(&b.id));
            web::card(
                &format!("board-{}", b.id),
                web::style()
                    .padding(10)
                    .radius(8)
                    .background(p.surface.clone())
                    .border(p.line.clone()),
                vec![web::row(
                    &format!("board-{}-row", b.id),
                    8,
                    "center",
                    vec![
                        web::link(
                            &format!("board-{}-link", b.id),
                            format!("r/{}", b.id),
                            format!("/r/{}", b.id),
                        ),
                        web::styled(
                            &format!("board-{}-members", b.id),
                            format!("{} members", b.members),
                            web::style().size(11).color(p.muted.clone()).width(110),
                        ),
                        pill(
                            &format!("board-{}-sub", b.id),
                            if on { "Joined" } else { "Join" },
                            on,
                            &format!("/boards/{}/subscribe", b.id),
                            here,
                            &[],
                            p,
                        ),
                    ],
                )],
            )
        })
        .collect();
    vec![
        web::divider("rail-rule"),
        web::styled(
            "rail-title",
            "Your communities",
            web::style().size(15).bold().color(p.ink.clone()),
        ),
        web::grid("rail", 1, 8, cards),
    ]
}
fn front_page(s: &ForumState, ctx: &ServiceContext) -> SimResult<HttpResponse> {
    let p = Palette::of(&s.theme);
    let ids = s.front(&ctx.actor, ctx.tick);
    let title = match s.mode.as_str() {
        "qa" => "Top questions",
        "linkfeed" => "Top stories",
        _ => "Popular posts",
    };
    let mut e = nav(s, &p, &ctx.actor, "/");
    e.extend(listing(s, ctx, &p, title, &ids, "/"));
    e.extend(board_rail(s, &ctx.actor, &p, "/"));
    web::themed_page(&format!("{} / {title}", s.brand), s.theme.clone(), e)
}
fn simple_list(
    s: &ForumState,
    ctx: &ServiceContext,
    title: &str,
    ids: Vec<String>,
    here: &str,
) -> SimResult<HttpResponse> {
    let p = Palette::of(&s.theme);
    let mut e = nav(s, &p, &ctx.actor, here);
    e.extend(listing(s, ctx, &p, title, &ids, here));
    web::themed_page(&format!("{} / {title}", s.brand), s.theme.clone(), e)
}
fn boards_page(s: &ForumState, ctx: &ServiceContext) -> SimResult<HttpResponse> {
    let p = Palette::of(&s.theme);
    let mut e = nav(s, &p, &ctx.actor, "/r");
    e.push(web::styled(
        "list-title",
        "Communities",
        web::style().size(19).bold().color(p.ink.clone()),
    ));
    for b in s.boards.values() {
        e.push(web::styled(
            &format!("about-{}", b.id),
            format!("r/{} — {}", b.id, b.description),
            web::style().size(13).color(p.muted.clone()),
        ));
    }
    e.extend(board_rail(s, &ctx.actor, &p, "/r"));
    web::themed_page(&format!("{} / Communities", s.brand), s.theme.clone(), e)
}
fn board_page(s: &ForumState, ctx: &ServiceContext, board: &str) -> SimResult<HttpResponse> {
    let Some(b) = s.boards.get(board) else {
        return web::error(404, "no such board");
    };
    let p = Palette::of(&s.theme);
    let here = format!("/r/{board}");
    let on = s
        .subscriptions
        .get(&ctx.actor)
        .is_some_and(|x| x.contains(board));
    let mut e = nav(s, &p, &ctx.actor, &here);
    e.push(web::card(
        "board-head",
        web::style()
            .padding(14)
            .radius(8)
            .background(p.surface.clone())
            .border(p.line.clone()),
        vec![web::row(
            "board-head-row",
            12,
            "center",
            vec![
                web::thumbnail(
                    "board-head-icon",
                    initials(&b.title),
                    web::style()
                        .width(48)
                        .height(48)
                        .radius(24)
                        .size(14)
                        .background(tint(board)),
                ),
                column(
                    "board-head-body",
                    4,
                    vec![
                        web::styled(
                            "board-head-title",
                            format!("r/{board}"),
                            web::style().size(20).bold().color(p.ink.clone()),
                        ),
                        web::styled(
                            "board-head-desc",
                            &b.description,
                            web::style().size(13).color(p.muted.clone()),
                        ),
                        web::badge(
                            "board-head-members",
                            format!("{} members", b.members),
                            web::style()
                                .size(11)
                                .width(130)
                                .background(p.accent.clone()),
                        ),
                    ],
                ),
                pill(
                    "board-head-sub",
                    if on { "Joined" } else { "Join" },
                    on,
                    &format!("/boards/{board}/subscribe"),
                    &here,
                    &[],
                    &p,
                ),
            ],
        )],
    ));
    let ids = s.in_board(board, ctx.tick);
    e.extend(listing(s, ctx, &p, &b.title, &ids, &here));
    web::themed_page(&format!("r/{board} / {}", s.brand), s.theme.clone(), e)
}
/// One reply, with everything under it. `depth` is the indent on a comment tree; Q&A answers are
/// flat by construction, so the indent never fires there.
fn reply_card(
    s: &ForumState,
    ctx: &ServiceContext,
    p: &Palette,
    t: &Thread,
    rid: &str,
    depth: u32,
    here: &str,
) -> Vec<PageElement> {
    let Some(r) = t.replies.iter().find(|r| r.id == rid) else {
        return vec![];
    };
    let accepted = t.accepted.as_deref() == Some(rid);
    let mut head = vec![
        web::thumbnail(
            &format!("{rid}-avatar"),
            initials(s.members.get(&r.author).map_or(&r.author, |m| &m.name)),
            web::style()
                .width(28)
                .height(28)
                .radius(14)
                .size(11)
                .background(tint(&r.author)),
        ),
        web::styled(
            &format!("{rid}-by"),
            format!("{} · {}", who(s, &r.author), ago(ctx.tick, r.tick)),
            web::style()
                .size(12)
                .medium()
                .color(p.muted.clone())
                .width(240),
        ),
    ];
    if let Some(m) = s.members.get(&r.author) {
        if !m.flair.is_empty() {
            head.push(web::badge(
                &format!("{rid}-flair"),
                &m.flair,
                web::style()
                    .size(10)
                    .width(150)
                    .background(p.surface.clone())
                    .color(p.muted.clone())
                    .border(p.line.clone()),
            ));
        }
        if s.qa() && m.reputation > 0 {
            head.push(web::badge(
                &format!("{rid}-rep"),
                format!("{} rep", m.reputation),
                web::style()
                    .size(10)
                    .width(110)
                    .background(p.accent.clone()),
            ));
        }
    }
    if accepted {
        head.push(web::badge(
            &format!("{rid}-accepted"),
            "Accepted",
            web::style().size(11).width(90).background("#2e7d32"),
        ));
    }
    let mut body = vec![
        web::row(&format!("{rid}-head"), 8, "center", head),
        web::styled(
            &format!("{rid}-body"),
            &r.body,
            web::style().size(14).color(p.ink.clone()),
        ),
    ];
    body.extend(web::links(&format!("{rid}-src"), &r.body));
    // Q&A: comments hang off the answer and the asker gets the accept control. Reddit and HN:
    // a reply box hangs off every comment, which is what makes the tree a tree.
    if s.qa() {
        for c in &r.comments {
            body.push(web::styled(
                &format!("{}-text", c.id),
                format!("{} — {}", c.body, who(s, &c.author)),
                web::style().size(12).color(p.muted.clone()),
            ));
        }
        let mut controls = vec![];
        if s.member_of(&ctx.actor)
            .is_some_and(|m| m.handle == t.author)
        {
            controls.push(pill(
                &format!("{rid}-accept"),
                if accepted { "Unaccept" } else { "Accept" },
                accepted,
                &format!("/threads/{}/accept", t.id),
                here,
                &[("reply", rid)],
                p,
            ));
        }
        controls.push(form_with(
            &format!("{rid}-comment"),
            &format!("/replies/{rid}/comments"),
            &[("body", "Add a comment", "")],
            &[("view", here)],
        ));
        body.push(web::row(&format!("{rid}-controls"), 8, "start", controls));
    } else {
        body.push(form_with(
            &format!("{rid}-reply"),
            &format!("/threads/{}/replies", t.id),
            &[("body", "Reply", "")],
            &[("parent", rid), ("view", here)],
        ));
    }
    // A nested comment is indented by an empty fixed-width cell, which is the whole visual
    // difference between a tree and a list.
    let indent = (depth * 24).min(144);
    let mut row = vec![votes(s, &ctx.actor, p, rid, "replies", here)];
    if indent > 0 {
        row.insert(
            0,
            web::styled(
                &format!("{rid}-indent"),
                "",
                web::style().width(indent).size(12),
            ),
        );
    }
    row.push(column(&format!("{rid}-col"), 8, body));
    let mut out = vec![web::card(
        &format!("reply-{rid}"),
        web::style()
            .padding(12)
            .radius(8)
            .background(if accepted {
                "#eef7ee".to_owned()
            } else {
                p.surface.clone()
            })
            .border(if accepted {
                "#2e7d32".to_owned()
            } else {
                p.line.clone()
            }),
        vec![web::row(&format!("{rid}-row"), 10, "start", row)],
    )];
    for child in s.children(t, Some(rid)) {
        out.extend(reply_card(s, ctx, p, t, &child, depth + 1, here));
    }
    out
}
impl ForumState {
    /// A thread by its stored id, or by the bare number a real question URL carries.
    fn thread_by_id(&self, id: &str) -> Option<&Thread> {
        self.threads
            .get(id)
            .or_else(|| self.threads.get(&format!("t-{id}")))
    }
}
fn thread_page(s: &ForumState, ctx: &ServiceContext, id: &str) -> SimResult<HttpResponse> {
    let Some(t) = s.thread_by_id(id) else {
        return web::error(404, "no such thread");
    };
    let p = Palette::of(&s.theme);
    let here = s.permalink(t);
    let mut e = nav(s, &p, &ctx.actor, &here);
    let mut head = vec![web::styled(
        "thread-title",
        &t.title,
        web::style().size(22).bold().color(p.ink.clone()),
    )];
    if let Some(u) = t.url.as_deref() {
        head.push(web::link("thread-out", u, u));
    }
    head.push(web::styled(
        "thread-by",
        match s.mode.as_str() {
            "subreddits" => format!(
                "r/{} · posted by {} {}",
                t.board,
                who(s, &t.author),
                ago(ctx.tick, t.tick)
            ),
            "qa" => format!(
                "asked {} by {} · {} views",
                ago(ctx.tick, t.tick),
                who(s, &t.author),
                t.views
            ),
            _ => format!(
                "{} points by {} {}",
                s.score_of(id),
                who(s, &t.author),
                ago(ctx.tick, t.tick)
            ),
        },
        web::style().size(12).color(p.muted.clone()),
    ));
    if !t.body.is_empty() {
        head.push(web::styled(
            "thread-body",
            &t.body,
            web::style().size(15).color(p.ink.clone()),
        ));
        head.extend(web::links("thread-src", &t.body));
    }
    if !t.tags.is_empty() {
        head.push(web::row(
            "thread-tags",
            6,
            "center",
            t.tags
                .iter()
                .enumerate()
                .map(|(i, tag)| {
                    web::card_action(
                        &format!("thread-tag-{i}"),
                        web::style()
                            .padding(6)
                            .radius(6)
                            .background(p.surface.clone())
                            .border(p.line.clone()),
                        web::visit(format!("/questions/tagged/{tag}")),
                        vec![web::styled(
                            &format!("thread-tag-{i}-text"),
                            tag,
                            web::style().size(11).medium().color(p.accent.clone()),
                        )],
                    )
                })
                .collect(),
        ));
    }
    e.push(web::card(
        "thread",
        web::style()
            .padding(16)
            .radius(8)
            .background(p.surface.clone())
            .border(p.line.clone()),
        vec![web::row(
            "thread-row",
            12,
            "start",
            vec![
                votes(s, &ctx.actor, &p, id, "threads", &here),
                column("thread-col", 8, head),
            ],
        )],
    ));
    e.push(web::divider("thread-rule"));
    let roots = s.children(t, None);
    e.push(web::styled(
        "replies-title",
        plural(
            if s.qa() {
                t.replies.len()
            } else {
                s.conversation_size(t)
            },
            s.reply_word(),
        ),
        web::style().size(17).bold().color(p.ink.clone()),
    ));
    e.push(form_with(
        "compose",
        &format!("/threads/{id}/replies"),
        &[(
            "body",
            match s.mode.as_str() {
                "qa" => "Your answer",
                _ => "Add a comment",
            },
            "",
        )],
        &[("view", &here)],
    ));
    for r in &roots {
        e.extend(reply_card(s, ctx, &p, t, r, 0, &here));
    }
    web::themed_page(&format!("{} / {}", t.title, s.brand), s.theme.clone(), e)
}
fn submit_page(s: &ForumState, ctx: &ServiceContext) -> SimResult<HttpResponse> {
    let p = Palette::of(&s.theme);
    let mut e = nav(s, &p, &ctx.actor, "/submit");
    let title = match s.mode.as_str() {
        "qa" => "Ask a question",
        "linkfeed" => "Submit a link",
        _ => "Create a post",
    };
    e.push(web::styled(
        "submit-head",
        title,
        web::style().size(19).bold().color(p.ink.clone()),
    ));
    if s.member_of(&ctx.actor).is_none() {
        e.push(web::styled(
            "submit-anon",
            "Sign in on this machine as a member of this site to post.",
            web::style().size(14).color(p.muted.clone()),
        ));
        return web::themed_page(&format!("{} / {title}", s.brand), s.theme.clone(), e);
    }
    let mut fields: Vec<(&str, &str, &str)> = vec![("title", "Title", "")];
    if s.subreddits() {
        fields.push(("board", "Community", ""));
    }
    if s.linkfeed() {
        fields.push(("url", "Link", ""));
    }
    if s.qa() {
        fields.push(("tags", "Tags (comma separated)", ""));
    }
    fields.push((
        "body",
        match s.mode.as_str() {
            "qa" => "What did you try?",
            _ => "Text (optional)",
        },
        "",
    ));
    e.push(form_with("submit", "/threads", &fields, &[]));
    if s.subreddits() {
        e.extend(board_rail(s, &ctx.actor, &p, "/submit"));
    }
    web::themed_page(&format!("{} / {title}", s.brand), s.theme.clone(), e)
}
fn search_page(s: &ForumState, ctx: &ServiceContext, q: &str) -> SimResult<HttpResponse> {
    let p = Palette::of(&s.theme);
    let mut e = nav(s, &p, &ctx.actor, "/search");
    e.push(web::form("search", "/search", &[("q", "Search", q)]));
    let ids = s.search(q, ctx.tick);
    let title = if q.trim().is_empty() {
        "Search".to_owned()
    } else {
        format!("{} results for \"{q}\"", ids.len())
    };
    e.extend(listing(s, ctx, &p, &title, &ids, "/search"));
    web::themed_page(&format!("{} / Search", s.brand), s.theme.clone(), e)
}
fn member_page(s: &ForumState, ctx: &ServiceContext, handle: &str) -> SimResult<HttpResponse> {
    let Some(m) = s.members.get(handle) else {
        return web::error(404, "no such member");
    };
    let p = Palette::of(&s.theme);
    let here = format!("/u/{handle}");
    let mut facts = vec![
        web::styled(
            "member-name",
            &m.name,
            web::style().size(22).bold().color(p.ink.clone()),
        ),
        web::styled(
            "member-handle",
            who(s, handle),
            web::style().size(13).color(p.muted.clone()),
        ),
    ];
    if !m.bio.is_empty() {
        facts.push(web::styled(
            "member-bio",
            &m.bio,
            web::style().size(14).color(p.ink.clone()),
        ));
    }
    facts.push(web::row(
        "member-meta",
        8,
        "center",
        vec![
            web::badge(
                "member-rep",
                format!("{} reputation", m.reputation),
                web::style().width(150).background(p.accent.clone()),
            ),
            web::badge(
                "member-posts",
                plural(s.by_author(handle).len(), s.item_word()),
                web::style()
                    .width(130)
                    .background(p.surface.clone())
                    .color(p.muted.clone())
                    .border(p.line.clone()),
            ),
        ],
    ));
    if !m.site.is_empty() {
        facts.push(web::link("member-site", &m.site, &m.site));
    }
    let mut e = nav(s, &p, &ctx.actor, &here);
    e.push(web::card(
        "member",
        web::style()
            .padding(16)
            .radius(8)
            .background(p.surface.clone())
            .border(p.line.clone()),
        vec![web::row(
            "member-row",
            14,
            "start",
            vec![
                web::thumbnail(
                    "member-avatar",
                    initials(&m.name),
                    web::style()
                        .width(56)
                        .height(56)
                        .radius(28)
                        .size(16)
                        .background(tint(handle)),
                ),
                column("member-body", 8, facts),
            ],
        )],
    ));
    let ids = s.by_author(handle);
    e.extend(listing(s, ctx, &p, "Posts", &ids, &here));
    web::themed_page(&format!("{} / {}", m.name, s.brand), s.theme.clone(), e)
}
/// The one router: every GET and every re-render after a successful form POST goes through here.
fn view(
    s: &ForumState,
    ctx: &ServiceContext,
    path: &str,
    q: Option<&str>,
    id: Option<&str>,
) -> SimResult<HttpResponse> {
    let parts: Vec<&str> = path.trim_matches('/').split('/').collect();
    match parts.as_slice() {
        [""] => front_page(s, ctx),
        ["newest"] => simple_list(s, ctx, "Newest", s.newest(), "/newest"),
        ["submit"] | ["ask"] => submit_page(s, ctx),
        ["search"] => search_page(s, ctx, q.unwrap_or_default()),
        ["r"] | ["boards"] => boards_page(s, ctx),
        ["item"] => match id {
            Some(id) => thread_page(s, ctx, id),
            None => web::error(404, "no item id"),
        },
        ["questions"] => simple_list(
            s,
            ctx,
            "All questions",
            s.front(&ctx.actor, ctx.tick),
            "/questions",
        ),
        ["questions", "tagged", tag] => simple_list(
            s,
            ctx,
            &format!("Questions tagged [{tag}]"),
            s.tagged(tag, ctx.tick),
            &format!("/questions/tagged/{tag}"),
        ),
        ["questions", id] => thread_page(s, ctx, id),
        ["u", handle] | ["users", handle] => member_page(s, ctx, handle),
        ["r", board] => board_page(s, ctx, board),
        ["r", _, "comments", id] => thread_page(s, ctx, id),
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
                return view(&s, ctx, &p, q.as_deref(), item.as_deref());
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
        let b = web::body(r)?;
        // The search form is a POST that reads; nothing else on this site is.
        if !api && parts.as_slice() == ["search"] {
            return view(&s, ctx, "/search", Some(&web::text(&b, "q")), None);
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
            rest.get("q").map(String::as_str),
            rest.get("id").map(String::as_str),
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
    /// Every page an agent can reach must survive `Page::validate`: unique ids, legal colours,
    /// legal spans. A duplicate id would make a control ambiguous to click.
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
                let page: cw_protocol::Page = serde_json::from_slice(&r.body).unwrap();
                page.validate().unwrap_or_else(|e| panic!("{url}: {e}"));
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
        let body = String::from_utf8(a.body).unwrap();
        assert!(body.contains("Accepted"), "the accepted answer is marked");
        assert!(body.contains("Your test iterates a HashMap."));
        assert!(body.contains("That was it."), "answer comments render");
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
        let body = String::from_utf8(page.body).unwrap();
        assert!(body.contains("Use a BTreeMap"));
        assert!(
            body.contains("Why does my BFS"),
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
        assert!(String::from_utf8(page.body).unwrap().contains("Joined"));
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
        let body = String::from_utf8(page.body).unwrap();
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
        let body = String::from_utf8(page.body).unwrap();
        assert!(
            body.contains("Determinism notes"),
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
        let body = String::from_utf8(page.body).unwrap();
        assert!(body.contains("148 points by tweber"));
        assert!(
            body.contains("theverge.com"),
            "the outbound host is on the row"
        );
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
        assert!(String::from_utf8(page.body)
            .unwrap()
            .contains("1 results for \\\"hashmap\\\""));
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
}
