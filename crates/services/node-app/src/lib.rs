//! `node-app`: a world service that serves a real web application.
//!
//! An app is a [`Package`]: its frontend as built by its own toolchain (the files
//! under `public/`, served at the site's root with their media types), and, when it
//! has one, its own Node backend (the files under `server/`, mounted read-only at
//! `/app`, with the frontend's files at `/app/public`), which runs on the in-house JavaScript VM (`cw_jsvm::serve`) to answer
//! every request the frontend's files do not. Packages are code, like a Rust
//! service's handler: they are compiled into the registry (`NodeApp::new`) and never
//! serialised. A world names one by id.
//!
//! What is serialised is the instance state:
//!
//! ```json
//! {"package": "realworld-api", "files": {"/data/db.json": {...}}, "env": {"JWT_SECRET": "..."}, "requests": 0}
//! ```
//!
//! `files` is the app's writable disk: every file under the package's data
//! directory (`/data` unless the manifest says otherwise), as text, as a JSON value
//! (a seed, written out compactly when the app first reads it) or as
//! `{"bytes": [...]}`. The backend reads and writes them with Node's own `fs`, so an
//! app keeps its database where it always did (a JSON file, a Prisma schema swapped
//! for a file-backed client, lowdb), and a snapshot, a fork or a replay of the world
//! carries the app's data with it.
//!
//! Determinism: every request boots the backend on a fresh VM whose clock is the
//! world's (`tick` past the world epoch), whose entropy is drawn from the instance
//! seed and the number of requests served so far (`requests`), and whose files are
//! the package plus `files`. Nothing else survives between requests, so the same
//! state and the same request give the same bytes, however the world got there.
//! The backend has no network of its own: an outgoing request fails as if the
//! machine were offline.

use cw_jsvm::serve::{serve, ServeRequest};
use cw_protocol::{HttpRequest, HttpResponse, Result, SimError};
use cw_script_host::{FileStat, FsError, FsErrorKind, ScriptHost};
use cw_sdk::{Registry, Service, ServiceContext};
use serde::Deserialize;
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

/// Unix seconds of world tick 0 (2026-09-17T09:00:00Z), as the machines' clocks.
pub const EPOCH_UNIX_SECONDS: i64 = 1_789_635_600;

/// Instructions one request may execute, boot included, before it fails.
pub const STEP_BUDGET: u64 = 2_000_000_000;

/// How a package is served; `manifest.json` at the package's root.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub id: String,
    /// What the app is, for people reading a world.
    pub name: String,
    /// The package directory served at the site's root (default `public`).
    #[serde(default = "default_static")]
    pub r#static: String,
    /// A path under the static root served for any other `GET` that accepts HTML:
    /// a single-page app's `index.html`, so its client-side router sees deep links.
    #[serde(default)]
    pub spa_fallback: Option<String>,
    #[serde(default)]
    pub server: Option<ServerSpec>,
    /// Response headers added to every static file (a `cache-control`, say).
    #[serde(default)]
    pub headers: BTreeMap<String, String>,
}

fn default_static() -> String {
    "public".into()
}

/// The Node backend.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ServerSpec {
    /// The entry point, an absolute path under `/app` (the package's `server/`).
    pub main: String,
    /// Arguments after the script, as its own `package.json` would start it.
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default = "default_cwd")]
    pub cwd: String,
    /// The port it listens on (its own default or the `PORT` it is given).
    pub port: u16,
    /// Environment; the instance's `env` is laid over it.
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    /// The directory whose files persist in the instance state.
    #[serde(default = "default_data")]
    pub data: String,
}

fn default_cwd() -> String {
    "/app".into()
}
fn default_data() -> String {
    "/data".into()
}

/// A web app: its manifest and its files by path relative to the package root
/// (`public/index.html`, `server/server.js`).
#[derive(Clone, Debug)]
pub struct Package {
    pub manifest: Manifest,
    pub files: BTreeMap<String, &'static [u8]>,
}

