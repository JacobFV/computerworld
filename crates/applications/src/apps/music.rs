//! Music over the `media` service in audio mode — the same catalogue spotify.com serves.
//!
//! The service answers reads with `cw_protocol::Page` documents and mutations with JSON, so
//! this application reads the pages it is sent and never keeps a catalogue of its own. Every
//! artist, track, playlist and now-playing line on screen is a string the service put in a
//! reply; a reply that carries nothing renders an empty library.
use super::look::{action, header, inert, look, notice, FAINT, INK, LINE, MUTED};
use super::{push_bounded, Status};
use crate::desktop_scene::{shared::Align, DesktopTheme, Painter};
use crate::AppEffect;
use cw_scene::{Color, Rect};
use serde::{Deserialize, Serialize};

/// Rows retained from any one reply. A catalogue may be any size; the window is not.
pub const LIST_LIMIT: usize = 200;
const QUERY_LIMIT: usize = 120;
const TITLE_LIMIT: usize = 80;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum View {
    #[default]
    Browse,
    Artist(String),
    Playlist(String),
    Results,
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

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Track {
    pub id: String,
    pub title: String,
    /// "artist · album" or "artist · N views", exactly as the service wrote it.
    pub subtitle: String,
    pub duration: String,
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Artist {
    pub id: String,
    pub name: String,
    pub meta: String,
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Playlist {
    pub id: String,
    pub title: String,
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct NowPlaying {
    pub id: String,
    pub title: String,
    pub meta: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Music {
    pub base: String,
    pub view: View,
    /// Title of the reply that filled the screen, as the service titled it.
    pub heading: String,
    pub query: String,
    pub focus: Focus,
    pub artists: Vec<Artist>,
    pub tracks: Vec<Track>,
    pub playlists: Vec<Playlist>,
    /// Boxed: `AppState` keeps every application side by side, so the largest one
    /// sets the size of them all.
    pub now: Option<Box<NowPlaying>>,
    /// Track ids the service shows as liked. Refreshed whole from a browse reply, which is
    /// the one page whose like control carries its engaged state, and adjusted by this
    /// application's own like replies, which say plainly which way the toggle went.
    pub liked: Vec<String>,
    pub selected: Option<String>,
    /// Title being typed for a new playlist; `None` when the composer is closed.
    pub draft: Option<String>,
    pub status: Status,
}
impl Music {
    pub const KIND: &'static str = "music";
    pub fn launch(argument: &str, window: u64, _clock_us: u64) -> (Self, Vec<AppEffect>) {
        let app = Self {
            base: if argument.is_empty() {
                "http://music.internal/".into()
            } else {
                argument.to_owned()
            },
            view: View::Browse,
            heading: String::new(),
            query: String::new(),
            focus: Focus::None,
            artists: vec![],
            tracks: vec![],
            playlists: vec![],
            now: None,
            liked: vec![],
            selected: None,
            draft: None,
            status: Status::Loading,
        };
        let effects = app.fetch(window);
        (app, effects)
    }
    pub fn kind(&self) -> &'static str {
        Self::KIND
    }
    pub fn title(&self, theme: DesktopTheme) -> String {
        match theme {
            DesktopTheme::Windows => "Media Player",
            DesktopTheme::Ubuntu => "Rhythmbox",
            DesktopTheme::Android => "Music",
            _ => "Music",
        }
        .into()
    }
    pub fn document(&self) -> String {
        String::new()
    }
    pub fn caption(&self) -> String {
        match &self.now {
            Some(now) => format!("{} — {}", now.title, now.meta),
            None => self.heading.clone(),
        }
    }
    pub fn modified(&self) -> bool {
        self.draft.is_some()
    }
    fn url(&self, suffix: &str) -> String {
        format!("{}{suffix}", self.base.trim_end_matches('/'))
    }
    fn get(&self, window: u64, tag: &str, suffix: &str) -> AppEffect {
        AppEffect::Http {
            window,
            tag: tag.into(),
            method: "GET".into(),
            url: self.url(suffix),
            body: String::new(),
        }
    }
    /// Re-read whatever the window is showing. Every mutation ends here, so the screen
    /// only ever shows what the service committed.
    fn fetch(&self, window: u64) -> Vec<AppEffect> {
        vec![match &self.view {
            View::Browse => self.get(window, "browse", "/"),
            View::Artist(id) => self.get(window, "artist", &format!("/artist/{id}")),
            View::Playlist(id) => self.get(window, "playlist", &format!("/playlist/{id}")),
            View::Results => self.get(
                window,
                "results",
                &format!("/search?q={}", escape(&self.query)),
            ),
        }]
    }
    pub fn offline(&mut self, _tag: &str, reason: &str) {
        self.status = Status::Offline(reason.to_owned());
    }
    pub fn http(
        &mut self,
        window: u64,
        tag: &str,
        status: u16,
        body: &str,
    ) -> Result<Vec<AppEffect>, String> {
        self.status = Status::from_status(status, body);
        if self.status != Status::Idle {
            return Ok(vec![]);
        }
        match tag {
            "browse" | "artist" | "playlist" | "results" => {
                let page: cw_protocol::Page =
                    serde_json::from_str(body).unwrap_or(cw_protocol::Page {
                        version: 1,
                        title: String::new(),
                        elements: vec![],
                        theme: None,
                    });
                self.absorb(tag, &page);
                Ok(vec![])
            }
            // A mutation's reply says what changed; the screen is then re-read rather than
            // patched, so a local guess can never drift from the service.
            "like" => {
                let id = self.selected.clone().unwrap_or_default();
                let liked = serde_json::from_str::<serde_json::Value>(body)
                    .ok()
                    .and_then(|v| v.get("liked")?.as_bool());
                match liked {
                    Some(true) if !id.is_empty() && !self.liked.contains(&id) => {
                        self.liked.push(id);
                        self.liked.truncate(LIST_LIMIT);
                    }
                    Some(false) => self.liked.retain(|x| *x != id),
                    _ => {}
                }
                self.status = Status::Loading;
                Ok(self.fetch(window))
            }
            "play" | "create" | "add" => {
                self.draft = None;
                self.status = Status::Loading;
                Ok(self.fetch(window))
            }
            other => Err(format!("unexpected music reply {other}")),
        }
    }
    /// Take every row on screen from one service reply. Ids are the service's own, so a
    /// control built here always names something the service will recognise.
    fn absorb(&mut self, tag: &str, page: &cw_protocol::Page) {
        let mut flat = Flat::default();
        flatten(&page.elements, &mut flat);
        let accent = page
            .theme
            .as_ref()
            .and_then(|t| t.accent.clone())
            .unwrap_or_else(|| "#ff0000".into());
        self.heading = if page.title.is_empty() {
            "Music".to_owned()
        } else {
            page.title.clone()
        };
        let (prefix, sub) = match tag {
            "browse" => ("line-", "-artist"),
            "artist" => ("channel-", "-channel"),
            "playlist" => ("playlist-", "-channel"),
            _ => ("result-", "-channel"),
        };
        self.tracks = flat
            .rows(prefix, "-title")
            .into_iter()
            .take(LIST_LIMIT)
            .map(|(id, title)| Track {
                subtitle: flat.text(&format!("{prefix}{id}{sub}")).unwrap_or_default(),
                duration: flat
                    .text(&format!("{prefix}{id}-duration"))
                    .unwrap_or_default(),
                id,
                title,
            })
            .collect();
        self.artists = match tag {
            "browse" => flat.rows("artist-", "-name"),
            "results" => flat.rows("result-channel-", "-name"),
            _ => vec![],
        }
        .into_iter()
        .take(LIST_LIMIT)
        .map(|(id, name)| Artist {
            meta: flat.text(&format!("artist-{id}-meta")).unwrap_or_default(),
            id,
            name,
        })
        .collect();
        // The sidebar is whatever the reply links to, which is the library the service has.
        // Only the browse reply lists them, so a page that mentions none leaves them alone
        // rather than reporting an empty library the service never claimed.
        let found: Vec<Playlist> = flat
            .links
            .iter()
            .filter_map(|(id, url)| {
                let list = url.strip_prefix("/playlist/")?;
                Some(Playlist {
                    id: list.to_owned(),
                    title: flat.text(id).unwrap_or_else(|| list.to_owned()),
                })
            })
            .take(LIST_LIMIT)
            .collect();
        if tag == "browse" || !found.is_empty() {
            self.playlists = found;
        }
        // The now-playing bar rides on some pages and not others; one that has none says
        // nothing about what is parked, so the last thing the service said stands.
        if flat.text("bar").is_some() {
            self.now = None;
        } else if let Some(title) = flat.text("bar-title") {
            self.now = flat
                .links
                .iter()
                .find(|(id, _)| id == "bar-title")
                .and_then(|(_, url)| url.strip_prefix("/track/"))
                .map(|id| {
                    Box::new(NowPlaying {
                        id: id.to_owned(),
                        title,
                        meta: flat.text("bar-meta").unwrap_or_default(),
                    })
                });
        }
        if tag == "browse" {
            // Only the browse page paints a like control with its engaged state, and it
            // carries it as the accent fill the page theme names.
            self.liked = flat
                .cards
                .iter()
                .filter(|(_, fill)| fill.as_deref() == Some(accent.as_str()))
                .filter_map(|(id, _)| {
                    id.strip_prefix("line-")?
                        .strip_suffix("-like")
                        .map(str::to_owned)
                })
                .take(LIST_LIMIT)
                .collect();
        }
        if self
            .selected
            .as_ref()
            .is_some_and(|id| !self.tracks.iter().any(|t| t.id == *id))
        {
            self.selected = None;
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
        match key {
            "Backspace" => {
                match self.focus {
                    Focus::Search => {
                        self.query.pop();
                    }
                    Focus::NewPlaylist => {
                        self.draft
                            .as_mut()
                            .ok_or("no playlist is being named")?
                            .pop();
                    }
                    Focus::None => return Err("no music field is focused".into()),
                }
                Ok(vec![])
            }
            "Enter" => match self.focus {
                Focus::Search => self.click(window, "music:search", clock_us),
                Focus::NewPlaylist => self.click(window, "music:create", clock_us),
                Focus::None => Err("no music field is focused".into()),
            },
            "Escape" => {
                self.focus = Focus::None;
                self.draft = None;
                Ok(vec![])
            }
            other => Err(format!("unsupported music key {other}")),
        }
    }
    fn post(&self, window: u64, tag: &str, suffix: &str, body: serde_json::Value) -> AppEffect {
        AppEffect::Http {
            window,
            tag: tag.into(),
            method: "POST".into(),
            url: self.url(suffix),
            body: body.to_string(),
        }
    }
    pub fn click(
        &mut self,
        window: u64,
        target: &str,
        _clock_us: u64,
    ) -> Result<Vec<AppEffect>, String> {
        let command = target
            .strip_prefix("music:")
            .ok_or("interaction does not belong to music")?;
        match command {
            "reload" => {
                self.status = Status::Loading;
                Ok(self.fetch(window))
            }
            "home" => {
                self.view = View::Browse;
                self.selected = None;
                self.status = Status::Loading;
                Ok(self.fetch(window))
            }
            "search-field" => {
                self.focus = Focus::Search;
                Ok(vec![])
            }
            "clear" => {
                self.query.clear();
                Ok(vec![])
            }
            "search" => {
                if self.query.trim().is_empty() {
                    return Err("a search needs something to look for".into());
                }
                self.view = View::Results;
                self.focus = Focus::None;
                self.status = Status::Loading;
                Ok(self.fetch(window))
            }
            "new" => {
                self.draft = Some(String::new());
                self.focus = Focus::NewPlaylist;
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
                self.status = Status::Loading;
                Ok(vec![self.post(
                    window,
                    "create",
                    "/api/playlists",
                    serde_json::json!({ "title": title }),
                )])
            }
            rest => {
                if let Some(id) = rest.strip_prefix("artist:") {
                    if !self.artists.iter().any(|a| a.id == id) {
                        return Err("artist not found".into());
                    }
                    self.view = View::Artist(id.to_owned());
                    self.selected = None;
                    self.status = Status::Loading;
                    return Ok(self.fetch(window));
                }
                if let Some(id) = rest.strip_prefix("playlist:") {
                    if !self.playlists.iter().any(|p| p.id == id) {
                        return Err("playlist not found".into());
                    }
                    self.view = View::Playlist(id.to_owned());
                    self.selected = None;
                    self.status = Status::Loading;
                    return Ok(self.fetch(window));
                }
                if let Some(id) = rest.strip_prefix("track:") {
                    self.track(id)?;
                    self.selected = Some(id.to_owned());
                    return Ok(vec![]);
                }
                if let Some(id) = rest.strip_prefix("play:") {
                    self.track(id)?;
                    self.selected = Some(id.to_owned());
                    let list = match &self.view {
                        View::Playlist(list) => list.clone(),
                        _ => String::new(),
                    };
                    self.status = Status::Loading;
                    return Ok(vec![self.post(
                        window,
                        "play",
                        &format!("/api/items/{id}/play"),
                        serde_json::json!({ "list": list }),
                    )]);
                }
                if let Some(id) = rest.strip_prefix("like:") {
                    self.track(id)?;
                    self.selected = Some(id.to_owned());
                    self.status = Status::Loading;
                    return Ok(vec![self.post(
                        window,
                        "like",
                        &format!("/api/items/{id}/like"),
                        serde_json::json!({}),
                    )]);
                }
                if let Some(list) = rest.strip_prefix("add:") {
                    let track = self.selected.clone().ok_or("no track is selected")?;
                    if !self.playlists.iter().any(|p| p.id == list) {
                        return Err("playlist not found".into());
                    }
                    self.status = Status::Loading;
                    return Ok(vec![self.post(
                        window,
                        "add",
                        &format!("/api/playlists/{list}/items"),
                        serde_json::json!({ "item": track }),
                    )]);
                }
                Err(format!("unknown music command {command}"))
            }
        }
    }
    fn track(&self, id: &str) -> Result<&Track, String> {
        self.tracks
            .iter()
            .find(|t| t.id == id)
            .ok_or_else(|| "track not found".into())
    }
    pub fn page(&self, page: &mut cw_protocol::Page) {
        use cw_protocol::PageElement as E;
        let act = |url: &str| cw_protocol::PageAction {
            method: "APP".into(),
            url: url.into(),
            fields: Default::default(),
        };
        page.elements.push(E::Heading {
            id: "music-heading".into(),
            text: self.heading.clone(),
            level: 2,
        });
        if let Some(text) = self.status.notice() {
            page.elements.push(E::Text {
                id: "music-status".into(),
                text: text.into(),
            });
        }
        if let Some(now) = &self.now {
            page.elements.push(E::Text {
                id: "music-now".into(),
                text: format!("Now playing: {} — {}", now.title, now.meta),
            });
        }
        page.elements.push(E::Input {
            id: "music-query".into(),
            label: "Search".into(),
            value: self.query.clone(),
            placeholder: "Songs and artists".into(),
        });
        for (id, label) in [
            ("music:search", "Search"),
            ("music:home", "Browse"),
            ("music:reload", "Reload"),
            ("music:new", "New playlist"),
        ] {
            page.elements.push(E::Button {
                id: id.into(),
                text: label.into(),
                action: act(id),
            });
        }
        for playlist in &self.playlists {
            page.elements.push(E::Button {
                id: format!("music:playlist:{}", playlist.id),
                text: playlist.title.clone(),
                action: act(&format!("music:playlist:{}", playlist.id)),
            });
            if self.selected.is_some() {
                page.elements.push(E::Button {
                    id: format!("music:add:{}", playlist.id),
                    text: format!("Add to {}", playlist.title),
                    action: act(&format!("music:add:{}", playlist.id)),
                });
            }
        }
        for artist in &self.artists {
            page.elements.push(E::Button {
                id: format!("music:artist:{}", artist.id),
                text: format!("{} — {}", artist.name, artist.meta),
                action: act(&format!("music:artist:{}", artist.id)),
            });
        }
        for track in &self.tracks {
            page.elements.push(E::Button {
                id: format!("music:track:{}", track.id),
                text: format!("{} — {}", track.title, track.subtitle),
                action: act(&format!("music:track:{}", track.id)),
            });
            page.elements.push(E::Button {
                id: format!("music:play:{}", track.id),
                text: format!("Play {}", track.title),
                action: act(&format!("music:play:{}", track.id)),
            });
            page.elements.push(E::Button {
                id: format!("music:like:{}", track.id),
                text: format!(
                    "{} {}",
                    if self.liked.contains(&track.id) {
                        "Unlike"
                    } else {
                        "Like"
                    },
                    track.title
                ),
                action: act(&format!("music:like:{}", track.id)),
            });
        }
        if let Some(draft) = &self.draft {
            page.elements.push(E::Input {
                id: "music-playlist-title".into(),
                label: "Playlist title".into(),
                value: draft.clone(),
                placeholder: "New playlist".into(),
            });
            for (id, label) in [("music:create", "Create"), ("music:cancel", "Cancel")] {
                page.elements.push(E::Button {
                    id: id.into(),
                    text: label.into(),
                    action: act(id),
                });
            }
        }
    }
    pub fn render(&self, p: &mut Painter, env: &crate::AppEnv<'_>) {
        let (theme, width, height) = (env.theme, env.width, env.height);
        let l = look(theme);
        p.scene.background = l.surface;
        let mut top = header(p, theme, &l, width, &self.title(theme));
        // The now-playing bar is the bottom of the window on every platform, because the
        // thing that is playing outlives whichever page you wandered onto.
        let bar_h = if theme.mobile() { 64 } else { 52 };
        let bottom = height.saturating_sub(bar_h);
        // Search: a desktop keeps it in a toolbar, a phone gives it its own row.
        let search_h = if theme.mobile() { 48 } else { 38 };
        p.box_(Rect::new(0, top, width, search_h), l.chrome, 0);
        p.hline(0, top + search_h as i32, width, LINE);
        let field = Rect::new(10, top + 6, width.saturating_sub(196), search_h - 12);
        p.border(field, l.surface, l.radius, LINE);
        p.region(field, "music:search-field", "Search songs and artists");
        p.symbol(
            "search",
            field.x + 8,
            field.y + (field.height as i32 - 14) / 2,
            14,
            FAINT,
        );
        p.left(
            field.x + 28,
            field.y + (field.height as i32 - 16) / 2,
            field.width.saturating_sub(40),
            if self.query.is_empty() {
                "Search songs and artists"
            } else {
                &self.query
            },
            13,
            if self.query.is_empty() { FAINT } else { INK },
        );
        let mut x = width as i32 - 184;
        for (target, label, primary) in [
            ("music:search", "Search", true),
            ("music:home", "Browse", false),
            ("music:reload", "Reload", false),
        ] {
            let w = p.measure(label, 12, false) + 20;
            let r = Rect::new(x, top + 6, w, search_h - 12);
            if target == "music:search" && self.query.trim().is_empty() {
                // Nothing typed: the service would be asked for nothing, so the control
                // is announced disabled instead of dispatching an empty query.
                inert(p, &l, r, label, "Search needs something to look for");
            } else {
                action(p, &l, r, label, target, primary);
            }
            x += w as i32 + 6;
        }
        top += search_h as i32 + 1;
        if let Some(text) = self.status.notice() {
            notice(p, width, top + 8, text);
            top += 26;
        }
        let side = if theme.mobile() || width < 560 {
            0
        } else {
            196
        };
        if side > 0 {
            self.sidebar(p, &l, top, side as u32, bottom);
        }
        let body_x = side;
        let body_w = width.saturating_sub(side as u32);
        let mut y = top;
        if !self.artists.is_empty() {
            y = self.artist_strip(p, theme, &l, body_x, y, body_w, bottom);
        }
        if self.tracks.is_empty() && self.artists.is_empty() {
            notice(p, body_w, y + 20, "Nothing here");
        }
        for track in &self.tracks {
            if y as u32 + l.row + 4 > bottom {
                break;
            }
            self.track_row(p, theme, &l, body_x, y, body_w, track);
            y += l.row as i32 + 4;
        }
        self.now_bar(p, theme, &l, width, bottom, bar_h);
        if let Some(draft) = &self.draft {
            self.composer(p, &l, width, height, draft);
        }
    }
    fn sidebar(&self, p: &mut Painter, l: &super::look::Look, top: i32, side: u32, bottom: u32) {
        p.box_(
            Rect::new(0, top, side, bottom.saturating_sub(top.max(0) as u32)),
            l.chrome,
            0,
        );
        p.vline(side as i32, top, bottom, LINE);
        p.strong(12, top + 10, side - 24, "Your library", 12, MUTED);
        let mut y = top + 30;
        for playlist in &self.playlists {
            if y as u32 + 30 > bottom {
                break;
            }
            let on = matches!(&self.view, View::Playlist(id) if *id == playlist.id);
            let add = if self.selected.is_some() { 44 } else { 0 };
            let r = Rect::new(6, y, side - 12 - add, 28);
            p.button(
                r,
                if on { l.selection } else { Color::TRANSPARENT },
                l.radius,
                &format!("music:playlist:{}", playlist.id),
                &playlist.title,
            );
            p.left(
                r.x + 8,
                r.y + 6,
                r.width.saturating_sub(14),
                &playlist.title,
                12,
                if on { l.accent } else { INK },
            );
            if add > 0 {
                // Only offered while a track is selected: with nothing chosen there is
                // nothing this control could add.
                action(
                    p,
                    l,
                    Rect::new(side as i32 - 46, y, 40, 28),
                    "Add",
                    &format!("music:add:{}", playlist.id),
                    false,
                );
            }
            y += 30;
        }
        if self.playlists.is_empty() {
            p.left(12, y + 4, side - 24, "No playlists", 12, FAINT);
            y += 24;
        }
        action(
            p,
            l,
            Rect::new(6, y + 6, side - 12, 28),
            "New playlist",
            "music:new",
            true,
        );
    }
    #[allow(clippy::too_many_arguments)]
    fn artist_strip(
        &self,
        p: &mut Painter,
        theme: DesktopTheme,
        l: &super::look::Look,
        x0: i32,
        top: i32,
        width: u32,
        bottom: u32,
    ) -> i32 {
        let tile = if theme.mobile() { 96 } else { 84 };
        if top as u32 + tile as u32 + 34 > bottom {
            return top;
        }
        p.strong(
            x0 + 12,
            top + 6,
            width.saturating_sub(24),
            "Artists",
            12,
            MUTED,
        );
        let mut x = x0 + 12;
        for artist in &self.artists {
            if x + tile > x0 + width as i32 - 8 {
                break;
            }
            let r = Rect::new(x, top + 24, tile as u32, tile as u32);
            p.button(
                r,
                Color(0, 0, 0, 12),
                // Round on the phones and on macOS, square-ish where the platform is.
                if theme == DesktopTheme::Windows {
                    l.radius
                } else {
                    tile as u32 / 2
                },
                &format!("music:artist:{}", artist.id),
                &artist.name,
            );
            p.symbol(
                "music",
                r.x + (tile - 24) / 2,
                r.y + (tile - 24) / 2,
                24,
                l.accent,
            );
            p.label(
                r.x - 6,
                r.y + tile + 3,
                tile as u32 + 12,
                &artist.name,
                11,
                INK,
                false,
                Align::Center,
            );
            x += tile + 14;
        }
        top + tile + 40
    }
    #[allow(clippy::too_many_arguments)]
    fn track_row(
        &self,
        p: &mut Painter,
        theme: DesktopTheme,
        l: &super::look::Look,
        x0: i32,
        y: i32,
        width: u32,
        track: &Track,
    ) {
        let on = self.selected.as_deref() == Some(track.id.as_str());
        let r = Rect::new(x0 + 8, y, width.saturating_sub(16), l.row);
        p.button(
            r,
            if on { l.selection } else { Color::TRANSPARENT },
            l.radius,
            &format!("music:track:{}", track.id),
            &track.title,
        );
        if !theme.mobile() {
            p.hline(r.x, r.y + r.height as i32, r.width, LINE);
        }
        let liked = self.liked.contains(&track.id);
        let controls = 150;
        let text = r.width.saturating_sub(controls + 70);
        if l.row > 34 {
            // A tall row stacks the title over the artist, the way a phone list does.
            p.left(r.x + 10, r.y + 6, text, &track.title, 14, INK);
            p.left(r.x + 10, r.y + 24, text, &track.subtitle, 11, MUTED);
        } else {
            let half = text / 2;
            p.left(
                r.x + 10,
                r.y + (l.row as i32 - 16) / 2,
                half,
                &track.title,
                13,
                INK,
            );
            p.left(
                r.x + 14 + half as i32,
                r.y + (l.row as i32 - 15) / 2,
                text - half,
                &track.subtitle,
                11,
                MUTED,
            );
        }
        if !track.duration.is_empty() {
            p.right(
                r.x + r.width as i32 - controls as i32 - 12,
                r.y + (l.row as i32 - 15) / 2,
                48,
                &track.duration,
                11,
                FAINT,
            );
        }
        let h = (l.row - 8).clamp(20, 32);
        let cy = r.y + (l.row as i32 - h as i32) / 2;
        action(
            p,
            l,
            Rect::new(r.x + r.width as i32 - 138, cy, 60, h),
            "Play",
            &format!("music:play:{}", track.id),
            true,
        );
        action(
            p,
            l,
            Rect::new(r.x + r.width as i32 - 72, cy, 64, h),
            if liked { "Liked" } else { "Like" },
            &format!("music:like:{}", track.id),
            liked,
        );
    }
    fn now_bar(
        &self,
        p: &mut Painter,
        theme: DesktopTheme,
        l: &super::look::Look,
        width: u32,
        top: u32,
        height: u32,
    ) {
        let r = Rect::new(0, top as i32, width, height);
        if theme.mobile() {
            p.glass(r, 0, 18, l.chrome, Some(LINE));
        } else {
            p.box_(r, l.chrome, 0);
        }
        p.hline(0, top as i32, width, LINE);
        match &self.now {
            Some(now) => {
                let art = Rect::new(10, r.y + 8, height - 16, height - 16);
                p.box_(art, Color(0, 0, 0, 16), l.radius);
                p.symbol(
                    "music",
                    art.x + (art.width as i32 - 18) / 2,
                    art.y + (art.height as i32 - 18) / 2,
                    18,
                    l.accent,
                );
                let x = art.x + art.width as i32 + 12;
                p.strong(
                    x,
                    r.y + 10,
                    width.saturating_sub(x as u32 + 100),
                    &now.title,
                    13,
                    INK,
                );
                p.left(
                    x,
                    r.y + 28,
                    width.saturating_sub(x as u32 + 100),
                    &now.meta,
                    11,
                    MUTED,
                );
                let h = (height - 16).min(28);
                action(
                    p,
                    l,
                    Rect::new(
                        width as i32 - 86,
                        r.y + (height as i32 - h as i32) / 2,
                        76,
                        h,
                    ),
                    "Play",
                    &format!("music:play:{}", now.id),
                    true,
                );
            }
            None => {
                p.left(
                    12,
                    r.y + (height as i32 - 16) / 2,
                    width - 24,
                    "Nothing playing",
                    12,
                    FAINT,
                );
            }
        }
    }
    fn composer(
        &self,
        p: &mut Painter,
        l: &super::look::Look,
        width: u32,
        height: u32,
        draft: &str,
    ) {
        let h = 132.min(height.saturating_sub(20));
        let r = Rect::new(
            12,
            height as i32 - h as i32 - 10,
            width.saturating_sub(24),
            h,
        );
        p.drop_shadow(r, l.radius, 18, 60, 6);
        p.border(r, l.surface, l.radius, LINE);
        p.strong(
            r.x + 14,
            r.y + 12,
            r.width.saturating_sub(28),
            "New playlist",
            14,
            INK,
        );
        let field = Rect::new(r.x + 14, r.y + 38, r.width.saturating_sub(28), 30);
        p.border(field, Color::WHITE, 5, LINE);
        p.region(field, "music:new", "Playlist title");
        p.left(
            field.x + 8,
            field.y + 7,
            field.width.saturating_sub(16),
            if draft.is_empty() { "Title" } else { draft },
            13,
            if draft.is_empty() { FAINT } else { INK },
        );
        let create = Rect::new(r.x + 14, r.y + 82, 92, 30);
        if draft.trim().is_empty() {
            // An untitled playlist is refused by the service, so the button says so first.
            inert(p, l, create, "Create", "A playlist needs a title");
        } else {
            action(p, l, create, "Create", "music:create", true);
        }
        action(
            p,
            l,
            Rect::new(r.x + 114, r.y + 82, 92, 30),
            "Cancel",
            "music:cancel",
            false,
        );
    }
}

/// Percent-encode a query for a URL path. Only the characters a query really needs;
/// everything else is passed through so the service sees what was typed.
fn escape(query: &str) -> String {
    let mut out = String::new();
    for ch in query.chars() {
        match ch {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '-' | '_' | '.' | '~' => out.push(ch),
            ' ' => out.push_str("%20"),
            other => {
                let mut buffer = [0u8; 4];
                for byte in other.encode_utf8(&mut buffer).as_bytes() {
                    out.push_str(&format!("%{byte:02X}"));
                }
            }
        }
    }
    out
}

/// A service page flattened to the three things this application reads out of it: the text
/// each id carries, where each link goes, and the fill each card was painted with.
#[derive(Default)]
struct Flat {
    texts: Vec<(String, String)>,
    links: Vec<(String, String)>,
    cards: Vec<(String, Option<String>)>,
}
impl Flat {
    fn text(&self, id: &str) -> Option<String> {
        self.texts
            .iter()
            .find(|(candidate, _)| candidate == id)
            .map(|(_, text)| text.clone())
    }
    /// Every id shaped `{prefix}{key}{suffix}`, in the order the page listed them, which is
    /// the order the service chose to show them in.
    fn rows(&self, prefix: &str, suffix: &str) -> Vec<(String, String)> {
        self.texts
            .iter()
            .filter_map(|(id, text)| {
                let key = id.strip_prefix(prefix)?.strip_suffix(suffix)?;
                (!key.is_empty()).then(|| (key.to_owned(), text.clone()))
            })
            .collect()
    }
}
fn flatten(elements: &[cw_protocol::PageElement], out: &mut Flat) {
    use cw_protocol::PageElement as E;
    for element in elements {
        match element {
            E::Heading { id, text, .. }
            | E::Text { id, text }
            | E::Styled { id, text, .. }
            | E::Badge { id, text, .. } => out.texts.push((id.clone(), text.clone())),
            E::Link { id, text, url } => {
                out.texts.push((id.clone(), text.clone()));
                out.links.push((id.clone(), url.clone()));
            }
            E::Thumbnail { id, label, .. } => out.texts.push((id.clone(), label.clone())),
            E::Card {
                id,
                children,
                style,
                ..
            } => {
                out.cards.push((id.clone(), style.background.clone()));
                flatten(children, out);
            }
            E::Row { children, .. }
            | E::Grid { children, .. }
            | E::Group { children, .. }
            | E::Form { children, .. } => flatten(children, out),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cw_protocol::{PageAction, PageElement as E, PageTheme, Style};

    fn styled(id: &str, text: &str) -> E {
        E::Styled {
            id: id.into(),
            text: text.into(),
            style: Style::default(),
        }
    }
    fn pill(id: &str, on: bool) -> E {
        E::Card {
            id: id.into(),
            children: vec![],
            style: Style {
                background: Some(if on {
                    "#1db954".into()
                } else {
                    "#212121".into()
                }),
                ..Style::default()
            },
            action: Some(PageAction {
                method: "POST".into(),
                url: "/items/x/like".into(),
                fields: Default::default(),
            }),
        }
    }
    /// The browse page the `media` service really serves in audio mode, reduced to the
    /// elements this application reads.
    fn browse_page() -> String {
        let page = cw_protocol::Page {
            version: 1,
            title: "Sound".into(),
            theme: Some(PageTheme {
                accent: Some("#1db954".into()),
                ..PageTheme::default()
            }),
            elements: vec![
                E::Link {
                    id: "chrome-library".into(),
                    text: "Library".into(),
                    url: "/playlists".into(),
                },
                E::Grid {
                    id: "browse-artists".into(),
                    columns: 3,
                    gap: 16,
                    style: Style::default(),
                    children: vec![E::Card {
                        id: "artist-nova".into(),
                        style: Style::default(),
                        action: None,
                        children: vec![
                            styled("artist-nova-name", "Nova Bloom"),
                            styled("artist-nova-meta", "12,400 followers"),
                        ],
                    }],
                },
                E::Link {
                    id: "side-night-drive".into(),
                    text: "Night drive".into(),
                    url: "/playlist/night-drive".into(),
                },
                E::Row {
                    id: "line-backpressure".into(),
                    gap: 12,
                    align: "center".into(),
                    style: Style::default(),
                    children: vec![
                        E::Card {
                            id: "line-backpressure-open".into(),
                            style: Style::default(),
                            action: None,
                            children: vec![
                                styled("line-backpressure-title", "Backpressure"),
                                styled("line-backpressure-artist", "Nova Bloom · Latency"),
                            ],
                        },
                        pill("line-backpressure-play", false),
                        pill("line-backpressure-like", true),
                        E::Badge {
                            id: "line-backpressure-duration".into(),
                            text: "3:41".into(),
                            style: Style::default(),
                        },
                    ],
                },
                E::Row {
                    id: "bar".into(),
                    gap: 12,
                    align: "center".into(),
                    style: Style::default(),
                    children: vec![
                        E::Link {
                            id: "bar-title".into(),
                            text: "Backpressure".into(),
                            url: "/track/backpressure".into(),
                        },
                        styled("bar-meta", "Latency · 91 plays"),
                    ],
                },
            ],
        };
        serde_json::to_string(&page).unwrap()
    }
    fn app() -> Music {
        let (mut app, effects) = Music::launch("http://spotify.com/", 1, 0);
        let AppEffect::Http { url, tag, .. } = &effects[0] else {
            panic!("expected a request");
        };
        assert_eq!(
            (tag.as_str(), url.as_str()),
            ("browse", "http://spotify.com/")
        );
        app.http(1, "browse", 200, &browse_page()).unwrap();
        app
    }
    #[test]
    fn every_row_comes_from_the_service_reply() {
        let app = app();
        assert_eq!(app.status, Status::Idle);
        assert_eq!(app.heading, "Sound");
        assert_eq!(app.artists.len(), 1);
        assert_eq!(app.artists[0].id, "nova");
        assert_eq!(app.artists[0].meta, "12,400 followers");
        assert_eq!(app.tracks.len(), 1);
        assert_eq!(app.tracks[0].id, "backpressure");
        assert_eq!(app.tracks[0].subtitle, "Nova Bloom · Latency");
        assert_eq!(app.tracks[0].duration, "3:41");
        assert_eq!(app.playlists.len(), 1);
        assert_eq!(app.playlists[0].title, "Night drive");
        // The like pill's engaged fill is the page theme's accent, so the state is read,
        // not guessed.
        assert_eq!(app.liked, vec!["backpressure".to_owned()]);
        assert_eq!(app.now.as_ref().unwrap().id, "backpressure");
    }
    #[test]
    fn playing_and_liking_post_real_routes_and_then_re_read() {
        let mut app = app();
        let effects = app.click(1, "music:play:backpressure", 0).unwrap();
        let AppEffect::Http {
            method,
            url,
            body,
            tag,
            ..
        } = &effects[0]
        else {
            panic!("expected a request");
        };
        assert_eq!((method.as_str(), tag.as_str()), ("POST", "play"));
        assert_eq!(url, "http://spotify.com/api/items/backpressure/play");
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(body).unwrap()["list"],
            ""
        );
        let more = app.http(1, "play", 200, r#"{"plays":92}"#).unwrap();
        assert!(matches!(more.as_slice(), [AppEffect::Http { tag, .. }] if tag == "browse"));
        let effects = app.click(1, "music:like:backpressure", 0).unwrap();
        assert!(
            matches!(&effects[0], AppEffect::Http { url, .. } if url.ends_with("/api/items/backpressure/like"))
        );
        app.http(1, "like", 200, r#"{"liked":false}"#).unwrap();
        assert!(app.liked.is_empty());
        assert!(app.click(1, "music:play:nope", 0).is_err());
    }
    #[test]
    fn a_playlist_is_created_and_added_to_over_http() {
        let mut app = app();
        app.click(1, "music:new", 0).unwrap();
        assert!(
            app.click(1, "music:create", 0).is_err(),
            "an untitled playlist is refused"
        );
        app.text("Deep focus").unwrap();
        let effects = app.click(1, "music:create", 0).unwrap();
        let AppEffect::Http { url, body, .. } = &effects[0] else {
            panic!("expected a request");
        };
        assert_eq!(url, "http://spotify.com/api/playlists");
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(body).unwrap()["title"],
            "Deep focus"
        );
        app.http(1, "create", 200, r#"{"id":"deep-focus"}"#)
            .unwrap();
        assert!(app.draft.is_none());
        // Adding needs a chosen track; without one the control is never painted.
        assert!(app.click(1, "music:add:night-drive", 0).is_err());
        app.click(1, "music:track:backpressure", 0).unwrap();
        let effects = app.click(1, "music:add:night-drive", 0).unwrap();
        let AppEffect::Http { url, body, .. } = &effects[0] else {
            panic!("expected a request");
        };
        assert_eq!(url, "http://spotify.com/api/playlists/night-drive/items");
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(body).unwrap()["item"],
            "backpressure"
        );
    }
    #[test]
    fn searching_asks_the_service_and_an_empty_reply_renders_empty() {
        let mut app = app();
        assert!(app.click(1, "music:search", 0).is_err());
        app.click(1, "music:search-field", 0).unwrap();
        app.text("nova drive").unwrap();
        let effects = app.click(1, "music:search", 0).unwrap();
        assert!(
            matches!(&effects[0], AppEffect::Http { url, .. } if url == "http://spotify.com/search?q=nova%20drive")
        );
        let empty = serde_json::to_string(&cw_protocol::Page {
            version: 1,
            title: "nova drive - search".into(),
            elements: vec![styled("results-heading", "0 results for \"nova drive\"")],
            theme: None,
        })
        .unwrap();
        app.http(1, "results", 200, &empty).unwrap();
        assert!(app.tracks.is_empty());
        assert!(app.artists.is_empty());
        assert_eq!(app.heading, "nova drive - search");
    }
    #[test]
    fn an_unreachable_service_and_a_refusal_are_different_states() {
        let mut app = app();
        app.offline("browse", "network unreachable");
        assert_eq!(app.status, Status::Offline("network unreachable".into()));
        app.http(1, "browse", 403, r#"{"error":"catalogue unavailable"}"#)
            .unwrap();
        assert_eq!(app.status, Status::Denied("catalogue unavailable".into()));
    }
    #[test]
    fn every_painted_control_is_one_the_model_accepts() {
        for theme in [
            DesktopTheme::Macos,
            DesktopTheme::Windows,
            DesktopTheme::Ubuntu,
            DesktopTheme::Ios,
            DesktopTheme::Android,
        ] {
            let mut base = app();
            base.click(1, "music:track:backpressure", 0).unwrap();
            base.click(1, "music:new", 0).unwrap();
            let mut scene = Painter::themed(theme, 900, 640, 0);
            base.render(
                &mut scene,
                &crate::AppEnv {
                    theme,
                    width: 900,
                    height: 640,
                    clock_us: 0,
                    settings: &crate::SystemSettings::DEFAULT,
                    clipboard: None,
                    share_to: None,
                    files: Default::default(),
                },
            );
            let targets: Vec<_> = scene
                .scene
                .nodes
                .iter()
                .filter_map(|n| n.interaction.clone())
                .collect();
            assert!(!targets.is_empty());
            for target in targets {
                let mut app = base.clone();
                assert!(
                    app.click(1, &target, 0).is_ok(),
                    "unhandled control {target} on {theme:?}"
                );
            }
        }
    }
}
