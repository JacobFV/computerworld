//! The music player: Apple Music on macOS and iOS, YouTube Music on Android, Media Player on
//! Windows and Rhythmbox on Ubuntu. One model, five faces.
//!
//! Everything shown comes from the `media` service the world backs the application with
//! (spotify.com's catalogue, or music.youtube.com's on Android): `GET /api/catalog` returns
//! the catalogue with this listener's library, likes, playlists, history and player, and
//! every control is a real request — `POST /api/player` for transport, the library, like
//! and playlist routes for the rest — after which the catalogue is read again, so the
//! screen only ever shows what the service committed.
//!
//! Playback is simulated but not faked. The service records where the player was at one
//! world tick and whether it was running; the position on screen is that plus the world
//! clock since, carried across track boundaries by the queue and the repeat mode exactly as
//! the service carries it (`Live`), so the scrubber moves as the world's time does.
use super::{push_bounded, Status};
use crate::desktop_scene::{DesktopTheme, Painter};
use crate::AppEffect;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

mod apple;
mod art;
mod rhythmbox;
mod wmp;
mod ytm;

/// Rows retained from any one reply. A catalogue may be any size; the window is not.
pub const LIST_LIMIT: usize = 500;
const QUERY_LIMIT: usize = 120;
const TITLE_LIMIT: usize = 80;
/// Views remembered for Back.
const HISTORY_LIMIT: usize = 16;
/// Steps across a scrubber. `music:seek:<n>` seeks to n/SEEK_STEPS of the track.
pub const SEEK_STEPS: u64 = 1_000;
/// Lines of lyrics kept per song.
const LYRIC_LIMIT: usize = 400;
/// Steps across a volume slider: each sets the volume to a multiple of 5%.
pub const VOLUME_STEPS: u32 = 20;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Shelf {
    /// Recently added: the library's front page on Apple Music.
    #[default]
    Recent,
    Playlists,
    Artists,
    Albums,
    Songs,
}
impl Shelf {
    pub const ALL: [Shelf; 5] = [
        Shelf::Recent,
        Shelf::Playlists,
        Shelf::Artists,
        Shelf::Albums,
        Shelf::Songs,
    ];
    pub fn key(self) -> &'static str {
        match self {
            Self::Recent => "recent",
            Self::Playlists => "playlists",
            Self::Artists => "artists",
            Self::Albums => "albums",
            Self::Songs => "songs",
        }
    }
    pub fn title(self) -> &'static str {
        match self {
            Self::Recent => "Recently Added",
            Self::Playlists => "Playlists",
            Self::Artists => "Artists",
            Self::Albums => "Albums",
            Self::Songs => "Songs",
        }
    }
    fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|shelf| shelf.key() == s)
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum View {
    /// Home / Listen Now.
    #[default]
    Home,
    /// New releases (Apple Music's New tab; YouTube Music's Explore).
    New,
    Radio,
    Library(Shelf),
    Album(String),
    Artist(String),
    Playlist(String),
    /// Liked songs: Favorite Songs, Liked Music.
    Liked,
    Search,
    Queue,
    /// The full-screen player a phone opens from its mini player.
    NowPlaying,
}

