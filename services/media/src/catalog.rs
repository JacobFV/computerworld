//! The music catalogue as a listener sees it: albums gathered from their tracks, a
//! personal library, listening history and the player. Both music sites and every native
//! music player read this one model, so a track saved in one is saved in all of them.
use super::player::{Player, Repeat};
use super::*;

/// Recently played entries kept per listener.
const HISTORY_LIMIT: usize = 50;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Album {
    pub id: String,
    pub title: String,
    pub artist: String,
    pub artist_name: String,
    pub year: String,
    pub genre: String,
    /// Track ids in running order.
    pub tracks: Vec<String>,
    /// Newest track's tick, so "new releases" is simply the highest.
    pub released: u64,
}
impl Album {
    /// "Album · 2024" style subtitle pieces most players print under the title.
    pub fn kind(&self) -> &'static str {
        if self.tracks.len() <= 2 {
            "Single"
        } else if self.tracks.len() <= 6 {
            "EP"
        } else {
            "Album"
        }
    }
}

pub fn album_id(item: &Value) -> String {
    match web::text(item, "album_id") {
        id if !id.is_empty() => id,
        _ => match web::text(item, "album") {
            album if !album.is_empty() => slug(&album),
            _ => slug(&web::text(item, "title")),
        },
    }
}
pub fn artist_name(state: &Value, artist: &str) -> String {
    record(state, "channels", artist)
        .map(|c| web::text(c, "name"))
        .unwrap_or_else(|| artist.to_owned())
}
fn year_of(item: &Value) -> String {
    web::text(item, "published")
        .rsplit(' ')
        .next()
        .filter(|y| y.len() == 4 && y.chars().all(|c| c.is_ascii_digit()))
        .unwrap_or_default()
        .to_owned()
}
fn titled(tag: &str) -> String {
    let mut chars = tag.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}