impl Package {
    /// A package from its files; `manifest.json` among them.
    pub fn new(files: BTreeMap<String, &'static [u8]>) -> Result<Package> {
        let raw = files
            .get("manifest.json")
            .ok_or_else(|| SimError::invalid("a node-app package needs a manifest.json"))?;
        let manifest: Manifest = serde_json::from_slice(raw)
            .map_err(|e| SimError::invalid(format!("manifest.json: {e}")))?;
        if let Some(server) = &manifest.server {
            let rel = server.main.strip_prefix("/app/").ok_or_else(|| {
                SimError::invalid(format!("{}: server.main must be under /app", manifest.id))
            })?;
            if !files.contains_key(&format!("server/{rel}")) {
                return Err(SimError::invalid(format!(
                    "{}: server/{rel} is not in the package",
                    manifest.id
                )));
            }
        }
        Ok(Package { manifest, files })
    }

    /// The static file a `GET` of `path` names: the path itself, or a directory's
    /// `index.html`.
    fn static_file(&self, path: &str) -> Option<(String, &'static [u8])> {
        let root = &self.manifest.r#static;
        let rel = path.trim_start_matches('/');
        let candidates = if rel.is_empty() || rel.ends_with('/') {
            vec![format!("{root}/{rel}index.html")]
        } else {
            vec![format!("{root}/{rel}")]
        };
        candidates
            .into_iter()
            .find_map(|c| self.files.get(&c).map(|b| (c, *b)))
    }

    /// The package's bytes, for sizes in reports.
    pub fn size(&self) -> usize {
        self.files.values().map(|b| b.len()).sum()
    }
}

/// The `node-app` kind, with the packages this build carries.
pub struct NodeApp {
    packages: BTreeMap<String, Arc<Package>>,
}

impl NodeApp {
    pub fn new(packages: Vec<Package>) -> NodeApp {
        NodeApp {
            packages: packages
                .into_iter()
                .map(|p| (p.manifest.id.clone(), Arc::new(p)))
                .collect(),
        }
    }
    pub fn package(&self, id: &str) -> Option<&Package> {
        self.packages.get(id).map(|p| &**p)
    }
}

/// Registers `node-app` with these packages.
pub fn register(registry: &mut Registry, packages: Vec<Package>) -> Result<()> {
    registry.register(NodeApp::new(packages))
}

impl Service for NodeApp {
    fn kind(&self) -> &str {
        "node-app"
    }

    fn initialize(&self, mut initial: Value, _: &ServiceContext) -> Result<Value> {
        let obj = initial
            .as_object_mut()
            .ok_or_else(|| SimError::invalid("node-app state must be an object"))?;
        let id = obj
            .get("package")
            .and_then(Value::as_str)
            .ok_or_else(|| SimError::invalid("node-app needs a `package`"))?
            .to_owned();
        let package = self.packages.get(&id).ok_or_else(|| {
            SimError::invalid(format!("node-app package `{id}` is not in this build"))
        })?;
        for key in ["files", "env"] {
            let v = obj.entry(key).or_insert_with(|| json!({}));
            if !v.is_object() {
                return Err(SimError::invalid(format!(
                    "node-app `{key}` must be an object"
                )));
            }
        }
        let data = package
            .manifest
            .server
            .as_ref()
            .map(|s| s.data.clone())
            .unwrap_or_else(default_data);
        for (path, file) in obj["files"].as_object().unwrap() {
            if !path.starts_with(&format!("{data}/")) {
                return Err(SimError::invalid(format!(
                    "node-app file {path} is outside the app's data directory {data}"
                )));
            }
            if let Some(o) = file.as_object() {
                if o.len() == 1 && o.contains_key("bytes") {
                    serde_json::from_value::<Vec<u8>>(o["bytes"].clone())?;
                }
            }
        }
        for (k, v) in obj["env"].as_object().unwrap() {
            if !v.is_string() {
                return Err(SimError::invalid(format!(
                    "node-app env {k} must be a string"
                )));
            }
        }
        obj.entry("requests").or_insert(json!(0));
        Ok(initial)
    }