/// Which field keystrokes go to. Text with no field focused is refused rather than lost.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Focus {
    #[default]
    None,
    Search,
    NewPlaylist,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Repeat {
    #[default]
    Off,
    All,
    One,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Artist {
    pub id: String,
    pub name: String,
    pub followers: u64,
    pub about: String,
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Album {
    pub id: String,
    pub title: String,
    pub artist: String,
    pub year: String,
    pub genre: String,
    pub kind: String,
    pub tracks: Vec<String>,
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Track {
    pub id: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub duration_ms: u64,
    pub plays: u64,
    pub explicit: bool,
    pub tags: Vec<String>,
    /// Time-synced lyrics, `(start_ms, line)` in the order they are sung; empty when the
    /// service has none for the song.
    pub lyrics: Vec<(u64, String)>,
}
impl Track {
    pub fn instrumental(&self) -> bool {
        self.tags.iter().any(|t| t == "instrumental")
    }
    /// The line being sung `position_ms` into the song.
    pub fn sung(&self, position_ms: u64) -> Option<usize> {
        self.lyrics.iter().rposition(|(at, _)| *at <= position_ms)
    }
}
/// A speaker the service knows this listener's account can play on. Whether it answers
/// is found out by asking it (`Music::reachable`).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Device {
    pub id: String,
    pub name: String,
    /// `speaker` or `tv`.
    pub kind: String,
    pub url: String,
    /// `airplay`, `cast`, `dlna`.
    pub protocols: Vec<String>,
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Playlist {
    pub id: String,
    pub title: String,
    pub owner: String,
    pub items: Vec<String>,
    pub editable: bool,
}
/// The player as the service last reported it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Player {
    pub item: String,
    pub index: usize,
    pub queue: Vec<String>,
    pub context: String,
    pub context_title: String,
    pub position_ms: u64,
    pub duration_ms: u64,
    pub playing: bool,
    pub shuffle: bool,
    pub repeat: Repeat,
    /// World tick at which `position_ms` was true.
    pub tick: u64,
    /// The player's own volume, 0 to 100.
    #[serde(default = "full_volume")]
    pub volume: u8,
    pub muted: bool,
    /// The speaker the session was handed to, empty when it plays here.
    pub device: String,
    pub device_name: String,
}
fn full_volume() -> u8 {
    100
}
impl Player {
    /// What reaches the output: nothing when muted.
    pub fn audible(&self) -> u8 {
        if self.muted {
            0
        } else {
            self.volume
        }
    }
}
/// One `GET /api/catalog` reply.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Catalog {
    pub brand: String,
    pub artists: Vec<Artist>,
    pub albums: Vec<Album>,
    /// Newest first, the service's order.
    pub tracks: Vec<Track>,
    pub playlists: Vec<Playlist>,
    pub liked: Vec<String>,
    pub library: Vec<String>,
    pub history: Vec<String>,
    /// Artists this listener subscribes to (YouTube Music) or follows.
    pub subscriptions: Vec<String>,
    pub player: Option<Player>,
    /// Speakers signed in to the account.
    pub devices: Vec<Device>,
}
impl Catalog {
    pub fn device(&self, id: &str) -> Option<&Device> {
        self.devices.iter().find(|d| d.id == id)
    }
    pub fn track(&self, id: &str) -> Option<&Track> {
        self.tracks.iter().find(|t| t.id == id)
    }
    pub fn album(&self, id: &str) -> Option<&Album> {
        self.albums.iter().find(|a| a.id == id)
    }
    pub fn artist(&self, id: &str) -> Option<&Artist> {
        self.artists.iter().find(|a| a.id == id)
    }
    pub fn playlist(&self, id: &str) -> Option<&Playlist> {
        self.playlists.iter().find(|p| p.id == id)
    }
    pub fn artist_name(&self, id: &str) -> String {
        self.artist(id)
            .map(|a| a.name.clone())
            .unwrap_or_else(|| id.to_owned())
    }
    /// Albums with at least one track in the library, newest first.
    pub fn library_albums(&self) -> Vec<&Album> {
        self.albums
            .iter()
            .filter(|a| a.tracks.iter().any(|t| self.library.contains(t)))
            .collect()
    }
    /// Artists with a track in the library, by name.
    pub fn library_artists(&self) -> Vec<&Artist> {
        let mut artists: Vec<&Artist> = self
            .artists
            .iter()
            .filter(|a| {
                self.library
                    .iter()
                    .filter_map(|t| self.track(t))
                    .any(|t| t.artist == a.id)
            })
            .collect();
        artists.sort_by_key(|a| a.name.to_lowercase());
        artists
    }
    /// Library songs by title, the way every Songs list sorts.
    pub fn library_songs(&self) -> Vec<&Track> {
        let mut songs: Vec<&Track> = self
            .library
            .iter()
            .filter_map(|id| self.track(id))
            .collect();
        songs.sort_by_key(|t| (t.title.to_lowercase(), t.id.clone()));
        songs
    }
    /// Recently added: newest library entries first.
    pub fn recently_added(&self) -> Vec<&Album> {
        let mut seen: Vec<&str> = vec![];
        self.library
            .iter()
            .rev()
            .filter_map(|id| self.track(id))
            .filter_map(|t| self.album(&t.album))
            .filter(|a| {
                let fresh = !seen.contains(&a.id.as_str());
                seen.push(&a.id);
                fresh
            })
            .collect()
    }
    /// An artist's songs, most played first.
    pub fn top_songs(&self, artist: &str) -> Vec<&Track> {
        let mut songs: Vec<&Track> = self.tracks.iter().filter(|t| t.artist == artist).collect();
        songs.sort_by_key(|t| (std::cmp::Reverse(t.plays), t.id.clone()));
        songs
    }
    pub fn liked_songs(&self) -> Vec<&Track> {
        self.liked
            .iter()
            .rev()
            .filter_map(|id| self.track(id))
            .collect()
    }
    /// Moods and genres carried by some track, sorted.
    pub fn tags(&self) -> Vec<String> {
        let mut tags: Vec<String> = self.tracks.iter().flat_map(|t| t.tags.clone()).collect();
        tags.sort();
        tags.dedup();
        tags
    }
    fn bound(&mut self) {
        self.artists.truncate(LIST_LIMIT);
        self.albums.truncate(LIST_LIMIT);
        self.tracks.truncate(LIST_LIMIT);
        self.playlists.truncate(LIST_LIMIT);
        self.liked.truncate(LIST_LIMIT);
        self.library.truncate(LIST_LIMIT);
        self.history.truncate(LIST_LIMIT);
        self.devices.truncate(LIST_LIMIT);
        for track in &mut self.tracks {
            track.lyrics.truncate(LYRIC_LIMIT);
        }
    }
}
/// A search reply: ids, each list in the service's relevance order.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Results {
    pub query: String,
    pub tracks: Vec<String>,
    pub albums: Vec<String>,
    pub artists: Vec<String>,
    pub playlists: Vec<String>,
}
impl Results {
    pub fn is_empty(&self) -> bool {
        self.tracks.is_empty()
            && self.albums.is_empty()
            && self.artists.is_empty()
            && self.playlists.is_empty()
    }
}
/// Where playback is at a given world tick.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Live {
    pub index: usize,
    pub id: String,
    pub position_ms: u64,
    pub duration_ms: u64,
    pub playing: bool,
}
/// The song menu ("…", "⋮", right click): what it is open on.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Menu {
    pub track: String,
    /// The "Add to Playlist" submenu is showing.
    pub playlists: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Music {
    pub base: String,
    pub view: View,
    /// Views to return to, most recent last.
    pub back: Vec<View>,
    /// The service's last catalogue reply. Boxed: `AppState` keeps every application side
    /// by side, so the largest one sets the size of them all.
    pub catalog: Box<Catalog>,
    pub results: Option<Box<Results>>,
    pub query: String,
    pub focus: Focus,
    pub menu: Option<Menu>,
    /// YouTube Music's mood chip, when one is on.
    pub mood: Option<String>,
    /// Rhythmbox browser selections.
    #[serde(default)]
    pub filter_artist: Option<String>,
    #[serde(default)]
    pub filter_album: Option<String>,
    /// Title being typed for a new playlist; `None` when the composer is closed.
    pub draft: Option<String>,
    pub status: Status,
    /// Set once the first catalogue reply has landed.
    pub loaded: bool,
    /// Lyrics are showing: Apple Music's lyrics panel, the phones' lyrics view.
    #[serde(default)]
    pub lyrics: bool,
    /// A popup over the player: the output picker, or a volume popover.
    #[serde(default)]
    pub popup: Option<Popup>,
    /// What each device answered when the output picker last asked it: `true` when it
    /// is on the network and answering. A device not yet heard from is absent.
    #[serde(default)]
    pub reachable: BTreeMap<String, bool>,
}
/// Popups a player opens over itself.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Popup {
    /// AirPlay, Cast, "Cast to device": where the sound goes.
    Output,
    /// Rhythmbox's volume button popover.
    Volume,
}
/// Which of a device's protocols the platform's player speaks, and what it calls the
/// device it is running on.
pub fn output_protocol(theme: DesktopTheme) -> Option<(&'static str, &'static str)> {
    match theme {
        DesktopTheme::Macos => Some(("airplay", "This Mac")),
        DesktopTheme::Ios => Some(("airplay", "iPhone")),
        DesktopTheme::Android => Some(("cast", "This phone")),
        DesktopTheme::Windows => Some(("dlna", "This PC")),
        // Rhythmbox plays on the machine it runs on and casts nowhere.
        DesktopTheme::Ubuntu => None,
    }
}