/// Every album, newest release first, tracks in running order.
pub fn albums(state: &Value) -> Vec<Album> {
    let mut out: Vec<Album> = Vec::new();
    let mut ids = keys(state, "items");
    ids.sort_by_key(|id| {
        let item = record(state, "items", id).cloned().unwrap_or(Value::Null);
        (
            num(&item, "track"),
            num(&item, "published_tick"),
            id.clone(),
        )
    });
    for id in ids {
        let Some(item) = record(state, "items", &id) else {
            continue;
        };
        let key = album_id(item);
        let tick = num(item, "published_tick");
        match out.iter_mut().find(|a| a.id == key) {
            Some(album) => {
                album.tracks.push(id.clone());
                album.released = album.released.max(tick);
            }
            None => {
                let meta = record(state, "albums", &key)
                    .cloned()
                    .unwrap_or(Value::Null);
                let artist = web::text(item, "channel");
                let pick = |k: &str, fallback: String| match web::text(&meta, k) {
                    s if s.is_empty() => fallback,
                    s => s,
                };
                out.push(Album {
                    id: key.clone(),
                    title: pick(
                        "title",
                        match web::text(item, "album") {
                            a if a.is_empty() => web::text(item, "title"),
                            a => a,
                        },
                    ),
                    artist_name: artist_name(state, &artist),
                    artist,
                    year: pick("year", year_of(item)),
                    genre: pick(
                        "genre",
                        web::strings(item, "tags")
                            .first()
                            .map(|t| titled(t))
                            .unwrap_or_default(),
                    ),
                    tracks: vec![id.clone()],
                    released: tick,
                });
            }
        }
    }
    out.sort_by_key(|a| (std::cmp::Reverse(a.released), a.id.clone()));
    out
}
pub fn album(state: &Value, id: &str) -> Option<Album> {
    albums(state).into_iter().find(|a| a.id == id)
}
pub fn duration_ms(state: &Value, id: &str) -> u64 {
    record(state, "items", id)
        .map(|item| num(item, "duration_s") * 1_000)
        .unwrap_or(0)
}
/// An artist's tracks, most played first: their "Top Songs".
pub fn top_tracks(state: &Value, artist: &str) -> Vec<String> {
    let mut ids: Vec<String> = keys(state, "items")
        .into_iter()
        .filter(|id| record(state, "items", id).is_some_and(|i| web::text(i, "channel") == artist))
        .collect();
    ids.sort_by_key(|id| {
        (
            std::cmp::Reverse(record(state, "items", id).map_or(0, |i| num(i, "plays"))),
            id.clone(),
        )
    });
    ids
}
/// Every track, most played first: the charts.
pub fn charts(state: &Value) -> Vec<String> {
    let mut ids = keys(state, "items");
    ids.sort_by_key(|id| {
        (
            std::cmp::Reverse(record(state, "items", id).map_or(0, |i| num(i, "plays"))),
            id.clone(),
        )
    });
    ids
}
/// A station starts with the artist and carries on with whatever shares their sound.
pub fn station(state: &Value, artist: &str) -> Vec<String> {
    let own = top_tracks(state, artist);
    let tags: Vec<String> = own
        .iter()
        .filter_map(|id| record(state, "items", id))
        .flat_map(|i| web::strings(i, "tags"))
        .collect();
    let related = charts(state).into_iter().filter(|id| {
        !own.contains(id)
            && record(state, "items", id)
                .is_some_and(|i| web::strings(i, "tags").iter().any(|t| tags.contains(t)))
    });
    own.clone().into_iter().chain(related).collect()
}
pub fn mood(state: &Value, tag: &str) -> Vec<String> {
    charts(state)
        .into_iter()
        .filter(|id| {
            record(state, "items", id).is_some_and(|i| {
                web::strings(i, "tags")
                    .iter()
                    .any(|t| t.eq_ignore_ascii_case(tag))
            })
        })
        .collect()
}
/// Songs in a listener's library, by title, as every library's Songs list sorts them.
pub fn library_songs(state: &Value, actor: &str) -> Vec<String> {
    let mut ids: Vec<String> = strings_at(state, "library", actor)
        .into_iter()
        .filter(|id| record(state, "items", id).is_some())
        .collect();
    ids.sort_by_key(|id| {
        (
            record(state, "items", id)
                .map(|i| web::text(i, "title").to_lowercase())
                .unwrap_or_default(),
            id.clone(),
        )
    });
    ids
}
/// Liked tracks, most recently liked first.
pub fn liked(state: &Value, actor: &str) -> Vec<String> {
    let mut ids = strings_at(state, "likes", actor);
    ids.retain(|id| record(state, "items", id).is_some());
    ids.reverse();
    ids
}
/// The tracks a context plays, in its own order.
pub fn context_tracks(state: &Value, actor: &str, context: &str) -> Vec<String> {
    let (kind, id) = context.split_once(':').unwrap_or((context, ""));
    match kind {
        "album" => album(state, id).map(|a| a.tracks).unwrap_or_default(),
        "playlist" => record(state, "playlists", id)
            .map(|p| web::strings(p, "items"))
            .unwrap_or_default()
            .into_iter()
            .filter(|t| record(state, "items", t).is_some())
            .collect(),
        "artist" => top_tracks(state, id),
        "station" => station(state, id),
        "mood" => mood(state, id),
        "library" => library_songs(state, actor),
        "liked" => liked(state, actor),
        "charts" => charts(state),
        _ => vec![],
    }
}
pub fn context_title(state: &Value, context: &str) -> String {
    let (kind, id) = context.split_once(':').unwrap_or((context, ""));
    match kind {
        "album" => album(state, id).map(|a| a.title).unwrap_or_default(),
        "playlist" => record(state, "playlists", id)
            .map(|p| web::text(p, "title"))
            .unwrap_or_default(),
        "artist" => artist_name(state, id),
        "station" => format!("{} Station", artist_name(state, id)),
        "mood" => titled(id),
        "library" => "Library".into(),
        "liked" => "Liked songs".into(),
        "charts" => "Top songs".into(),
        _ => String::new(),
    }
}
/// The listener's player as of `now`, or `None` when nothing was ever loaded.
pub fn player(state: &Value, actor: &str, now: u64) -> Option<Player> {
    let raw = record(state, "now_playing", actor)?;
    let mut parked: Player = serde_json::from_value(raw.clone()).ok()?;
    // An older seed names only the track and its list; the list is the queue.
    if parked.queue.is_empty() {
        if let Some(list) = &parked.list {
            let queue = context_tracks(state, actor, &format!("playlist:{list}"));
            if queue.contains(&parked.item) {
                parked.queue = queue;
                parked.index = parked
                    .queue
                    .iter()
                    .position(|id| *id == parked.item)
                    .unwrap_or(0);
            }
        }
    }
    if record(state, "items", &parked.item).is_none() && parked.queue.is_empty() {
        return None;
    }
    Some(parked.settle(now, |id| duration_ms(state, id)))
}
fn store(state: &mut Value, actor: &str, player: &Player) {
    let value = serde_json::to_value(player).unwrap_or(Value::Null);
    state
        .as_object_mut()
        .expect("state is an object")
        .entry("now_playing")
        .or_insert_with(|| json!({}))[actor] = value;
}
fn remember(state: &mut Value, actor: &str, id: &str) {
    let mut history = strings_at(state, "history", actor);
    history.retain(|h| h != id);
    history.insert(0, id.to_owned());
    history.truncate(HISTORY_LIMIT);
    state
        .as_object_mut()
        .expect("state is an object")
        .entry("history")
        .or_insert_with(|| json!({}))[actor] = json!(history);
}
fn started(state: &mut Value, actor: &str, player: &Player) {
    if let Some(id) = player.current() {
        let id = id.to_owned();
        bump(state, &id, "plays", 1);
        remember(state, actor, &id);
    }
}
fn field(body: &Value, key: &str) -> String {
    match body.get(key) {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Number(n)) => n.to_string(),
        Some(Value::Bool(b)) => b.to_string(),
        _ => String::new(),
    }
}
fn flag(body: &Value, key: &str) -> Option<bool> {
    match field(body, key).as_str() {
        "true" | "1" | "on" => Some(true),
        "false" | "0" | "off" => Some(false),
        _ => None,
    }
}
/// One transport command. Returns the player as it now stands.
pub fn command(
    state: &mut Value,
    ctx: &ServiceContext,
    body: &Value,
) -> std::result::Result<Player, String> {
    let actor = ctx.actor.clone();
    let now = ctx.tick;
    let action = field(body, "action");
    let seed = ctx.seed ^ now.rotate_left(17) ^ hash(&actor);
    if action == "play" {
        let item = field(body, "item");
        let context = match field(body, "context") {
            c if c.is_empty() => "track".to_owned(),
            c => c,
        };
        let mut source = context_tracks(state, &actor, &context);
        if !item.is_empty() && record(state, "items", &item).is_none() {
            return Err("item not found".into());
        }
        if source.is_empty() {
            if item.is_empty() {
                return Err(format!("{context} has nothing to play"));
            }
            source = vec![item.clone()];
        } else if !item.is_empty() && !source.contains(&item) {
            return Err("that track is not part of what was asked to play".into());
        }
        let index = source.iter().position(|id| *id == item).unwrap_or(0);
        // A shuffled play keeps the listener's repeat mode; so does a plain one.
        let previous = player(state, &actor, now);
        let mut next = Player::start(&context, source, index, now);
        if let Some(previous) = &previous {
            next.repeat = previous.repeat;
        }
        let shuffle = flag(body, "shuffle").unwrap_or(previous.is_some_and(|p| p.shuffle));
        if shuffle {
            if item.is_empty() {
                // "Shuffle" on an album starts anywhere in it.
                next.index = (seed % next.source.len().max(1) as u64) as usize;
            }
            next.set_shuffle(true, seed);
        }
        next.item = next.current().unwrap_or_default().to_owned();
        started(state, &actor, &next);
        store(state, &actor, &next);
        return Ok(next);
    }
    let mut p = player(state, &actor, now).ok_or("nothing is playing")?;
    let before = p.current().map(str::to_owned);
    match action.as_str() {
        "toggle" => p.toggle(),
        "pause" => p.playing = false,
        "resume" => p.playing = true,
        "next" => p.skip()?,
        "previous" => p.previous(),
        "seek" => {
            let at = field(body, "position_ms")
                .parse::<u64>()
                .map_err(|_| "seek needs position_ms")?;
            let length = duration_ms(state, p.current().unwrap_or_default());
            p.seek(at, length);
        }
        "shuffle" => {
            let on = flag(body, "on").unwrap_or(!p.shuffle);
            p.set_shuffle(on, seed);
        }
        "repeat" => {
            p.repeat = match field(body, "mode").as_str() {
                "" => p.repeat.next(),
                mode => Repeat::parse(mode).ok_or("repeat is off, all or one")?,
            };
        }
        "jump" => {
            let index = field(body, "index")
                .parse::<usize>()
                .map_err(|_| "jump needs an index")?;
            p.jump(index)?;
        }
        "queue" => {
            let item = field(body, "item");
            if record(state, "items", &item).is_none() {
                return Err("item not found".into());
            }
            p.enqueue(&item, flag(body, "next").unwrap_or(false));
        }
        other => return Err(format!("unknown player action {other}")),
    }
    p.tick = now;
    p.item = p.current().unwrap_or_default().to_owned();
    if p.current().map(str::to_owned) != before && p.playing {
        started(state, &actor, &p);
    }
    store(state, &actor, &p);
    Ok(p)
}
/// Starting a paused player for the first time from a queue action has nothing to queue
/// behind, so "Play Next" with nothing loaded simply plays the track.
pub fn queue_or_play(
    state: &mut Value,
    ctx: &ServiceContext,
    item: &str,
    next: bool,
) -> std::result::Result<Player, String> {
    if player(state, &ctx.actor, ctx.tick).is_none() {
        command(state, ctx, &json!({"action": "play", "item": item}))
    } else {
        command(
            state,
            ctx,
            &json!({"action": "queue", "item": item, "next": next}),
        )
    }
}
pub fn hash(s: &str) -> u64 {
    s.bytes().fold(0xcbf29ce484222325u64, |h, b| {
        (h ^ u64::from(b)).wrapping_mul(0x100000001b3)
    })
}
/// Saving is a toggle; an album saves (or, when all of it is saved, removes) every track.
pub fn save(state: &mut Value, actor: &str, ids: &[String]) -> bool {
    let mut library = strings_at(state, "library", actor);
    let all = ids.iter().all(|id| library.contains(id));
    if all {
        library.retain(|id| !ids.contains(id));
    } else {
        for id in ids {
            if !library.contains(id) {
                library.push(id.clone());
            }
        }
    }
    state
        .as_object_mut()
        .expect("state is an object")
        .entry("library")
        .or_insert_with(|| json!({}))[actor] = json!(library);
    !all
}
pub fn remove_from_playlist(
    state: &mut Value,
    actor: &str,
    list: &str,
    item: &str,
) -> std::result::Result<Value, String> {
    let playlist = record(state, "playlists", list)
        .cloned()
        .ok_or("playlist not found")?;
    let owner = web::text(&playlist, "owner");
    if !owner.is_empty() && owner != actor {
        return Err(format!("playlist {list} is not writable by {actor}"));
    }
    let mut items = web::strings(&playlist, "items");
    let before = items.len();
    items.retain(|i| i != item);
    if items.len() == before {
        return Err("that track is not in the playlist".into());
    }
    state["playlists"][list]["items"] = json!(items);
    Ok(state["playlists"][list].clone())
}
pub fn player_json(state: &Value, p: &Player) -> Value {
    let current = p.current().unwrap_or_default();
    json!({
        "item": current,
        "index": p.index,
        "queue": p.queue,
        "context": p.context,
        "context_title": context_title(state, &p.context),
        "position_ms": p.position_ms,
        "duration_ms": duration_ms(state, current),
        "playing": p.playing,
        "shuffle": p.shuffle,
        "repeat": p.repeat,
        "tick": p.tick,
    })
}
/// Everything a music application shows, in one reply: the catalogue and this listener's
/// library, likes, playlists, history and player at `now`.
pub fn snapshot(state: &Value, actor: &str, now: u64) -> Value {
    let artists: Vec<Value> = keys(state, "channels")
        .iter()
        .filter_map(|id| record(state, "channels", id).map(|c| (id, c)))
        .map(|(id, c)| {
            json!({
                "id": id,
                "name": web::text(c, "name"),
                "followers": num(c, "subscribers"),
                "about": web::text(c, "about"),
            })
        })
        .collect();
    let albums: Vec<Value> = albums(state)
        .into_iter()
        .map(|a| {
            json!({
                "id": a.id, "title": a.title, "artist": a.artist, "year": a.year,
                "genre": a.genre, "kind": a.kind(), "tracks": a.tracks,
            })
        })
        .collect();
    let tracks: Vec<Value> = by_recency(state, |_| true)
        .iter()
        .filter_map(|id| record(state, "items", id).map(|i| (id, i)))
        .map(|(id, i)| {
            json!({
                "id": id,
                "title": web::text(i, "title"),
                "artist": web::text(i, "channel"),
                "album": album_id(i),
                "duration_ms": num(i, "duration_s") * 1_000,
                "plays": num(i, "plays"),
                "explicit": i.get("explicit").and_then(Value::as_bool).unwrap_or(false),
                "tags": web::strings(i, "tags"),
            })
        })
        .collect();
    let playlists: Vec<Value> = keys(state, "playlists")
        .iter()
        .filter_map(|id| record(state, "playlists", id).map(|p| (id, p)))
        .map(|(id, p)| {
            let owner = web::text(p, "owner");
            json!({
                "id": id,
                "title": web::text(p, "title"),
                "owner": owner,
                "items": web::strings(p, "items"),
                "editable": owner.is_empty() || owner == actor,
            })
        })
        .collect();
    json!({
        "brand": web::text(state, "brand"),
        "mode": web::text(state, "mode"),
        "tick": now,
        "artists": artists,
        "albums": albums,
        "tracks": tracks,
        "playlists": playlists,
        "liked": strings_at(state, "likes", actor),
        "library": strings_at(state, "library", actor),
        "history": strings_at(state, "history", actor),
        "subscriptions": strings_at(state, "subscriptions", actor),
        "player": player(state, actor, now).map(|p| player_json(state, &p)),
    })
}
/// Search across tracks, albums, artists and playlists, each list in relevance order.
pub fn search(state: &Value, query: &str) -> Value {
    let words: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
    let hit =
        |hay: String| !words.is_empty() && words.iter().all(|w| hay.to_lowercase().contains(w));
    let tracks: Vec<String> = charts(state)
        .into_iter()
        .filter(|id| matches(state, id, query))
        .collect();
    let albums: Vec<String> = albums(state)
        .into_iter()
        .filter(|a| hit(format!("{} {} {}", a.title, a.artist_name, a.genre)))
        .map(|a| a.id)
        .collect();
    let artists: Vec<String> = keys(state, "channels")
        .into_iter()
        .filter(|id| hit(artist_name(state, id)))
        .collect();
    let playlists: Vec<String> = keys(state, "playlists")
        .into_iter()
        .filter(|id| record(state, "playlists", id).is_some_and(|p| hit(web::text(p, "title"))))
        .collect();
    json!({"query": query, "tracks": tracks, "albums": albums, "artists": artists, "playlists": playlists})
}