    fn handle(
        &self,
        state: &mut Value,
        ctx: &ServiceContext,
        req: &HttpRequest,
    ) -> Result<HttpResponse> {
        let id = state["package"].as_str().unwrap_or_default().to_owned();
        let Some(package) = self.packages.get(&id).cloned() else {
            return cw_service_common::error(503, format!("{id} is not installed in this build"));
        };
        let url = url::Url::parse(&req.url).ok();
        let path = url
            .as_ref()
            .map(|u| u.path().to_owned())
            .unwrap_or_else(|| "/".into());
        if matches!(req.method.as_str(), "GET" | "HEAD") {
            if let Some((file, bytes)) = package.static_file(&path) {
                return Ok(static_response(&package, &file, bytes, &req.method));
            }
            if !path.ends_with('/') && package.static_file(&format!("{path}/")).is_some() {
                return Ok(redirect(&format!("{path}/")));
            }
        }
        if let Some(server) = &package.manifest.server {
            return Ok(run_server(&package, server, state, ctx, req, url.as_ref()));
        }
        if req.method == "GET" && accepts_html(req) {
            if let Some(fallback) = &package.manifest.spa_fallback {
                if let Some((file, bytes)) = package.static_file(fallback) {
                    return Ok(static_response(&package, &file, bytes, "GET"));
                }
            }
        }
        cw_service_common::error(404, "not found")
    }
}

fn accepts_html(req: &HttpRequest) -> bool {
    match header(req, "accept") {
        Some(a) => a.contains("text/html") || a.contains("*/*"),
        None => true,
    }
}

fn header<'a>(req: &'a HttpRequest, name: &str) -> Option<&'a str> {
    req.headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(name))
        .map(|(_, v)| v.as_str())
}

fn static_response(package: &Package, file: &str, bytes: &[u8], method: &str) -> HttpResponse {
    let mut headers = BTreeMap::from([(
        "content-type".to_owned(),
        cw_service_static_site::content_type_of(file).to_owned(),
    )]);
    for (k, v) in &package.manifest.headers {
        headers.insert(k.to_ascii_lowercase(), v.clone());
    }
    HttpResponse {
        status: 200,
        headers,
        body: if method == "HEAD" {
            Vec::new()
        } else {
            bytes.to_vec()
        },
    }
}

fn redirect(to: &str) -> HttpResponse {
    HttpResponse {
        status: 301,
        headers: BTreeMap::from([("location".to_owned(), to.to_owned())]),
        body: Vec::new(),
    }
}

/// Headers that describe the one connection, not the response.
const HOP_BY_HOP: &[&str] = &[
    "connection",
    "keep-alive",
    "transfer-encoding",
    "content-length",
    "date",
];