impl Music {
    pub const KIND: &'static str = "music";
    pub fn launch(argument: &str, window: u64, _clock_us: u64) -> (Self, Vec<AppEffect>) {
        let (service, _) = super::launch_target(argument);
        let app = Self {
            base: if service.is_empty() {
                "http://spotify.com/".into()
            } else {
                service.to_owned()
            },
            view: View::Home,
            back: vec![],
            catalog: Box::default(),
            results: None,
            query: String::new(),
            focus: Focus::None,
            menu: None,
            mood: None,
            filter_artist: None,
            filter_album: None,
            draft: None,
            status: Status::Loading,
            loaded: false,
            lyrics: false,
            popup: None,
            reachable: BTreeMap::new(),
        };
        let effects = vec![app.catalog_request(window)];
        (app, effects)
    }
    pub fn kind(&self) -> &'static str {
        Self::KIND
    }
    pub fn title(&self, theme: DesktopTheme) -> String {
        match theme {
            DesktopTheme::Windows => "Media Player",
            DesktopTheme::Ubuntu => "Rhythmbox",
            DesktopTheme::Android => "YouTube Music",
            _ => "Music",
        }
        .into()
    }
    pub fn document(&self) -> String {
        String::new()
    }
    pub fn caption(&self) -> String {
        match self.now() {
            Some(t) => format!("{} — {}", t.title, self.catalog.artist_name(&t.artist)),
            None => String::new(),
        }
    }
    pub fn modified(&self) -> bool {
        self.draft.is_some()
    }
    /// Whether keystrokes go into a field. A player with no field focused takes none, so
    /// a phone shows no keyboard over it.
    pub fn takes_text(&self) -> bool {
        self.focus != Focus::None
    }
    /// The track the player has loaded, as the service last reported it.
    pub fn now(&self) -> Option<&Track> {
        let p = self.catalog.player.as_ref()?;
        self.catalog.track(p.queue.get(p.index).unwrap_or(&p.item))
    }
    /// Where playback is at `clock_us`: the service's recorded position plus the world
    /// time since, carried through the queue the way the service carries it.
    pub fn live(&self, clock_us: u64) -> Option<Live> {
        let p = self.catalog.player.as_ref()?;
        if p.queue.is_empty() {
            return None;
        }
        let length = |i: usize| {
            self.catalog
                .track(&p.queue[i])
                .map_or(0, |t| t.duration_ms)
                .max(1_000)
        };
        let mut index = p.index.min(p.queue.len() - 1);
        let mut position = p.position_ms;
        let mut playing = p.playing;
        if playing && clock_us > p.tick {
            let mut elapsed = (clock_us - p.tick) / 1_000;
            for _ in 0..=p.queue.len() * 2 + 2 {
                let now = length(index);
                let left = now.saturating_sub(position);
                if elapsed < left {
                    position += elapsed;
                    break;
                }
                elapsed -= left;
                position = 0;
                match p.repeat {
                    Repeat::One => elapsed %= now,
                    _ if index + 1 < p.queue.len() => index += 1,
                    Repeat::All => {
                        index = 0;
                        let lap: u64 = (0..p.queue.len()).map(length).sum();
                        elapsed %= lap.max(1);
                    }
                    Repeat::Off => {
                        playing = false;
                        break;
                    }
                }
            }
        }
        Some(Live {
            index,
            id: p.queue[index].clone(),
            position_ms: position,
            duration_ms: length(index),
            playing,
        })
    }
    /// Whether Next has anywhere to go: a later track, or repeat-all to wrap to the first.
    pub fn can_next(&self, clock_us: u64) -> bool {
        match (self.catalog.player.as_ref(), self.live(clock_us)) {
            (Some(p), Some(live)) => live.index + 1 < p.queue.len() || p.repeat == Repeat::All,
            _ => false,
        }
    }
    fn url(&self, suffix: &str) -> String {
        format!("{}{suffix}", self.base.trim_end_matches('/'))
    }
    fn catalog_request(&self, window: u64) -> AppEffect {
        AppEffect::Http {
            window,
            tag: "catalog".into(),
            method: "GET".into(),
            url: self.url("/api/catalog"),
            body: String::new(),
        }
    }
    fn post(&self, window: u64, suffix: &str, body: serde_json::Value) -> AppEffect {
        AppEffect::Http {
            window,
            tag: "done".into(),
            method: "POST".into(),
            url: self.url(suffix),
            body: body.to_string(),
        }
    }
    fn player(&self, window: u64, body: serde_json::Value) -> Vec<AppEffect> {
        vec![self.post(window, "/api/player", body)]
    }
    pub fn offline(&mut self, tag: &str, reason: &str) {
        // A speaker that does not answer is a fact about the speaker, not the service.
        if let Some(id) = tag.strip_prefix("probe:") {
            self.reachable.insert(id.to_owned(), false);
            return;
        }
        let device = |id: &str| {
            self.catalog
                .device(id)
                .map_or_else(|| id.to_owned(), |d| d.name.clone())
        };
        self.status = match tag.split_once(':') {
            Some(("cast", id)) => {
                self.reachable.insert(id.to_owned(), false);
                Status::Offline(format!("Couldn't connect to {}: {reason}", device(id)))
            }
            Some(("sync", id)) => {
                Status::Offline(format!("{} is not responding: {reason}", device(id)))
            }
            _ => Status::Offline(reason.to_owned()),
        };
    }
    /// The session as a speaker is handed it: the player and what each queued song is.
    fn session(&self, p: &Player) -> serde_json::Value {
        let tracks: serde_json::Map<String, serde_json::Value> = p
            .queue
            .iter()
            .filter_map(|id| self.catalog.track(id))
            .map(|t| {
                (
                    t.id.clone(),
                    serde_json::json!({
                        "title": t.title,
                        "artist": self.catalog.artist_name(&t.artist),
                        "album": self.catalog.album(&t.album).map(|a| a.title.clone()).unwrap_or_default(),
                        "duration_ms": t.duration_ms,
                    }),
                )
            })
            .collect();
        serde_json::json!({
            "source": self.base,
            "player": {
                "item": p.item, "queue": p.queue, "index": p.index, "context": p.context,
                "position_ms": p.position_ms, "playing": p.playing, "shuffle": p.shuffle,
                "repeat": p.repeat, "volume": p.volume, "muted": p.muted, "tick": p.tick,
            },
            "tracks": tracks,
        })
    }
    fn device_post(
        &self,
        window: u64,
        tag: String,
        device: &Device,
        route: &str,
        body: serde_json::Value,
    ) -> AppEffect {
        AppEffect::Http {
            window,
            tag,
            method: "POST".into(),
            url: format!("{}{route}", device.url.trim_end_matches('/')),
            body: body.to_string(),
        }
    }
    /// Keep the speaker that is playing the session current: whatever the service now
    /// says the player is, the speaker is sent.
    fn sync(&self, window: u64) -> Vec<AppEffect> {
        let Some(p) = &self.catalog.player else {
            return vec![];
        };
        match self.catalog.device(&p.device) {
            Some(device) if !p.queue.is_empty() => vec![self.device_post(
                window,
                format!("sync:{}", device.id),
                device,
                "/api/cast",
                self.session(p),
            )],
            _ => vec![],
        }
    }
    pub fn http(
        &mut self,
        window: u64,
        tag: &str,
        status: u16,
        body: &str,
    ) -> Result<Vec<AppEffect>, String> {
        // Replies from speakers, not from the service.
        if let Some(id) = tag.strip_prefix("probe:") {
            self.reachable.insert(id.to_owned(), status == 200);
            return Ok(vec![]);
        }
        if let Some(id) = tag.strip_prefix("cast:") {
            if status != 200 {
                self.reachable.insert(id.to_owned(), false);
                self.status = Status::from_status(status, body);
                return Ok(vec![]);
            }
            // The speaker has the session: the service now sends the sound there, and the
            // speaker it came from lets it go.
            let mut effects = vec![];
            if let Some(previous) = self
                .catalog
                .player
                .as_ref()
                .and_then(|p| self.catalog.device(&p.device))
                .filter(|d| d.id != id)
            {
                effects.push(self.device_post(
                    window,
                    format!("stopped:{}", previous.id),
                    previous,
                    "/api/stop",
                    serde_json::json!({}),
                ));
            }
            effects.extend(self.player(
                window,
                serde_json::json!({"action": "output", "device": id}),
            ));
            self.status = Status::Loading;
            return Ok(effects);
        }
        if tag.starts_with("sync:") || tag.starts_with("stopped:") {
            if status != 200 {
                self.status = Status::from_status(status, body);
            }
            return Ok(vec![]);
        }
        let outcome = Status::from_status(status, body);
        if outcome != Status::Idle {
            // A refused command (nothing queued after this track, say) leaves the screen as
            // it was and says why; a refused catalogue read is the whole app's state.
            self.status = outcome;
            return Ok(vec![]);
        }
        match tag {
            "catalog" => {
                let mut catalog: Catalog = serde_json::from_str(body).unwrap_or_default();
                catalog.bound();
                *self.catalog = catalog;
                self.loaded = true;
                self.status = Status::Idle;
                if let Some(menu) = &self.menu {
                    if self.catalog.track(&menu.track).is_none() {
                        self.menu = None;
                    }
                }
                Ok(self.sync(window))
            }
            "search" => {
                let mut results: Results = serde_json::from_str(body).unwrap_or_default();
                for list in [
                    &mut results.tracks,
                    &mut results.albums,
                    &mut results.artists,
                    &mut results.playlists,
                ] {
                    list.truncate(LIST_LIMIT);
                }
                self.results = Some(Box::new(results));
                self.status = Status::Idle;
                Ok(vec![])
            }
            // A command's reply says what changed; the catalogue is then re-read rather
            // than patched, so a local guess can never drift from the service.
            "done" => {
                self.draft = None;
                self.status = Status::Loading;
                Ok(vec![self.catalog_request(window)])
            }
            other => Err(format!("unexpected music reply {other}")),
        }
    }
    pub fn text(&mut self, text: &str) -> Result<(), String> {
        match self.focus {
            Focus::Search => {
                push_bounded(&mut self.query, text, QUERY_LIMIT);
                Ok(())
            }
            Focus::NewPlaylist => {
                let draft = self.draft.as_mut().ok_or("no playlist is being named")?;
                push_bounded(draft, text, TITLE_LIMIT);
                Ok(())
            }
            Focus::None => Err("no music field is focused".into()),
        }
    }
    pub fn key(&mut self, window: u64, key: &str, clock_us: u64) -> Result<Vec<AppEffect>, String> {
        match (key, self.focus) {
            ("Backspace", Focus::Search) => {
                self.query.pop();
                Ok(vec![])
            }
            ("Backspace", Focus::NewPlaylist) => {
                self.draft
                    .as_mut()
                    .ok_or("no playlist is being named")?
                    .pop();
                Ok(vec![])
            }
            ("Enter", Focus::Search) => self.click(window, "music:search", clock_us),
            ("Enter", Focus::NewPlaylist) => self.click(window, "music:create", clock_us),
            ("Escape", _) => {
                self.focus = Focus::None;
                self.draft = None;
                self.menu = None;
                self.popup = None;
                Ok(vec![])
            }
            // Media keys and the players' own shortcuts.
            (" " | "Space" | "MediaPlayPause", Focus::None) => {
                self.click(window, "music:toggle", clock_us)
            }
            ("MediaTrackNext", _) => self.click(window, "music:next", clock_us),
            ("MediaTrackPrevious", _) => self.click(window, "music:previous", clock_us),
            (other, _) => Err(format!("unsupported music key {other}")),
        }
    }
    fn go(&mut self, view: View) {
        if self.view != view {
            let from = std::mem::replace(&mut self.view, view);
            self.back.push(from);
            if self.back.len() > HISTORY_LIMIT {
                self.back.remove(0);
            }
        }
        self.menu = None;
        self.popup = None;
        self.focus = Focus::None;
    }
    /// A top-level destination (a tab or a sidebar entry) starts a fresh Back history.
    fn tab(&mut self, view: View) {
        self.view = view;
        self.back.clear();
        self.menu = None;
        self.popup = None;
        self.focus = Focus::None;
    }
    /// Rhythmbox shows an album or artist page as its browser narrowed to it; touching the
    /// browser again returns to the whole collection.
    fn leave_page(&mut self) {
        let artist = match &self.view {
            View::Album(id) => self.catalog.album(id).map(|a| a.artist.clone()),
            View::Artist(id) => Some(id.clone()),
            _ => return,
        };
        self.filter_artist = artist;
        self.go(View::Library(Shelf::Songs));
    }
    fn track(&self, id: &str) -> Result<&Track, String> {
        self.catalog
            .track(id)
            .ok_or_else(|| "track not found".into())
    }
    /// Contexts a play command may name, checked against what the catalogue holds.
    fn context_ok(&self, context: &str) -> bool {
        let (kind, id) = context.split_once(':').unwrap_or((context, ""));
        match kind {
            "album" => self.catalog.album(id).is_some(),
            "playlist" => self.catalog.playlist(id).is_some(),
            "artist" | "station" => self.catalog.artist(id).is_some(),
            "mood" => self.catalog.tags().iter().any(|t| t == id),
            "library" | "liked" | "charts" | "track" => true,
            _ => false,
        }
    }
    pub fn click(
        &mut self,
        window: u64,
        target: &str,
        clock_us: u64,
    ) -> Result<Vec<AppEffect>, String> {
        let command = target
            .strip_prefix("music:")
            .ok_or("interaction does not belong to music")?;
        let (verb, arg) = command.split_once(':').unwrap_or((command, ""));
        match verb {
            "reload" => {
                self.status = Status::Loading;
                Ok(vec![self.catalog_request(window)])
            }
            "home" => {
                self.tab(View::Home);
                Ok(vec![])
            }
            "new" => {
                self.tab(View::New);
                Ok(vec![])
            }
            "radio" => {
                self.tab(View::Radio);
                Ok(vec![])
            }
            "library" => {
                let shelf = if arg.is_empty() {
                    Shelf::Recent
                } else {
                    Shelf::parse(arg).ok_or("no such library section")?
                };
                self.tab(View::Library(shelf));
                Ok(vec![])
            }
            "liked" => {
                self.go(View::Liked);
                Ok(vec![])
            }
            "queue" => {
                self.go(View::Queue);
                Ok(vec![])
            }
            "expand" => {
                self.catalog.player.as_ref().ok_or("nothing is playing")?;
                self.go(View::NowPlaying);
                Ok(vec![])
            }
            "back" => {
                self.view = self.back.pop().ok_or("there is nowhere to go back to")?;
                self.menu = None;
                Ok(vec![])
            }
            "album" => {
                self.catalog.album(arg).ok_or("album not found")?;
                self.go(View::Album(arg.to_owned()));
                Ok(vec![])
            }
            "artist" => {
                self.catalog.artist(arg).ok_or("artist not found")?;
                self.go(View::Artist(arg.to_owned()));
                Ok(vec![])
            }
            "playlist" => {
                self.catalog.playlist(arg).ok_or("playlist not found")?;
                self.go(View::Playlist(arg.to_owned()));
                Ok(vec![])
            }
            "mood" => {
                if !self.catalog.tags().iter().any(|t| t == arg) {
                    return Err("no such mood".into());
                }
                self.mood = if self.mood.as_deref() == Some(arg) {
                    None
                } else {
                    Some(arg.to_owned())
                };
                // A mood chip filters Home; picked anywhere else, it takes you there.
                if self.view != View::Home {
                    self.tab(View::Home);
                }
                Ok(vec![])
            }
            "search-field" => {
                if self.view != View::Search {
                    self.go(View::Search);
                }
                self.focus = Focus::Search;
                Ok(vec![])
            }
            "clear" => {
                self.query.clear();
                self.results = None;
                self.focus = Focus::Search;
                Ok(vec![])
            }
            "search" => {
                if self.query.trim().is_empty() {
                    return Err("a search needs something to look for".into());
                }
                if self.view != View::Search {
                    self.go(View::Search);
                }
                self.focus = Focus::None;
                self.status = Status::Loading;
                Ok(vec![AppEffect::Http {
                    window,
                    tag: "search".into(),
                    method: "GET".into(),
                    url: self.url(&format!("/api/search?q={}", escape(&self.query))),
                    body: String::new(),
                }])
            }
            // A genre or mood tile: search for it.
            "find" => {
                if !self.catalog.tags().iter().any(|t| t == arg) {
                    return Err("no such genre".into());
                }
                self.query = arg.to_owned();
                self.focus = Focus::Search;
                self.click(window, "music:search", clock_us)
            }
            // Rhythmbox's browser: narrow the song table to one artist, then one album.
            "filter-artist" => {
                if arg != "all" {
                    self.catalog.artist(arg).ok_or("artist not found")?;
                }
                self.leave_page();
                self.filter_artist = (arg != "all").then(|| arg.to_owned());
                self.filter_album = None;
                Ok(vec![])
            }
            "filter-album" => {
                if arg != "all" {
                    self.catalog.album(arg).ok_or("album not found")?;
                }
                self.leave_page();
                self.filter_album = (arg != "all").then(|| arg.to_owned());
                Ok(vec![])
            }
            "compose" => {
                if self.draft.is_none() {
                    self.draft = Some(String::new());
                }
                self.focus = Focus::NewPlaylist;
                self.menu = None;
                Ok(vec![])
            }
            "cancel" => {
                self.draft = None;
                self.focus = Focus::None;
                Ok(vec![])
            }
            "create" => {
                let title = self.draft.clone().ok_or("no playlist is being named")?;
                if title.trim().is_empty() {
                    return Err("a playlist needs a title".into());
                }
                self.focus = Focus::None;
                self.status = Status::Loading;
                Ok(vec![self.post(
                    window,
                    "/api/playlists",
                    serde_json::json!({ "title": title }),
                )])
            }
            // Transport.
            "play" => {
                // `music:play:<context>` or `music:play:<context>@<track>`.
                let (context, item) = arg.split_once('@').unwrap_or((arg, ""));
                if !self.context_ok(context) {
                    return Err("nothing like that to play".into());
                }
                if !item.is_empty() {
                    self.track(item)?;
                }
                self.menu = None;
                Ok(self.player(
                    window,
                    serde_json::json!({"action": "play", "context": context, "item": item}),
                ))
            }
            "shuffle-play" => {
                if !self.context_ok(arg) {
                    return Err("nothing like that to play".into());
                }
                Ok(self.player(
                    window,
                    serde_json::json!({"action": "play", "context": arg, "shuffle": true}),
                ))
            }
            "toggle" | "shuffle" | "repeat" => {
                self.catalog.player.as_ref().ok_or("nothing is playing")?;
                Ok(self.player(window, serde_json::json!({ "action": verb })))
            }
            "next" => {
                self.live(clock_us).ok_or("nothing is playing")?;
                if !self.can_next(clock_us) {
                    return Err("nothing is queued after this track".into());
                }
                Ok(self.player(window, serde_json::json!({"action": "next"})))
            }
            "previous" => {
                self.live(clock_us).ok_or("nothing is playing")?;
                Ok(self.player(window, serde_json::json!({"action": "previous"})))
            }
            "seek" => {
                let live = self.live(clock_us).ok_or("nothing is playing")?;
                let step: u64 = arg.parse().map_err(|_| "seek needs a step")?;
                if step >= SEEK_STEPS {
                    return Err("that is past the end of the track".into());
                }
                let at = live.duration_ms * step / SEEK_STEPS;
                Ok(self.player(
                    window,
                    serde_json::json!({"action": "seek", "position_ms": at}),
                ))
            }
            "jump" => {
                let index: usize = arg.parse().map_err(|_| "jump needs a queue position")?;
                let p = self.catalog.player.as_ref().ok_or("nothing is playing")?;
                if index >= p.queue.len() {
                    return Err("that queue position does not exist".into());
                }
                Ok(self.player(
                    window,
                    serde_json::json!({"action": "jump", "index": index}),
                ))
            }
            // Songs.
            "like" => {
                self.track(arg)?;
                self.menu = None;
                Ok(vec![self.post(
                    window,
                    &format!("/api/items/{arg}/like"),
                    serde_json::json!({}),
                )])
            }
            "save" => {
                self.track(arg)?;
                self.menu = None;
                Ok(vec![self.post(
                    window,
                    &format!("/api/library/items/{arg}"),
                    serde_json::json!({}),
                )])
            }
            "save-album" => {
                self.catalog.album(arg).ok_or("album not found")?;
                Ok(vec![self.post(
                    window,
                    &format!("/api/library/albums/{arg}"),
                    serde_json::json!({}),
                )])
            }
            "subscribe" => {
                self.catalog.artist(arg).ok_or("artist not found")?;
                Ok(vec![self.post(
                    window,
                    &format!("/api/channels/{arg}/subscribe"),
                    serde_json::json!({}),
                )])
            }
            "play-next" | "play-last" => {
                self.track(arg)?;
                self.menu = None;
                Ok(vec![self.post(
                    window,
                    &format!("/api/items/{arg}/queue"),
                    serde_json::json!({"next": if verb == "play-next" { "true" } else { "false" }}),
                )])
            }
            "menu" => {
                self.track(arg)?;
                self.menu = if self.menu.as_ref().is_some_and(|m| m.track == arg) {
                    None
                } else {
                    Some(Menu {
                        track: arg.to_owned(),
                        playlists: false,
                    })
                };
                Ok(vec![])
            }
            "menu-close" => {
                self.menu = None;
                Ok(vec![])
            }
            "menu-playlists" => {
                let menu = self.menu.as_mut().ok_or("no song menu is open")?;
                menu.playlists = !menu.playlists;
                Ok(vec![])
            }
            "add" => {
                let menu = self.menu.clone().ok_or("no song menu is open")?;
                let list = self.catalog.playlist(arg).ok_or("playlist not found")?;
                if !list.editable {
                    return Err(format!("{} belongs to someone else", list.title));
                }
                if list.items.contains(&menu.track) {
                    return Err(format!("already in {}", list.title));
                }
                self.menu = None;
                Ok(vec![self.post(
                    window,
                    &format!("/api/playlists/{arg}/items"),
                    serde_json::json!({ "item": menu.track }),
                )])
            }
            "remove" => {
                // `music:remove:<playlist>@<track>`.
                let (list, item) = arg
                    .split_once('@')
                    .ok_or("remove needs a playlist and a track")?;
                let playlist = self.catalog.playlist(list).ok_or("playlist not found")?;
                if !playlist.editable {
                    return Err(format!("{} belongs to someone else", playlist.title));
                }
                if !playlist.items.iter().any(|i| i == item) {
                    return Err("that track is not in the playlist".into());
                }
                self.menu = None;
                Ok(vec![self.post(
                    window,
                    &format!("/api/playlists/{list}/remove"),
                    serde_json::json!({ "item": item }),
                )])
            }
            // Volume: the player's own, which the service keeps with the session.
            "volume" => {
                self.catalog.player.as_ref().ok_or("nothing is playing")?;
                let level: u8 = arg
                    .parse()
                    .ok()
                    .filter(|l| *l <= 100)
                    .ok_or("volume is a level from 0 to 100")?;
                Ok(self.player(
                    window,
                    serde_json::json!({"action": "volume", "level": level}),
                ))
            }
            "mute" => {
                self.catalog.player.as_ref().ok_or("nothing is playing")?;
                Ok(self.player(window, serde_json::json!({"action": "mute"})))
            }
            "volume-popover" => {
                self.popup = match self.popup {
                    Some(Popup::Volume) => None,
                    _ => Some(Popup::Volume),
                };
                Ok(vec![])
            }
            // Lyrics follow the song playing; a line seeks to where it is sung.
            "lyrics" => {
                self.lyrics = !self.lyrics;
                self.popup = None;
                Ok(vec![])
            }
            "lyric" => {
                let live = self.live(clock_us).ok_or("nothing is playing")?;
                let track = self.track(&live.id)?;
                let index: usize = arg.parse().map_err(|_| "lyric needs a line")?;
                let (at, _) = track.lyrics.get(index).ok_or("the song has no such line")?;
                Ok(self.player(
                    window,
                    serde_json::json!({"action": "seek", "position_ms": at}),
                ))
            }
            // The output picker: AirPlay, Cast, Cast to device. Opening it asks each of
            // the account's speakers whether it is there.
            "output" if arg.is_empty() && !command.ends_with(':') => {
                if self.popup == Some(Popup::Output) {
                    self.popup = None;
                    return Ok(vec![]);
                }
                self.popup = Some(Popup::Output);
                self.menu = None;
                self.reachable.clear();
                Ok(self
                    .catalog
                    .devices
                    .iter()
                    .map(|d| AppEffect::Http {
                        window,
                        tag: format!("probe:{}", d.id),
                        method: "GET".into(),
                        url: format!("{}/api/status", d.url.trim_end_matches('/')),
                        body: String::new(),
                    })
                    .collect())
            }
            "output" => {
                self.popup = None;
                let current = self
                    .catalog
                    .player
                    .as_ref()
                    .map(|p| p.device.clone())
                    .unwrap_or_default();
                if arg == current {
                    return Ok(vec![]);
                }
                if arg.is_empty() {
                    // Back to this device: the speaker lets the session go.
                    let mut effects = vec![];
                    if let Some(device) = self.catalog.device(&current) {
                        effects.push(self.device_post(
                            window,
                            format!("stopped:{}", device.id),
                            device,
                            "/api/stop",
                            serde_json::json!({}),
                        ));
                    }
                    effects.extend(self.player(
                        window,
                        serde_json::json!({"action": "output", "device": ""}),
                    ));
                    return Ok(effects);
                }
                let device = self.catalog.device(arg).ok_or("no such device")?;
                if self.reachable.get(arg) != Some(&true) {
                    return Err(format!("{} is not available", device.name));
                }
                match self.catalog.player.as_ref().filter(|p| !p.queue.is_empty()) {
                    // Hand the session over first; the service follows once it is there.
                    Some(p) => Ok(vec![self.device_post(
                        window,
                        format!("cast:{}", device.id),
                        device,
                        "/api/cast",
                        self.session(p),
                    )]),
                    None => Err("play something first".into()),
                }
            }
            "popup-close" => {
                self.popup = None;
                Ok(vec![])
            }
            // The lyrics sheet's own surface: a touch on it stays on it.
            "lyrics-sheet" => Ok(vec![]),
            _ => Err(format!("unknown music command {command}")),
        }
    }
    /// Semantic projection, so an agent can drive the player without pixels.
    pub fn page(&self, page: &mut cw_protocol::Page) {
        use cw_protocol::PageElement as E;
        let act = |url: String| cw_protocol::PageAction {
            method: "APP".into(),
            url,
            fields: Default::default(),
        };
        let button = |id: String, text: String| E::Button {
            id: id.clone(),
            text,
            action: act(id),
            style: None,
        };
        if let Some(text) = self.status.notice() {
            page.elements.push(E::Text {
                id: "music-status".into(),
                text: text.into(),
            });
        }
        if let (Some(p), Some(track)) = (&self.catalog.player, self.now()) {
            page.elements.push(E::Text {
                id: "music-now".into(),
                text: format!(
                    "{} {} — {} ({}; shuffle {}, repeat {:?})",
                    if p.playing { "Playing" } else { "Paused" },
                    track.title,
                    self.catalog.artist_name(&track.artist),
                    p.context_title,
                    if p.shuffle { "on" } else { "off" },
                    p.repeat
                ),
            });
            for (id, label) in [
                ("toggle", if p.playing { "Pause" } else { "Play" }),
                ("previous", "Previous"),
                ("next", "Next"),
                ("shuffle", "Shuffle"),
                ("repeat", "Repeat"),
                ("queue", "Playing Next"),
                ("mute", if p.muted { "Unmute" } else { "Mute" }),
                ("output", "Output"),
                ("lyrics", "Lyrics"),
            ] {
                page.elements
                    .push(button(format!("music:{id}"), label.into()));
            }
            page.elements.push(E::Text {
                id: "music-volume".into(),
                text: format!(
                    "Volume {}%{}; {}",
                    p.volume,
                    if p.muted { " (muted)" } else { "" },
                    if p.device.is_empty() {
                        "playing here".to_owned()
                    } else {
                        format!("playing on {}", p.device_name)
                    }
                ),
            });
            if !track.lyrics.is_empty() {
                page.elements.push(E::Text {
                    id: "music-lyrics".into(),
                    text: track
                        .lyrics
                        .iter()
                        .map(|(at, line)| format!("[{}] {line}", clock(*at)))
                        .collect::<Vec<_>>()
                        .join("\n"),
                });
            }
        }
        if self.popup == Some(Popup::Output) {
            for d in &self.catalog.devices {
                let state = match self.reachable.get(&d.id) {
                    Some(true) => "available",
                    Some(false) => "not available",
                    None => "looking",
                };
                page.elements.push(button(
                    format!("music:output:{}", d.id),
                    format!("{} ({state})", d.name),
                ));
            }
        }
        page.elements.push(E::Input {
            id: "music-query".into(),
            label: "Search".into(),
            value: self.query.clone(),
            placeholder: "Artists, Songs, Albums".into(),
        });
        for (id, label) in [
            ("home", "Home"),
            ("new", "New"),
            ("radio", "Radio"),
            ("library", "Library"),
            ("liked", "Favorite Songs"),
            ("compose", "New Playlist"),
        ] {
            page.elements
                .push(button(format!("music:{id}"), label.into()));
        }
        for playlist in &self.catalog.playlists {
            page.elements.push(button(
                format!("music:playlist:{}", playlist.id),
                playlist.title.clone(),
            ));
        }
        for album in &self.catalog.albums {
            page.elements.push(button(
                format!("music:album:{}", album.id),
                format!(
                    "{} — {}",
                    album.title,
                    self.catalog.artist_name(&album.artist)
                ),
            ));
        }
        for track in &self.catalog.tracks {
            page.elements.push(button(
                format!("music:play:track@{}", track.id),
                format!("Play {}", track.title),
            ));
        }
    }
    /// The scroll pane the current view is painted in. Each view gets its own, so a
    /// playlist opens at its top however far down the last one was scrolled.
    pub fn view_pane(&self) -> String {
        let slug: String = format!("{:?}", self.view)
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
            .take(48)
            .collect();
        format!("view-{slug}")
    }
    pub fn render(&self, p: &mut Painter, env: &crate::AppEnv<'_>) {
        match env.theme {
            DesktopTheme::Macos => apple::mac(self, p, env),
            DesktopTheme::Ios => apple::ios(self, p, env),
            DesktopTheme::Android => ytm::render(self, p, env),
            DesktopTheme::Windows => wmp::render(self, p, env),
            DesktopTheme::Ubuntu => rhythmbox::render(self, p, env),
        }
    }
    /// The songs a view lists, and the context playing one of them plays within.
    pub fn listing(&self, view: &View) -> (Vec<&Track>, String) {
        let c = &self.catalog;
        match view {
            View::Album(id) => (
                c.album(id)
                    .map(|a| a.tracks.iter().filter_map(|t| c.track(t)).collect())
                    .unwrap_or_default(),
                format!("album:{id}"),
            ),
            View::Playlist(id) => (
                c.playlist(id)
                    .map(|p| p.items.iter().filter_map(|t| c.track(t)).collect())
                    .unwrap_or_default(),
                format!("playlist:{id}"),
            ),
            View::Artist(id) => (c.top_songs(id), format!("artist:{id}")),
            View::Liked => (c.liked_songs(), "liked".into()),
            View::Library(Shelf::Songs) => (c.library_songs(), "library".into()),
            _ => (vec![], String::new()),
        }
    }
    /// The open song menu's rows, worded the way `theme`'s player words them.
    pub fn menu_items(&self, theme: DesktopTheme) -> Vec<MenuItem> {
        let Some(menu) = &self.menu else {
            return vec![];
        };
        let Some(track) = self.catalog.track(&menu.track) else {
            return vec![];
        };
        let t = &track.id;
        let item = |label: &str, target: String| MenuItem {
            label: label.to_owned(),
            target: Some(target),
            why: "",
            destructive: false,
        };
        let yt = theme == DesktopTheme::Android;
        if menu.playlists {
            let mut items = vec![
                item("‹ Back", "music:menu-playlists".into()),
                item("New Playlist…", "music:compose".into()),
            ];
            for list in &self.catalog.playlists {
                if !list.editable {
                    items.push(MenuItem {
                        label: list.title.clone(),
                        target: None,
                        why: "This playlist belongs to someone else",
                        destructive: false,
                    });
                } else if list.items.contains(t) {
                    items.push(MenuItem {
                        label: format!("✓ {}", list.title),
                        target: None,
                        why: "Already in this playlist",
                        destructive: false,
                    });
                } else {
                    items.push(item(&list.title, format!("music:add:{}", list.id)));
                }
            }
            return items;
        }
        let saved = self.catalog.library.contains(t);
        let liked = self.catalog.liked.contains(t);
        let mut items = vec![];
        if yt {
            items.push(item(
                "Start radio",
                format!("music:play:station:{}", track.artist),
            ));
        }
        items.push(item("Play Next", format!("music:play-next:{t}")));
        items.push(item(
            match theme {
                DesktopTheme::Android => "Add to queue",
                DesktopTheme::Windows => "Add to play queue",
                DesktopTheme::Ubuntu => "Add to Play Queue",
                _ => "Play Last",
            },
            format!("music:play-last:{t}"),
        ));
        items.push(item(
            match (yt, saved) {
                (true, true) => "Remove from library",
                (true, false) => "Save to library",
                (false, true) => "Delete from Library",
                (false, false) => "Add to Library",
            },
            format!("music:save:{t}"),
        ));
        items.push(item(
            match (theme, liked) {
                (DesktopTheme::Android, true) => "Remove from liked songs",
                (DesktopTheme::Android, false) => "Like",
                (DesktopTheme::Macos | DesktopTheme::Ios, true) => "Undo Favorite",
                (DesktopTheme::Macos | DesktopTheme::Ios, false) => "Favorite",
                (_, true) => "Unlike",
                (_, false) => "Like",
            },
            format!("music:like:{t}"),
        ));
        items.push(item(
            if yt {
                "Save to playlist"
            } else {
                "Add to Playlist…"
            },
            "music:menu-playlists".into(),
        ));
        if !yt {
            items.push(item(
                "Create Station",
                format!("music:play:station:{}", track.artist),
            ));
        }
        if self.catalog.album(&track.album).is_some() {
            items.push(item(
                if yt { "Go to album" } else { "Go to Album" },
                format!("music:album:{}", track.album),
            ));
        }
        items.push(item(
            if yt { "Go to artist" } else { "Go to Artist" },
            format!("music:artist:{}", track.artist),
        ));
        if let View::Playlist(list) = &self.view {
            if self
                .catalog
                .playlist(list)
                .is_some_and(|p| p.editable && p.items.contains(t))
            {
                items.push(MenuItem {
                    destructive: true,
                    ..item(
                        if yt {
                            "Remove from playlist"
                        } else {
                            "Remove from Playlist"
                        },
                        format!("music:remove:{list}@{t}"),
                    )
                });
            }
        }
        items
    }
}

