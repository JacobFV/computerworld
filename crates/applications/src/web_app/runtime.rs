//! The script layer under a web application, behind one interface so the host does
//! not care what runs the application: React on the JS VM (`JsRuntime`, always
//! available) or a compiled TSX runtime that builds the same cw-web document without
//! a VM. Whatever runs it, the host sees a cw-web `Document` with its styles and
//! fragment tree, sends `UiEvent`s in, delivers replies to requests, and drains an
//! `Outbox` of what the application asked for.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, MutexGuard};

use cw_web::dom::{Document, NodeId};
use cw_web::layout::fragment::FragmentTree;
use cw_web::script::{DefaultAction, LogLevel, Realm, ScriptHostDocument, StorageArea, UiEvent};
use cw_web::style::StyleSet;
use cw_web::Viewport;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// VM steps booting an application (React's own bundle included) may spend.
pub const BOOT_BUDGET: u64 = 200_000_000;
/// VM steps one later entry may spend.
pub const STEP_BUDGET: u64 = 30_000_000;
/// Virtual milliseconds the event loop runs after every entry, so a framework's
/// scheduler (React's `MessageChannel` task, a zero timer, a resolved promise) has
/// rendered before the host paints. The browser tab's `SETTLE_MS`, for the same reason.
pub const SETTLE_MS: u32 = 20;

/// The reserved key space of the channel (see `bridge.js`).
const KEY: &str = "\u{1}cw:";

/// One thing the application asked of its machine.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Request {
    Read {
        path: String,
    },
    Write {
        path: String,
        content: String,
    },
    List {
        path: String,
    },
    Mkdir {
        path: String,
    },
    Http {
        url: String,
        method: String,
        body: String,
    },
    Launch {
        app: String,
        argument: String,
    },
    Emit {
        name: String,
        data: Value,
    },
}

/// A message from the application to the host.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum Message {
    Request {
        id: u64,
        #[serde(flatten)]
        request: Request,
    },
    State {
        value: Value,
    },
    Refuse {
        message: String,
    },
    Chrome {
        chrome: Chrome,
    },
}

/// Facts the window frame shows that only the application knows: what it has open
/// and whether that has unsaved changes.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Chrome {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub document: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub caption: String,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub modified: bool,
}

/// What the application asked for during one entry.
#[derive(Debug, Default)]
pub struct Outbox {
    pub requests: Vec<(u64, Request)>,
    /// The last state it declared, if it declared one.
    pub state: Option<Value>,
    /// Why it refused the input, if it did.
    pub refusal: Option<String>,
    pub chrome: Option<Chrome>,
    /// Console lines and uncaught errors.
    pub logs: Vec<String>,
}

/// The answer to one request.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Reply {
    pub id: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}
impl Reply {
    pub fn ok(id: u64, value: Value) -> Self {
        Self {
            id,
            value: Some(value),
            error: None,
        }
    }
    pub fn err(id: u64, error: impl Into<String>) -> Self {
        Self {
            id,
            value: None,
            error: Some(error.into()),
        }
    }
}

/// What the application is told about where it runs. `css` carries the platform's
/// palette as custom properties (`--cw-accent`, …) and its UI typeface.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Env {
    pub platform: String,
    pub mobile: bool,
    pub width: u32,
    pub height: u32,
    pub css: String,
}

/// What a boot hands the application: its launch argument, the state it declared
/// (`None` on a first launch) and its environment.
#[derive(Clone, Debug, Serialize)]
pub struct Boot<'a> {
    pub kind: &'a str,
    pub argument: &'a str,
    pub state: Option<&'a Value>,
    pub env: &'a Env,
}

/// The document an application currently shows, laid out.
pub struct View<'a> {
    pub doc: &'a Document,
    pub styles: &'a StyleSet,
    pub tree: &'a FragmentTree,
    pub focused: Option<NodeId>,
    /// Live values of form controls.
    pub values: &'a BTreeMap<NodeId, String>,
    /// Selection `(start, end)` of text controls, in characters.
    pub selection: &'a BTreeMap<NodeId, (usize, usize)>,
}

