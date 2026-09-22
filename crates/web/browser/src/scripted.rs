//! A document with script: the `cw_web::script::Realm` that owns its DOM, styles and
//! layout, the host the realm calls back into, and the bookkeeping that lets a realm
//! (a VM heap, which can be neither cloned nor serialised) live inside browser state
//! that is cloned for every snapshot and serialised for every checkpoint.
//!
//! Ownership. The realm owns the document; `WebDocument` keeps no second DOM for a
//! scripted page and reads the realm's `document()`, `styles()` and
//! `fragment_tree()` to paint, hit-test and project. A `Scripted` is a handle:
//!
//! * the live realm sits in a cell shared (by `Arc`) between every clone of the
//!   document, stamped with an *epoch* that every mutating entry bumps;
//! * each handle remembers the epoch it last saw. A handle whose epoch is the cell's
//!   owns the live realm. A handle left behind (the copy a snapshot holds, once the
//!   live world moved on) carries the `RealmState` it was cloned at, and rebuilds a
//!   realm of its own by journal replay the first time it is used again;
//! * cloning captures the `RealmState` once per epoch and shares it, so taking a
//!   snapshot costs one copy of the journal and restoring costs one replay, paid only
//!   if the restored copy is actually used.
//!
//! Serialisation writes the `RealmState`; deserialisation yields a handle with no
//! live realm, restored lazily. A new document is a new realm, so a journal never
//! outlives a navigation.
//!
//! The host. A realm's host must be `'static`, but what it needs (the transport
//! closure, the cookie jar, storage, the world clock, the tab's entropy stream) lives
//! in the `BrowserState` for the length of one call. So the host holds a shared
//! `HostEnv` slot: the browser moves those things into it before entering the realm
//! and takes them back afterwards. Navigations and submissions the page asks for are
//! queued there and performed by the browser after the entry returns, never
//! re-entrantly.
//!
//! Network policy for page script: `fetch`/XHR may reach any origin the transport
//! allows (there is no same-origin policy or CORS inside the simulated internet),
//! but cookies are attached, and `Set-Cookie` honoured, only for requests to the
//! document's own origin. `document.cookie` never shows or overwrites an `HttpOnly`
//! cookie.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, MutexGuard};

use cw_determinism::Determinism;
use cw_protocol::{HttpRequest, HttpResponse, Result};
use cw_web::script::{
    FetchRequest, FetchResponse, LogLevel, Realm, RealmState, ScriptHostDocument, StorageArea,
};
use cw_web::Viewport;
use serde::{Deserialize, Serialize};
use url::Url;

use crate::{cookie_path_matches, parse_cookie, Cookie, MAX_TEXT_RESOURCE_BYTES};

/// VM steps a document's load (each `<script>`) and each later entry may spend.
pub const STEP_BUDGET: u64 = 30_000_000;
/// Virtual milliseconds the event loop is given to settle after every event: zero
/// and near-zero timers (`$(fn)`, a framework's scheduler, a resolved `fetch`) and
/// one animation frame run before the next paint, as they would in a browser, even
/// though the world clock has not moved. The realm's clock may therefore run up to
/// this far ahead of the world's; it never goes back.
pub const SETTLE_MS: u32 = 20;
/// Requests one entry into the realm may make.
pub const MAX_FETCHES_PER_ENTRY: u32 = 64;
/// Lines a tab's console keeps.
pub const MAX_CONSOLE_LINES: usize = 500;

/// One line of a tab's console: `console.*` output, uncaught errors, budget
/// interruptions.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConsoleEntry {
    /// `log`, `info`, `warn`, `error` or `debug`.
    pub level: String,
    pub text: String,
}

/// What a page asked the browser to do once the current event is over.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PendingNav {
    Navigate {
        url: String,
        new_tab: bool,
    },
    Submit {
        action: String,
        method: String,
        enctype: String,
        data: Vec<(String, String)>,
    },
}