/// One row of the song menu. `target` is `None` for a row shown disabled, with `why`.
pub struct MenuItem {
    pub label: String,
    pub target: Option<String>,
    pub why: &'static str,
    pub destructive: bool,
}

/// Percent-encode a query for a URL.
fn escape(query: &str) -> String {
    let mut out = String::new();
    for byte in query.bytes() {
        match byte {
            b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char)
            }
            b' ' => out.push_str("%20"),
            other => out.push_str(&format!("%{other:02X}")),
        }
    }
    out
}
/// "3:41"; "1:02:03" past an hour.
pub fn clock(ms: u64) -> String {
    let s = ms / 1_000;
    match (s / 3600, (s / 60) % 60, s % 60) {
        (0, m, s) => format!("{m}:{s:02}"),
        (h, m, s) => format!("{h}:{m:02}:{s:02}"),
    }
}
/// "11 songs, 42 minutes", as the players summarise a list.
pub fn summary(tracks: &[&Track]) -> String {
    let minutes = tracks.iter().map(|t| t.duration_ms).sum::<u64>() / 60_000;
    format!(
        "{} {}, {} {}",
        tracks.len(),
        if tracks.len() == 1 { "song" } else { "songs" },
        minutes,
        if minutes == 1 { "minute" } else { "minutes" }
    )
}

#[cfg(test)]
mod tests;