fn run_server(
    package: &Package,
    server: &ServerSpec,
    state: &mut Value,
    ctx: &ServiceContext,
    req: &HttpRequest,
    url: Option<&url::Url>,
) -> HttpResponse {
    let served_before = state["requests"].as_u64().unwrap_or(0);
    let mut env: BTreeMap<String, String> = server.env.clone();
    for (k, v) in state["env"].as_object().into_iter().flatten() {
        env.insert(k.clone(), v.as_str().unwrap_or_default().to_owned());
    }
    env.entry("PORT".into())
        .or_insert_with(|| server.port.to_string());
    env.entry("NODE_ENV".into())
        .or_insert_with(|| "production".into());
    let mut host = AppHost::new(package, server, state, ctx, served_before);
    let mut path = url
        .map(|u| u.path().to_owned())
        .unwrap_or_else(|| "/".into());
    if let Some(q) = url.and_then(|u| u.query()) {
        path.push('?');
        path.push_str(q);
    }
    let mut headers: Vec<(String, String)> = req
        .headers
        .iter()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    if header(req, "host").is_none() {
        if let Some(h) = url.and_then(|u| u.host_str()) {
            headers.push(("Host".into(), h.to_owned()));
        }
    }
    if !req.body.is_empty() && header(req, "content-length").is_none() {
        headers.push(("Content-Length".into(), req.body.len().to_string()));
    }
    let request = ServeRequest {
        method: req.method.clone(),
        path,
        headers,
        body: req.body.clone(),
    };
    let served = serve(
        &mut host,
        &server.main,
        server.args.clone(),
        env.into_iter().collect(),
        server.port,
        &request,
        STEP_BUDGET,
    );
    if std::env::var_os("CW_NODE_APP_LOG").is_some() {
        eprintln!(
            "[node-app {}] {} {} -> {:?} ({} steps)\n{}{}",
            package.manifest.id,
            req.method,
            req.url,
            served.response.as_ref().map(|r| r.status),
            served.steps,
            served.stdout,
            served.stderr
        );
    }
    // Whatever the program wrote under its data directory is the app's state now,
    // whether or not it managed to answer.
    let files = host.persisted();
    state["files"] = Value::Object(files);
    state["requests"] = json!(served_before + 1);
    match served.response {
        Some(r) => {
            let mut headers = BTreeMap::new();
            for (k, v) in r.headers {
                let k = k.to_ascii_lowercase();
                if HOP_BY_HOP.contains(&k.as_str()) {
                    continue;
                }
                headers
                    .entry(k)
                    .and_modify(|cur: &mut String| {
                        cur.push_str(", ");
                        cur.push_str(&v);
                    })
                    .or_insert(v);
            }
            HttpResponse {
                status: r.status,
                headers,
                body: r.body,
            }
        }
        None => {
            let message = served
                .error
                .unwrap_or_else(|| "the app did not answer".into());
            HttpResponse {
                status: 502,
                headers: BTreeMap::from([(
                    "content-type".to_owned(),
                    "text/plain; charset=utf-8".to_owned(),
                )]),
                body: format!("{}: {message}\n", package.manifest.id).into_bytes(),
            }
        }
    }
}

/// The backend's machine: the package's `server/` read-only at `/app`, the
/// instance's `files` read-write, scratch space in `/tmp` that lasts one request,
/// the world clock and the instance's entropy.
struct AppHost<'a> {
    package: &'a Package,
    data: String,
    cwd: String,
    /// Absolute path -> contents, for everything written or seeded.
    files: BTreeMap<String, Vec<u8>>,
    dirs: BTreeSet<String>,
    now_micros: i64,
    rng: u64,
    hostname: String,
}