/// A script backend. See the module documentation.
pub trait AppRuntime {
    /// Dispatches an input event and lets the application settle.
    fn dispatch(&mut self, event: UiEvent, now_us: u64) -> DefaultAction;
    /// Delivers replies to earlier requests and lets the application settle.
    fn deliver(&mut self, replies: &[Reply], now_us: u64);
    /// Tells the application its environment changed (a resize, another platform).
    fn set_env(&mut self, env: &Env, now_us: u64);
    /// Everything the application asked for since the last drain.
    fn drain(&mut self) -> Outbox;
    /// Reads the laid-out document.
    fn view(&mut self, f: &mut dyn FnMut(&View<'_>));
    /// VM inputs (or the backend's equivalent) since boot: what a live runtime has
    /// accumulated, which the host bounds by rebooting from declared state.
    fn weight(&self) -> usize;
    /// The backend's own complete state, for a backend that can snapshot itself
    /// (cw-ui); `None` when the application's declared state is what a snapshot keeps.
    fn snapshot(&mut self) -> Option<Value> {
        None
    }
}

/// The channel between the realm's host and the `JsRuntime` driving it.
#[derive(Default)]
struct Channel {
    boot: String,
    now_us: u64,
    viewport: Viewport,
    /// The application's own `localStorage`, kept for the realm's life only; state
    /// that must survive a snapshot goes through `cw.state`.
    local: BTreeMap<String, String>,
    session: BTreeMap<String, String>,
    outbox: Outbox,
    seed: u64,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    match m.lock() {
        Ok(g) => g,
        Err(p) => p.into_inner(),
    }
}

struct Host {
    channel: Arc<Mutex<Channel>>,
}

impl ScriptHostDocument for Host {
    fn now_micros(&self) -> i64 {
        lock(&self.channel).now_us.min(i64::MAX as u64) as i64
    }
    fn random_u64(&mut self) -> u64 {
        // SplitMix64 over a seed fixed by the application's kind: the VM draws once
        // to seed `Math.random`, so every boot of one kind draws the same sequence.
        let mut c = lock(&self.channel);
        c.seed = c.seed.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = c.seed;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    fn viewport(&self) -> Viewport {
        lock(&self.channel).viewport
    }
    fn storage_get(&self, area: StorageArea, key: &str) -> Option<String> {
        let c = lock(&self.channel);
        if let Some(reserved) = key.strip_prefix(KEY) {
            return match reserved {
                "boot" => Some(c.boot.clone()),
                "now" => Some(c.now_us.to_string()),
                _ => None,
            };
        }
        match area {
            StorageArea::Local => c.local.get(key).cloned(),
            StorageArea::Session => c.session.get(key).cloned(),
        }
    }
    fn storage_set(&mut self, area: StorageArea, key: &str, value: &str) {
        let mut c = lock(&self.channel);
        if key == "\u{1}cw:out" {
            match serde_json::from_str::<Message>(value) {
                Ok(Message::Request { id, request }) => c.outbox.requests.push((id, request)),
                Ok(Message::State { value }) => c.outbox.state = Some(value),
                Ok(Message::Refuse { message }) => c.outbox.refusal = Some(message),
                Ok(Message::Chrome { chrome }) => c.outbox.chrome = Some(chrome),
                Err(e) => c.outbox.logs.push(format!("error: cw: {e}")),
            }
            return;
        }
        if key.starts_with(KEY) {
            return;
        }
        match area {
            StorageArea::Local => c.local.insert(key.to_owned(), value.to_owned()),
            StorageArea::Session => c.session.insert(key.to_owned(), value.to_owned()),
        };
    }
    fn storage_remove(&mut self, area: StorageArea, key: &str) {
        let mut c = lock(&self.channel);
        match area {
            StorageArea::Local => c.local.remove(key),
            StorageArea::Session => c.session.remove(key),
        };
    }
    fn storage_keys(&self, area: StorageArea) -> Vec<String> {
        let c = lock(&self.channel);
        match area {
            StorageArea::Local => c.local.keys().cloned().collect(),
            StorageArea::Session => c.session.keys().cloned().collect(),
        }
    }
    fn log(&mut self, level: LogLevel, text: &str) {
        let level = match level {
            LogLevel::Error => "error",
            LogLevel::Warn => "warn",
            _ => "log",
        };
        lock(&self.channel)
            .outbox
            .logs
            .push(format!("{level}: {text}"));
    }
}

/// The page every JS application boots into. The theme sheet comes first so the
/// application's own styles can override it; the bridge runs before the bundle.
fn document(style: &str, script: &str, react: bool) -> String {
    // A literal `</script` inside a script would end it early.
    let guard = |s: &str| s.replace("</script", "<\\/script");
    let mut html = String::with_capacity(script.len() + style.len() + 300_000);
    html.push_str("<!DOCTYPE html><html><head><meta charset=\"utf-8\">");
    html.push_str("<style id=\"cw-theme\"></style><style>");
    html.push_str(style);
    html.push_str("</style></head><body><div id=\"root\"></div><script>");
    html.push_str(&guard(include_str!("bridge.js")));
    html.push_str("</script>");
    if react {
        html.push_str("<script>");
        html.push_str(&guard(include_str!(
            "../../web/vendor/react-18.3.1.production.min.js"
        )));
        html.push_str("</script><script>");
        html.push_str(&guard(include_str!(
            "../../web/vendor/react-dom-18.3.1.production.min.js"
        )));
        html.push_str("</script>");
    }
    html.push_str("<script>");
    html.push_str(&guard(script));
    html.push_str("</script></body></html>");
    html
}

/// An application running on the JS VM: a cw-web `Realm` whose page is the
/// application's bundle.
pub struct JsRuntime {
    realm: Realm,
    channel: Arc<Mutex<Channel>>,
    inputs: usize,
}

impl JsRuntime {
    /// Boots `script` (and React first when `react`), handing it `boot`.
    pub fn boot(style: &str, script: &str, react: bool, boot: &Boot<'_>, now_us: u64) -> Self {
        let viewport = Viewport {
            width: boot.env.width.max(1),
            height: boot.env.height.max(1),
            scale: 1,
            zoom: 100,
        };
        let seed = boot.kind.bytes().fold(0xCBF2_9CE4_8422_2325_u64, |h, b| {
            (h ^ u64::from(b)).wrapping_mul(0x100_0000_01B3)
        });
        let channel = Arc::new(Mutex::new(Channel {
            boot: serde_json::to_string(boot).expect("boot facts serialise"),
            now_us,
            viewport,
            seed,
            ..Channel::default()
        }));
        let html = document(style, script, react);
        let mut realm = Realm::new(
            &html,
            "cw-app://application/",
            Box::new(Host {
                channel: channel.clone(),
            }),
        );
        realm.set_step_budget(BOOT_BUDGET);
        realm.run_document();
        realm.run_until_idle(SETTLE_MS);
        realm.set_step_budget(STEP_BUDGET);
        Self {
            realm,
            channel,
            inputs: 2,
        }
    }

    fn at(&mut self, now_us: u64) {
        lock(&self.channel).now_us = now_us;
    }
}

impl AppRuntime for JsRuntime {
    fn dispatch(&mut self, event: UiEvent, now_us: u64) -> DefaultAction {
        self.at(now_us);
        let action = self.realm.dispatch(event);
        self.realm.run_until_idle(SETTLE_MS);
        self.inputs += 2;
        action
    }
    fn deliver(&mut self, replies: &[Reply], now_us: u64) {
        if replies.is_empty() {
            return;
        }
        self.at(now_us);
        let json = serde_json::to_string(replies).expect("replies serialise");
        let _ = self.realm.eval(&format!("__cw_deliver({json})"));
        self.realm.run_until_idle(SETTLE_MS);
        self.inputs += 2;
    }
    fn set_env(&mut self, env: &Env, now_us: u64) {
        self.at(now_us);
        let resized = {
            let mut c = lock(&self.channel);
            let before = c.viewport;
            c.viewport.width = env.width.max(1);
            c.viewport.height = env.height.max(1);
            before != c.viewport
        };
        if resized {
            self.realm.dispatch(UiEvent::Resize {
                width: env.width.max(1),
                height: env.height.max(1),
            });
            self.inputs += 1;
        }
        let json = serde_json::to_string(env).expect("environment serialises");
        let _ = self.realm.eval(&format!("__cw_env({json})"));
        self.realm.run_until_idle(SETTLE_MS);
        self.inputs += 2;
    }
    fn drain(&mut self) -> Outbox {
        std::mem::take(&mut lock(&self.channel).outbox)
    }
    fn view(&mut self, f: &mut dyn FnMut(&View<'_>)) {
        let inner = self.realm.layout();
        let Some(tree) = inner.tree.as_ref() else {
            return;
        };
        f(&View {
            doc: &inner.doc,
            styles: &inner.styles,
            tree,
            focused: inner.focused,
            values: &inner.form.values,
            selection: &inner.form.selection,
        });
    }
    fn weight(&self) -> usize {
        self.inputs
    }
}

/// The page shell a compiled application renders into: the theme sheet, its own
/// stylesheet and the container. Nothing in it runs.
fn shell(style: &str, env: &Env) -> String {
    format!(
        "<!DOCTYPE html><html data-platform=\"{}\"{}><head><meta charset=\"utf-8\">\
         <style id=\"cw-theme\">{}</style><style>{}</style></head>\
         <body><div id=\"root\"></div></body></html>",
        env.platform,
        if env.mobile { " data-mobile" } else { "" },
        env.css,
        style
    )
}

/// An application compiled by cw-tsx, run by cw-ui on the document with React 18's
/// semantics and no VM. Its snapshot is cw-ui's own state.
pub struct UiRuntime {
    app: cw_ui::UiApp,
    channel: Arc<Mutex<Channel>>,
}

impl UiRuntime {
    fn host(boot: &Boot<'_>, now_us: u64) -> (Arc<Mutex<Channel>>, Box<Host>) {
        let channel = Arc::new(Mutex::new(Channel {
            boot: serde_json::to_string(boot).expect("boot facts serialise"),
            now_us,
            viewport: Viewport {
                width: boot.env.width.max(1),
                height: boot.env.height.max(1),
                scale: 1,
                zoom: 100,
            },
            seed: boot.kind.bytes().fold(0xCBF2_9CE4_8422_2325_u64, |h, b| {
                (h ^ u64::from(b)).wrapping_mul(0x100_0000_01B3)
            }),
            ..Channel::default()
        }));
        let host = Box::new(Host {
            channel: channel.clone(),
        });
        (channel, host)
    }

    /// Mounts the IR `ir` in a fresh document, or restores it from `state` (a
    /// snapshot this backend took).
    pub fn boot(
        ir: &str,
        style: &str,
        boot: &Boot<'_>,
        state: Option<&Value>,
        now_us: u64,
    ) -> Result<Self, String> {
        let (channel, host) = Self::host(boot, now_us);
        // cw-ui's own snapshot restores; state the app declared boots it afresh
        // with that state in its boot facts, as on the JS backend.
        let own = state
            .filter(|s| !s.is_null())
            .and_then(|s| cw_ui::UiState::from_json(&s.to_string()).ok());
        let app = match own {
            Some(state) => cw_ui::UiApp::restore(&state, host).map_err(|e| e.to_string())?,
            None => {
                let module = cw_ui::UiApp::parse_ir(ir).map_err(|e| e.to_string())?;
                let mut app = cw_ui::UiApp::new(
                    module,
                    &shell(style, boot.env),
                    "cw-app://application/",
                    host,
                )
                .map_err(|e| e.to_string())?;
                app.boot();
                app.run_until_idle(SETTLE_MS);
                app
            }
        };
        Ok(Self { app, channel })
    }
}

impl AppRuntime for UiRuntime {
    fn dispatch(&mut self, event: UiEvent, now_us: u64) -> DefaultAction {
        lock(&self.channel).now_us = now_us;
        let action = self.app.dispatch(event);
        self.app.run_until_idle(SETTLE_MS);
        action
    }
    fn deliver(&mut self, replies: &[Reply], now_us: u64) {
        if replies.is_empty() {
            return;
        }
        lock(&self.channel).now_us = now_us;
        let json = serde_json::to_string(replies).expect("replies serialise");
        let _ = self.app.cw_deliver(&json);
        self.app.run_until_idle(SETTLE_MS);
    }
    fn set_env(&mut self, env: &Env, now_us: u64) {
        lock(&self.channel).now_us = now_us;
        let resized = {
            let mut c = lock(&self.channel);
            let before = c.viewport;
            c.viewport.width = env.width.max(1);
            c.viewport.height = env.height.max(1);
            before != c.viewport
        };
        {
            let inner = self.app.inner();
            if let Some(html) = inner.doc.document_element() {
                inner.doc.set_attr(html, "data-platform", &env.platform);
                if env.mobile {
                    inner.doc.set_attr(html, "data-mobile", "");
                } else {
                    inner.doc.remove_attr(html, "data-mobile");
                }
            }
            let sheet = inner.doc.by_id("cw-theme").first().copied();
            if let Some(text) = sheet.and_then(|s| inner.doc.first_child(s)) {
                inner.doc.set_text(text, &env.css);
                inner.sheets_dirty = true;
            }
            inner.touch();
        }
        if resized {
            self.app.dispatch(UiEvent::Resize {
                width: env.width.max(1),
                height: env.height.max(1),
            });
        }
        let json = serde_json::to_string(env).expect("environment serialises");
        let _ = self.app.cw_env(&json);
        self.app.run_until_idle(SETTLE_MS);
    }
    fn drain(&mut self) -> Outbox {
        std::mem::take(&mut lock(&self.channel).outbox)
    }
    fn view(&mut self, f: &mut dyn FnMut(&View<'_>)) {
        let inner = self.app.inner();
        inner.ensure_layout();
        let Some(tree) = inner.tree.as_ref() else {
            return;
        };
        f(&View {
            doc: &inner.doc,
            styles: &inner.styles,
            tree,
            focused: inner.focused,
            values: &inner.form.values,
            selection: &inner.form.selection,
        });
    }
    fn weight(&self) -> usize {
        0
    }
    fn snapshot(&mut self) -> Option<Value> {
        if self.app.declares_state() {
            return None;
        }
        serde_json::from_str(&self.app.snapshot().to_json()).ok()
    }
}