pub(crate) struct TransportPtr(*mut (dyn FnMut(HttpRequest) -> Result<HttpResponse> + 'static));
// SAFETY: the pointer is only dereferenced on the thread that installed it, inside
// the call that installed it (`HostEnv::with_transport`), while that call still
// holds the `&mut` it was made from.
unsafe impl Send for TransportPtr {}

/// Everything a realm's host needs from the browser for one entry.
#[derive(Default)]
pub struct HostEnv {
    pub(crate) transport: Option<TransportPtr>,
    pub cookies: BTreeMap<String, Vec<Cookie>>,
    /// localStorage, by origin.
    pub storage: BTreeMap<String, BTreeMap<String, String>>,
    /// The tab's sessionStorage, by origin.
    pub session: BTreeMap<String, BTreeMap<String, String>>,
    /// World clock, microseconds.
    pub now: u64,
    pub entropy: Option<Determinism>,
    pub stream: String,
    pub viewport: Viewport,
    pub console: Vec<ConsoleEntry>,
    pub navs: Vec<PendingNav>,
    /// The document's URL: its origin scopes storage and cookies.
    pub url: String,
    pub(crate) fetches: u32,
    /// Replaying a journal: console lines were already reported.
    pub(crate) muted: bool,
}

impl HostEnv {
    /// Installs `transport` for the length of `f`.
    pub fn with_transport<T>(
        &mut self,
        transport: Option<&mut (dyn FnMut(HttpRequest) -> Result<HttpResponse> + '_)>,
        f: impl FnOnce(&mut HostEnv) -> T,
    ) -> T {
        self.transport = transport.map(|t| {
            let p: *mut (dyn FnMut(HttpRequest) -> Result<HttpResponse> + '_) = t;
            // SAFETY: only the lifetime bound is erased; the pointer is cleared below,
            // before the borrow it came from ends.
            TransportPtr(unsafe {
                std::mem::transmute::<
                    *mut (dyn FnMut(HttpRequest) -> Result<HttpResponse> + '_),
                    *mut (dyn FnMut(HttpRequest) -> Result<HttpResponse> + 'static),
                >(p)
            })
        });
        let out = f(self);
        self.transport = None;
        out
    }

    fn origin(&self) -> String {
        Url::parse(&self.url)
            .map(|u| u.origin().ascii_serialization())
            .unwrap_or_default()
    }

    fn cookie_header(&self, url: &Url, for_script: bool) -> String {
        self.cookies
            .get(&url.origin().ascii_serialization())
            .map(|cookies| {
                cookies
                    .iter()
                    .filter(|c| {
                        cookie_path_matches(url.path(), &c.path)
                            && (!c.secure || url.scheme() == "https")
                            && !(for_script && c.http_only)
                    })
                    .map(|c| format!("{}={}", c.name, c.value))
                    .collect::<Vec<_>>()
                    .join("; ")
            })
            .unwrap_or_default()
    }

    fn store_cookie(&mut self, url: &Url, header: &str, from_script: bool) {
        let Some(cookie) = parse_cookie(header, url.path()) else {
            return;
        };
        if from_script && cookie.http_only {
            return;
        }
        let jar = self
            .cookies
            .entry(url.origin().ascii_serialization())
            .or_default();
        if from_script
            && jar
                .iter()
                .any(|c| c.name == cookie.name && c.path == cookie.path && c.http_only)
        {
            return;
        }
        jar.retain(|c| c.name != cookie.name || c.path != cookie.path);
        let lower = header.to_ascii_lowercase();
        if !lower.contains("max-age=0")
            && !lower.contains("max-age=-")
            && !lower.contains("expires=thu, 01 jan 1970")
        {
            jar.push(cookie);
            jar.sort_by(|a, b| (&a.name, &a.path).cmp(&(&b.name, &b.path)));
        }
    }

    fn fetch(&mut self, request: &FetchRequest) -> std::result::Result<FetchResponse, String> {
        let Some(TransportPtr(transport)) = self.transport.as_ref().map(|t| TransportPtr(t.0))
        else {
            return Err("network unavailable".into());
        };
        self.fetches += 1;
        if self.fetches > MAX_FETCHES_PER_ENTRY {
            return Err("too many requests".into());
        }
        let mut url = Url::parse(&request.url).map_err(|e| e.to_string())?;
        let document_origin = Url::parse(&self.url).ok().map(|u| u.origin());
        let mut method = request.method.to_ascii_uppercase();
        let mut body = request.body.clone().unwrap_or_default();
        let mut headers: BTreeMap<String, String> = request
            .headers
            .iter()
            .filter(|(k, _)| !k.eq_ignore_ascii_case("cookie"))
            .map(|(k, v)| (k.to_ascii_lowercase(), v.clone()))
            .collect();
        for _ in 0..=8 {
            if !matches!(url.scheme(), "http" | "https")
                || url.host_str().is_none()
                || !url.username().is_empty()
                || url.password().is_some()
            {
                return Err("browser supports credential-free http/https URLs only".into());
            }
            url.set_fragment(None);
            let same_origin = document_origin.as_ref() == Some(&url.origin());
            let mut out = HttpRequest::get(url.as_str());
            out.method = method.clone();
            out.headers = headers.clone();
            out.body = body.clone();
            if same_origin {
                let cookies = self.cookie_header(&url, false);
                if !cookies.is_empty() {
                    out.headers.insert("cookie".into(), cookies);
                }
            }
            // SAFETY: see `TransportPtr`; `with_transport` is on the stack above us.
            let response = unsafe { (*transport)(out) }.map_err(|e| e.message)?;
            if same_origin {
                if let Some(header) = response.header("set-cookie") {
                    let header = header.to_owned();
                    self.store_cookie(&url, &header, false);
                }
            }
            if matches!(response.status, 301 | 302 | 303 | 307 | 308) {
                if let Some(location) = response.header("location") {
                    url = url.join(location).map_err(|e| e.to_string())?;
                    if response.status == 303
                        || (matches!(response.status, 301 | 302) && method == "POST")
                    {
                        method = "GET".into();
                        body.clear();
                        headers.remove("content-type");
                    }
                    continue;
                }
            }
            if response.body.len() > MAX_TEXT_RESOURCE_BYTES {
                return Err("response exceeds the subresource budget".into());
            }
            return Ok(FetchResponse {
                status: response.status,
                status_text: status_text(response.status).into(),
                headers: response
                    .headers
                    .iter()
                    .map(|(k, v)| (k.to_ascii_lowercase(), v.clone()))
                    .collect(),
                body: response.body,
                url: url.to_string(),
            });
        }
        Err("redirect limit".into())
    }
}

fn status_text(status: u16) -> &'static str {
    match status {
        200 => "OK",
        201 => "Created",
        204 => "No Content",
        304 => "Not Modified",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        409 => "Conflict",
        422 => "Unprocessable Entity",
        500 => "Internal Server Error",
        503 => "Service Unavailable",
        _ => "",
    }
}

/// The realm's host: a handle on the slot the browser fills for each entry.
struct BrowserHost {
    env: Arc<Mutex<HostEnv>>,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    match m.lock() {
        Ok(g) => g,
        Err(p) => p.into_inner(),
    }
}

impl ScriptHostDocument for BrowserHost {
    fn fetch(&mut self, request: &FetchRequest) -> std::result::Result<FetchResponse, String> {
        lock(&self.env).fetch(request)
    }
    fn navigate(&mut self, url: &str) {
        let (url, new_tab) = match url.strip_prefix("cw-new-tab:") {
            Some(rest) => (rest, true),
            None => (url, false),
        };
        lock(&self.env).navs.push(PendingNav::Navigate {
            url: url.to_owned(),
            new_tab,
        });
    }
    fn submit_form(
        &mut self,
        action: &str,
        method: &str,
        enctype: &str,
        data: &[(String, String)],
    ) {
        lock(&self.env).navs.push(PendingNav::Submit {
            action: action.to_owned(),
            method: method.to_owned(),
            enctype: enctype.to_owned(),
            data: data.to_vec(),
        });
    }
    fn now_micros(&self) -> i64 {
        lock(&self.env).now.min(i64::MAX as u64) as i64
    }
    fn random_u64(&mut self) -> u64 {
        let mut env = lock(&self.env);
        let stream = env.stream.clone();
        env.entropy
            .get_or_insert_with(|| Determinism::new(0))
            .next_u64(&stream)
    }
    fn viewport(&self) -> Viewport {
        lock(&self.env).viewport
    }
    fn storage_get(&self, area: StorageArea, key: &str) -> Option<String> {
        let env = lock(&self.env);
        let origin = env.origin();
        match area {
            StorageArea::Local => env.storage.get(&origin),
            StorageArea::Session => env.session.get(&origin),
        }
        .and_then(|m| m.get(key).cloned())
    }
    fn storage_set(&mut self, area: StorageArea, key: &str, value: &str) {
        let mut env = lock(&self.env);
        let origin = env.origin();
        let map = match area {
            StorageArea::Local => &mut env.storage,
            StorageArea::Session => &mut env.session,
        };
        map.entry(origin)
            .or_default()
            .insert(key.to_owned(), value.to_owned());
    }
    fn storage_remove(&mut self, area: StorageArea, key: &str) {
        let mut env = lock(&self.env);
        let origin = env.origin();
        let map = match area {
            StorageArea::Local => &mut env.storage,
            StorageArea::Session => &mut env.session,
        };
        if let Some(m) = map.get_mut(&origin) {
            m.remove(key);
        }
    }
    fn storage_keys(&self, area: StorageArea) -> Vec<String> {
        let env = lock(&self.env);
        let origin = env.origin();
        match area {
            StorageArea::Local => env.storage.get(&origin),
            StorageArea::Session => env.session.get(&origin),
        }
        .map(|m| m.keys().cloned().collect())
        .unwrap_or_default()
    }
    fn cookie_get(&self) -> String {
        let env = lock(&self.env);
        match Url::parse(&env.url) {
            Ok(u) => env.cookie_header(&u, true),
            Err(_) => String::new(),
        }
    }
    fn cookie_set(&mut self, cookie: &str) {
        let mut env = lock(&self.env);
        if cookie
            .split(';')
            .skip(1)
            .any(|p| p.trim().eq_ignore_ascii_case("httponly"))
        {
            return;
        }
        if let Ok(u) = Url::parse(&env.url) {
            env.store_cookie(&u, cookie, true);
        }
    }
    fn log(&mut self, level: LogLevel, text: &str) {
        let mut env = lock(&self.env);
        if env.muted {
            return;
        }
        let level = match level {
            LogLevel::Error => "error",
            LogLevel::Warn => "warn",
            LogLevel::Info => "info",
            LogLevel::Debug => "debug",
            _ => "log",
        };
        env.console.push(ConsoleEntry {
            level: level.into(),
            text: text.to_owned(),
        });
    }
}

/// The live realm and the slot its host reads. `Realm` is full of `Rc`s.
///
/// SAFETY: every `Rc` a `Realm` holds is created inside it and dropped with it; no
/// reference into it ever leaves the mutex that guards the cell (`Scripted::enter`
/// and `Scripted::read` hand the closure a `&mut Realm` that cannot escape). The
/// reference counts are therefore only touched by the thread holding that mutex.
struct RealmCell {
    realm: Option<Realm>,
    epoch: u64,
    env: Arc<Mutex<HostEnv>>,
}
unsafe impl Send for RealmCell {}

/// What the browser mirrors out of the realm after every entry, so the chrome, the
/// observations and `refresh_pending` never have to enter the VM.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Mirror {
    /// World-clock microseconds of the earliest pending timer.
    pub next_timer: Option<i64>,
    pub wants_frame: bool,
    /// The realm's same-document history: `(index, length)`.
    pub history: (usize, usize),
    /// World tick timers last ran at (background tabs are throttled against it).
    pub last_run: u64,
    /// `visibilitychange` state last dispatched.
    pub hidden: bool,
}

struct Local {
    cell: Arc<Mutex<RealmCell>>,
    epoch: u64,
    /// The realm's state at `epoch`; always present when the cell has moved on or
    /// holds no realm.
    saved: Option<Arc<RealmState>>,
    mirror: Mirror,
}

/// A scripted document's handle on its realm. See the module documentation.
pub struct Scripted {
    local: Mutex<Local>,
}

#[derive(Serialize)]
struct ScriptedRef<'a> {
    state: &'a RealmState,
    mirror: &'a Mirror,
}
#[derive(Deserialize)]
struct ScriptedOwned {
    state: RealmState,
    #[serde(default)]
    mirror: Mirror,
}

impl Serialize for Scripted {
    fn serialize<S: serde::Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        let state = self.state();
        ScriptedRef {
            state: &state,
            mirror: &self.mirror(),
        }
        .serialize(s)
    }
}
impl<'de> Deserialize<'de> for Scripted {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        let o = ScriptedOwned::deserialize(d)?;
        Ok(Scripted::from_state(o.state, o.mirror))
    }
}
impl Clone for Scripted {
    fn clone(&self) -> Self {
        let state = self.state();
        let local = lock(&self.local);
        Scripted {
            local: Mutex::new(Local {
                cell: local.cell.clone(),
                epoch: local.epoch,
                saved: Some(state),
                mirror: local.mirror.clone(),
            }),
        }
    }
}
impl PartialEq for Scripted {
    fn eq(&self, o: &Self) -> bool {
        self.mirror() == o.mirror() && *self.state() == *o.state()
    }
}
impl Eq for Scripted {}

impl Scripted {
    /// A new realm for `html` at `url`; nothing has run yet.
    pub fn new(html: &str, url: &str, viewport: Viewport, now: u64) -> Scripted {
        let env = Arc::new(Mutex::new(HostEnv {
            viewport,
            now,
            url: url.to_owned(),
            ..HostEnv::default()
        }));
        let mut realm = Realm::new(html, url, Box::new(BrowserHost { env: env.clone() }));
        realm.set_step_budget(STEP_BUDGET);
        let cell = RealmCell {
            realm: Some(realm),
            epoch: 1,
            env,
        };
        Scripted {
            local: Mutex::new(Local {
                cell: Arc::new(Mutex::new(cell)),
                epoch: 1,
                saved: None,
                mirror: Mirror::default(),
            }),
        }
    }

    fn from_state(state: RealmState, mirror: Mirror) -> Scripted {
        let cell = RealmCell {
            realm: None,
            epoch: 0,
            env: Arc::new(Mutex::new(HostEnv::default())),
        };
        Scripted {
            local: Mutex::new(Local {
                cell: Arc::new(Mutex::new(cell)),
                epoch: 0,
                saved: Some(Arc::new(state)),
                mirror,
            }),
        }
    }

    pub fn mirror(&self) -> Mirror {
        lock(&self.local).mirror.clone()
    }

    pub fn update_mirror(&self, f: impl FnOnce(&mut Mirror)) {
        f(&mut lock(&self.local).mirror)
    }

    /// The realm's serialisable state as of this handle's epoch.
    pub fn state(&self) -> Arc<RealmState> {
        let mut local = lock(&self.local);
        if let Some(s) = &local.saved {
            return s.clone();
        }
        let state = {
            let cell = lock(&local.cell);
            Arc::new(
                cell.realm
                    .as_ref()
                    .expect("an unsaved handle owns the live realm")
                    .snapshot(),
            )
        };
        local.saved = Some(state.clone());
        state
    }

    /// How many inputs the realm's journal holds (the replay cost of a restore).
    pub fn journal_len(&self) -> usize {
        self.state().inputs.len()
    }

    /// Drops the live realm, keeping its state: what a document left behind in the
    /// history costs is its journal, not a VM heap.
    pub fn suspend(&self) {
        let _ = self.state();
        let local = lock(&self.local);
        let mut cell = lock(&local.cell);
        if cell.epoch == local.epoch {
            cell.realm = None;
        }
    }

    /// Makes sure this handle's cell holds the realm of this handle's epoch,
    /// rebuilding it by replay when the shared one moved on (or was never built).
    fn ready(local: &mut Local) {
        let in_sync = {
            let cell = lock(&local.cell);
            cell.epoch == local.epoch && cell.realm.is_some()
        };
        if in_sync {
            return;
        }
        let state = local
            .saved
            .clone()
            .expect("a handle out of step with its cell carries its state");
        let env = Arc::new(Mutex::new(HostEnv {
            muted: true,
            ..HostEnv::default()
        }));
        let realm = Realm::restore(&state, Box::new(BrowserHost { env: env.clone() }));
        lock(&env).muted = false;
        let fresh = RealmCell {
            realm: Some(realm),
            epoch: local.epoch,
            env,
        };
        if Arc::strong_count(&local.cell) == 1 {
            *lock(&local.cell) = fresh;
        } else {
            local.cell = Arc::new(Mutex::new(fresh));
        }
    }

    /// Enters the realm to change it. `env` is moved into the host's slot for the
    /// length of `f` and handed back with what the page did to it.
    pub fn enter<T>(&self, env: &mut HostEnv, f: impl FnOnce(&mut Realm) -> T) -> T {
        let mut local = lock(&self.local);
        Self::ready(&mut local);
        let cell_arc = local.cell.clone();
        let mut cell = lock(&cell_arc);
        let slot = cell.env.clone();
        env.fetches = 0;
        std::mem::swap(&mut *lock(&slot), env);
        let realm = cell.realm.as_mut().expect("ready");
        let out = f(realm);
        let (next_timer, wants_frame, history) = (
            realm.next_timer_micros(),
            realm.wants_animation_frame(),
            realm.history_position(),
        );
        std::mem::swap(&mut *lock(&slot), env);
        cell.epoch += 1;
        local.epoch = cell.epoch;
        local.saved = None;
        local.mirror.next_timer = next_timer;
        local.mirror.wants_frame = wants_frame;
        local.mirror.history = history;
        out
    }

    /// The handle's epoch: bumped by every `enter`, so it keys render caches.
    pub fn epoch(&self) -> u64 {
        lock(&self.local).epoch
    }

    /// Enters the realm to read it (paint, hit testing, projection). Layout may be
    /// flushed; nothing the page can observe changes.
    pub fn read<T>(&self, f: impl FnOnce(&mut Realm) -> T) -> T {
        let mut local = lock(&self.local);
        Self::ready(&mut local);
        let cell_arc = local.cell.clone();
        let mut cell = lock(&cell_arc);
        f(cell.realm.as_mut().expect("ready"))
    }
}

impl std::fmt::Debug for Scripted {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Scripted")
            .field("mirror", &self.mirror())
            .finish()
    }
}