impl<'a> AppHost<'a> {
    fn new(
        package: &'a Package,
        server: &ServerSpec,
        state: &Value,
        ctx: &ServiceContext,
        served_before: u64,
    ) -> AppHost<'a> {
        let mut files = BTreeMap::new();
        for (path, v) in state["files"].as_object().into_iter().flatten() {
            let bytes = match v {
                Value::String(s) => s.as_bytes().to_vec(),
                Value::Object(o) if o.len() == 1 && o.contains_key("bytes") => {
                    serde_json::from_value(o["bytes"].clone()).unwrap_or_default()
                }
                other => serde_json::to_vec(other).unwrap_or_default(),
            };
            files.insert(path.clone(), bytes);
        }
        let mut dirs: BTreeSet<String> = ["/", "/tmp", "/app"]
            .into_iter()
            .map(String::from)
            .collect();
        dirs.insert(server.data.clone());
        dirs.insert(server.cwd.clone());
        let mut host = AppHost {
            package,
            data: server.data.clone(),
            cwd: server.cwd.clone(),
            files,
            dirs,
            now_micros: EPOCH_UNIX_SECONDS * 1_000_000 + ctx.tick as i64,
            rng: mix(ctx.seed ^ mix(served_before.wrapping_add(0x6e6f_6465))),
            hostname: ctx.instance.clone(),
        };
        let paths: Vec<String> = host
            .files
            .keys()
            .cloned()
            .chain(host.package_paths())
            .collect();
        for p in paths {
            host.add_parents(&p);
        }
        host
    }

    fn add_parents(&mut self, path: &str) {
        let mut cur = path;
        while let Some((parent, _)) = cur.rsplit_once('/') {
            let parent = if parent.is_empty() { "/" } else { parent };
            if !self.dirs.insert(parent.to_owned()) || parent == "/" {
                break;
            }
            cur = parent;
        }
    }

    fn package_file(&self, path: &str) -> Option<&'static [u8]> {
        let rel = path.strip_prefix("/app/")?;
        self.package
            .files
            .get(&format!("server/{rel}"))
            .or_else(|| self.package.files.get(&self.static_path(rel)?))
            .copied()
    }

    /// `/app/public/...` is the package's static files, where a backend that
    /// serves them itself (`express.static`) looks for them.
    fn static_path(&self, rel: &str) -> Option<String> {
        let rest = rel.strip_prefix("public/")?;
        Some(format!("{}/{rest}", self.package.manifest.r#static))
    }

    /// Every file of the package as the backend sees it, by absolute path.
    fn package_paths(&self) -> Vec<String> {
        let root = format!("{}/", self.package.manifest.r#static);
        self.package
            .files
            .keys()
            .filter_map(|k| {
                k.strip_prefix("server/")
                    .map(|r| format!("/app/{r}"))
                    .or_else(|| k.strip_prefix(&root).map(|r| format!("/app/public/{r}")))
            })
            .collect()
    }

    fn read_only(&self, path: &str) -> bool {
        path == "/app" || path.starts_with("/app/")
    }

    /// The data directory's files, as instance state.
    fn persisted(&self) -> Map<String, Value> {
        let prefix = format!("{}/", self.data);
        self.files
            .iter()
            .filter(|(p, _)| p.starts_with(&prefix))
            .map(|(p, b)| {
                let v = match std::str::from_utf8(b) {
                    Ok(s) => Value::String(s.to_owned()),
                    Err(_) => json!({ "bytes": b }),
                };
                (p.clone(), v)
            })
            .collect()
    }
}

fn mix(mut z: u64) -> u64 {
    z = z.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

fn fs_err(kind: FsErrorKind) -> FsError {
    FsError::new(kind)
}

fn normalize(path: &str) -> String {
    let mut parts: Vec<&str> = Vec::new();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            p => parts.push(p),
        }
    }
    format!("/{}", parts.join("/"))
}

impl ScriptHost for AppHost<'_> {
    fn read_file(&mut self, path: &str) -> std::result::Result<Vec<u8>, FsError> {
        let p = self.resolve(path);
        if let Some(b) = self.files.get(&p) {
            return Ok(b.clone());
        }
        if let Some(b) = self.package_file(&p) {
            return Ok(b.to_vec());
        }
        if self.dirs.contains(&p) {
            return Err(fs_err(FsErrorKind::IsADirectory));
        }
        Err(fs_err(FsErrorKind::NotFound))
    }
    fn write_file(
        &mut self,
        path: &str,
        data: &[u8],
        append: bool,
    ) -> std::result::Result<(), FsError> {
        let p = self.resolve(path);
        if self.read_only(&p) {
            return Err(fs_err(FsErrorKind::PermissionDenied));
        }
        let parent = p.rsplit_once('/').map(|(d, _)| d).unwrap_or("/");
        let parent = if parent.is_empty() { "/" } else { parent };
        if !self.dirs.contains(parent) {
            return Err(fs_err(FsErrorKind::NotFound));
        }
        if self.dirs.contains(&p) {
            return Err(fs_err(FsErrorKind::IsADirectory));
        }
        let entry = self.files.entry(p).or_default();
        if !append {
            entry.clear();
        }
        entry.extend_from_slice(data);
        Ok(())
    }
    fn stat(&mut self, path: &str) -> std::result::Result<FileStat, FsError> {
        let p = self.resolve(path);
        let size = if let Some(b) = self.files.get(&p) {
            Some(b.len())
        } else {
            self.package_file(&p).map(|b| b.len())
        };
        let (is_dir, size) = match size {
            Some(n) => (false, n),
            None if self.dirs.contains(&p) => (true, 4096),
            None => return Err(fs_err(FsErrorKind::NotFound)),
        };
        Ok(FileStat {
            size: size as u64,
            is_dir,
            is_symlink: false,
            mode: if is_dir { 0o755 } else { 0o644 },
            mtime_micros: self.now_micros,
            ..FileStat::default()
        })
    }
    fn list_dir(&mut self, path: &str) -> std::result::Result<Vec<String>, FsError> {
        let p = self.resolve(path);
        if !self.dirs.contains(&p) {
            return Err(fs_err(if self.files.contains_key(&p) {
                FsErrorKind::NotADirectory
            } else {
                FsErrorKind::NotFound
            }));
        }
        let prefix = if p == "/" {
            "/".to_owned()
        } else {
            format!("{p}/")
        };
        let mut names = BTreeSet::new();
        let package_paths = self.package_paths();
        for full in self
            .files
            .keys()
            .cloned()
            .chain(self.dirs.iter().cloned())
            .chain(package_paths)
        {
            if let Some(rest) = full.strip_prefix(&prefix) {
                if let Some(name) = rest.split('/').next().filter(|n| !n.is_empty()) {
                    names.insert(name.to_owned());
                }
            }
        }
        Ok(names.into_iter().collect())
    }
    fn mkdir(&mut self, path: &str, parents: bool) -> std::result::Result<(), FsError> {
        let p = self.resolve(path);
        if self.read_only(&p) {
            return Err(fs_err(FsErrorKind::PermissionDenied));
        }
        if self.dirs.contains(&p) {
            return if parents {
                Ok(())
            } else {
                Err(fs_err(FsErrorKind::Exists))
            };
        }
        if self.files.contains_key(&p) {
            return Err(fs_err(FsErrorKind::Exists));
        }
        let parent = p.rsplit_once('/').map(|(d, _)| d).unwrap_or("/");
        let parent = if parent.is_empty() { "/" } else { parent };
        if !self.dirs.contains(parent) {
            if !parents {
                return Err(fs_err(FsErrorKind::NotFound));
            }
            self.mkdir(parent, true)?;
        }
        self.dirs.insert(p);
        Ok(())
    }
    fn remove(&mut self, path: &str, recursive: bool) -> std::result::Result<(), FsError> {
        let p = self.resolve(path);
        if self.read_only(&p) {
            return Err(fs_err(FsErrorKind::PermissionDenied));
        }
        if self.files.remove(&p).is_some() {
            return Ok(());
        }
        if !self.dirs.contains(&p) {
            return Err(fs_err(FsErrorKind::NotFound));
        }
        let prefix = format!("{p}/");
        let has_children = self.files.keys().any(|k| k.starts_with(&prefix))
            || self.dirs.iter().any(|d| d.starts_with(&prefix));
        if has_children && !recursive {
            return Err(fs_err(FsErrorKind::NotEmpty));
        }
        self.files.retain(|k, _| !k.starts_with(&prefix));
        self.dirs.retain(|d| !d.starts_with(&prefix) && *d != p);
        Ok(())
    }
    fn rename(&mut self, from: &str, to: &str) -> std::result::Result<(), FsError> {
        let (f, t) = (self.resolve(from), self.resolve(to));
        if self.read_only(&f) || self.read_only(&t) {
            return Err(fs_err(FsErrorKind::PermissionDenied));
        }
        let Some(b) = self.files.remove(&f) else {
            return Err(fs_err(FsErrorKind::NotFound));
        };
        self.files.insert(t, b);
        Ok(())
    }
    fn cwd(&self) -> String {
        self.cwd.clone()
    }
    fn chdir(&mut self, path: &str) -> std::result::Result<(), FsError> {
        let p = self.resolve(path);
        if !self.dirs.contains(&p) {
            return Err(fs_err(FsErrorKind::NotFound));
        }
        self.cwd = p;
        Ok(())
    }
    fn resolve(&self, path: &str) -> String {
        if path.starts_with('/') {
            normalize(path)
        } else {
            normalize(&format!("{}/{path}", self.cwd))
        }
    }
    fn now_micros(&self) -> i64 {
        self.now_micros
    }
    fn random_u64(&mut self) -> u64 {
        self.rng = self.rng.wrapping_add(0x9E37_79B9_7F4A_7C15);
        mix(self.rng)
    }
    fn user(&self) -> String {
        "node".into()
    }
    fn hostname(&self) -> String {
        self.hostname.clone()
    }
    fn pid(&self) -> u64 {
        1
    }
}
