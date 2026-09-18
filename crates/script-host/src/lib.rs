//! The only window an embedded language runtime has onto the world.
//!
//! `python3` and `node` run inside the simulation, so they must never touch the
//! host: files come from the machine's VFS, time from the world clock, and entropy
//! from the world's seeded determinism. Each interpreter crate is written against
//! this trait; `cw-computer` implements it once over a `Computer`.
//!
//! Beyond files, clock and entropy the surface covers the world's network (HTTP,
//! DNS, TCP reachability), child processes run by the machine's own shell, the
//! seed of the deterministic thread scheduler, interactive standard input (a run
//! that needs a line nobody has typed yet suspends, and is resumed by replaying
//! its [`journal`]), and a Debug-Adapter-Protocol-shaped [`debug`] interface.

pub mod dap;
pub mod debug;
pub mod journal;

/// Why a filesystem operation failed, in the vocabulary both runtimes translate
/// into their own errors (`FileNotFoundError: [Errno 2] …`, `ENOENT: …`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FsErrorKind {
    NotFound,
    Exists,
    NotADirectory,
    IsADirectory,
    NotEmpty,
    PermissionDenied,
    Invalid,
}
impl FsErrorKind {
    /// POSIX errno number, as CPython reports it in `[Errno N]`.
    pub fn errno(self) -> i32 {
        match self {
            Self::NotFound => 2,
            Self::PermissionDenied => 13,
            Self::Exists => 17,
            Self::NotADirectory => 20,
            Self::IsADirectory => 21,
            Self::Invalid => 22,
            Self::NotEmpty => 39,
        }
    }
    /// The symbolic code Node puts in `err.code`.
    pub fn code(self) -> &'static str {
        match self {
            Self::NotFound => "ENOENT",
            Self::PermissionDenied => "EACCES",
            Self::Exists => "EEXIST",
            Self::NotADirectory => "ENOTDIR",
            Self::IsADirectory => "EISDIR",
            Self::Invalid => "EINVAL",
            Self::NotEmpty => "ENOTEMPTY",
        }
    }
    /// `strerror` text for the errno.
    pub fn strerror(self) -> &'static str {
        match self {
            Self::NotFound => "No such file or directory",
            Self::PermissionDenied => "Permission denied",
            Self::Exists => "File exists",
            Self::NotADirectory => "Not a directory",
            Self::IsADirectory => "Is a directory",
            Self::Invalid => "Invalid argument",
            Self::NotEmpty => "Directory not empty",
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FsError {
    pub kind: FsErrorKind,
}
impl FsError {
    pub fn new(kind: FsErrorKind) -> Self {
        Self { kind }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FileStat {
    pub size: u64,
    pub is_dir: bool,
    pub is_symlink: bool,
    /// Permission bits only (e.g. `0o644`).
    pub mode: u32,
    /// Modification time in microseconds since the Unix epoch.
    pub mtime_micros: i64,
    pub inode: u64,
    pub links: u32,
    pub owner: String,
}

/// Paths are passed exactly as the program wrote them; the host resolves a relative
/// path against [`ScriptHost::cwd`].
pub trait ScriptHost {
    fn read_file(&mut self, path: &str) -> Result<Vec<u8>, FsError>;
    /// Creates the file if missing. `append` adds to the end instead of truncating.
    fn write_file(&mut self, path: &str, data: &[u8], append: bool) -> Result<(), FsError>;
    /// Follows symlinks.
    fn stat(&mut self, path: &str) -> Result<FileStat, FsError>;
    /// Does not follow a final symlink.
    fn lstat(&mut self, path: &str) -> Result<FileStat, FsError> {
        self.stat(path)
    }
    /// Entry names, sorted, without `.` and `..`.
    fn list_dir(&mut self, path: &str) -> Result<Vec<String>, FsError>;
    fn mkdir(&mut self, path: &str, parents: bool) -> Result<(), FsError>;
    /// Removes a file, or a directory (which must be empty unless `recursive`).
    fn remove(&mut self, path: &str, recursive: bool) -> Result<(), FsError>;
    fn rename(&mut self, from: &str, to: &str) -> Result<(), FsError>;
    /// Absolute, canonical working directory.
    fn cwd(&self) -> String;
    fn chdir(&mut self, path: &str) -> Result<(), FsError>;
    /// Canonical absolute form of `path` relative to the working directory.
    fn resolve(&self, path: &str) -> String;
    /// The world clock: microseconds since the Unix epoch.
    fn now_micros(&self) -> i64;
    /// Deterministic entropy from the world's seeded streams.
    fn random_u64(&mut self) -> u64;
    fn user(&self) -> String;
    fn hostname(&self) -> String;
    fn pid(&self) -> u64;
    fn os_family(&self) -> String {
        "linux".into()
    }

    // ---------------------------------------------------------------- network
    /// One HTTP exchange through the world's network, exactly as the machine's other
    /// clients (the browser, `curl`) make it: DNS, routes, service availability and
    /// failures are the world's. The response carries the simulated latency.
    fn http(&mut self, request: &HttpRequest) -> Result<HttpResponse, NetError> {
        let _ = request;
        Err(NetError::unavailable())
    }
    /// Name resolution against the world's DNS (`getaddrinfo`, `dns.lookup`).
    fn resolve_host(&mut self, name: &str) -> Result<Vec<String>, NetError> {
        let _ = name;
        Err(NetError::unavailable())
    }
    /// Opens a TCP connection to `host:port` (DNS, routing and the listener are checked
    /// by the world). Bytes on it are carried by the runtime's HTTP stream shim, since
    /// every simulated service speaks HTTP.
    fn tcp_connect(&mut self, host: &str, port: u16) -> Result<TcpConnection, NetError> {
        let _ = (host, port);
        Err(NetError::unavailable())
    }
    /// This machine's primary IPv4 address (`socket.getsockname`, `os.networkInterfaces`).
    fn local_address(&self) -> String {
        "127.0.0.1".into()
    }

    // ---------------------------------------------------------------- processes
    /// Runs a child process through the machine's own shell (nested `python3` and
    /// `node` included) and waits for it. `Err` only when the program named by an
    /// argv spawn does not exist; a failing command is an `Ok` with its status.
    fn spawn(&mut self, request: &SpawnRequest) -> Result<Outcome, FsError> {
        let _ = request;
        Err(FsError::new(FsErrorKind::NotFound))
    }

    // ---------------------------------------------------------------- scheduling
    /// Seed for the deterministic thread scheduler: the preemption quantum derives
    /// from it, so a race replays identically under one world seed.
    fn scheduler_seed(&mut self) -> u64 {
        0x5eed_7ead_2026_0917
    }
}

/// An HTTP request as a runtime sends it. Header names keep the program's spelling.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HttpRequest {
    pub method: String,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
    /// Give up (with [`NetErrorKind::TimedOut`]) when the exchange takes longer.
    pub timeout_micros: Option<u64>,
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HttpResponse {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
    /// Simulated time the exchange took (DNS, link latency, the service's work).
    pub elapsed_micros: u64,
}
impl HttpResponse {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }
}
/// The standard reason phrase for a status code.
pub fn reason_phrase(status: u16) -> &'static str {
    match status {
        100 => "Continue",
        101 => "Switching Protocols",
        200 => "OK",
        201 => "Created",
        202 => "Accepted",
        204 => "No Content",
        206 => "Partial Content",
        301 => "Moved Permanently",
        302 => "Found",
        303 => "See Other",
        304 => "Not Modified",
        307 => "Temporary Redirect",
        308 => "Permanent Redirect",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        406 => "Not Acceptable",
        408 => "Request Timeout",
        409 => "Conflict",
        410 => "Gone",
        411 => "Length Required",
        413 => "Payload Too Large",
        415 => "Unsupported Media Type",
        418 => "I'm a Teapot",
        422 => "Unprocessable Entity",
        429 => "Too Many Requests",
        500 => "Internal Server Error",
        501 => "Not Implemented",
        502 => "Bad Gateway",
        503 => "Service Unavailable",
        504 => "Gateway Timeout",
        _ => "Unknown",
    }
}
/// A TCP connection the world accepted.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TcpConnection {
    pub remote_address: String,
    pub remote_port: u16,
    pub local_address: String,
    pub local_port: u16,
    pub elapsed_micros: u64,
}
/// Why a network operation failed, in the vocabulary both runtimes translate
/// (`socket.gaierror`, `ConnectionRefusedError`, `ECONNREFUSED`, `ENOTFOUND`…).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetErrorKind {
    /// DNS has no such name.
    NameNotFound,
    /// Nothing listens there (or the service is stopped).
    Refused,
    /// No route to the destination.
    Unreachable,
    /// The exchange exceeded its deadline.
    TimedOut,
    /// The world's network policy forbids it.
    Denied,
    /// The connection was dropped (packet loss, a service failure mid-exchange).
    Reset,
    /// A malformed URL or address.
    Invalid,
    /// This host has no network at all.
    Unavailable,
}
impl NetErrorKind {
    /// Linux errno (`EAI_NONAME` is negative, as `socket.gaierror` reports it).
    pub fn errno(self) -> i32 {
        match self {
            Self::NameNotFound => -2,
            Self::Refused => 111,
            Self::Unreachable => 113,
            Self::TimedOut => 110,
            Self::Denied => 13,
            Self::Reset => 104,
            Self::Invalid => 22,
            Self::Unavailable => 101,
        }
    }
    /// libuv / Node error code.
    pub fn code(self) -> &'static str {
        match self {
            Self::NameNotFound => "ENOTFOUND",
            Self::Refused => "ECONNREFUSED",
            Self::Unreachable => "EHOSTUNREACH",
            Self::TimedOut => "ETIMEDOUT",
            Self::Denied => "EACCES",
            Self::Reset => "ECONNRESET",
            Self::Invalid => "EINVAL",
            Self::Unavailable => "ENETUNREACH",
        }
    }
    pub fn strerror(self) -> &'static str {
        match self {
            Self::NameNotFound => "Name or service not known",
            Self::Refused => "Connection refused",
            Self::Unreachable => "No route to host",
            Self::TimedOut => "Connection timed out",
            Self::Denied => "Permission denied",
            Self::Reset => "Connection reset by peer",
            Self::Invalid => "Invalid argument",
            Self::Unavailable => "Network is unreachable",
        }
    }
    /// Stable tag used by the journal.
    pub fn index(self) -> u8 {
        self as u8
    }
    pub fn from_index(i: u8) -> Self {
        match i {
            0 => Self::NameNotFound,
            1 => Self::Refused,
            2 => Self::Unreachable,
            3 => Self::TimedOut,
            4 => Self::Denied,
            5 => Self::Reset,
            6 => Self::Invalid,
            _ => Self::Unavailable,
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NetError {
    pub kind: NetErrorKind,
    pub message: String,
}
impl NetError {
    pub fn new(kind: NetErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }
    pub fn unavailable() -> Self {
        Self::new(NetErrorKind::Unavailable, "network adapter unavailable")
    }
}

/// What a child process runs: a shell command line (`shell=True`, `exec`) or an
/// argument vector looked up on `PATH` (`execFile`, `subprocess.run([...])`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpawnProgram {
    Shell(String),
    Argv(Vec<String>),
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpawnRequest {
    pub program: SpawnProgram,
    pub stdin: String,
    /// Working directory; the parent's when `None`.
    pub cwd: Option<String>,
    /// The complete environment; the parent's when `None`.
    pub env: Option<Vec<(String, String)>>,
}

/// Quotes one word for the POSIX shell (`shlex.quote`).
pub fn shell_quote(word: &str) -> String {
    if !word.is_empty()
        && word
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "@%+=:,./-_".contains(c))
    {
        return word.to_string();
    }
    format!("'{}'", word.replace('\'', "'\"'\"'"))
}

/// What a runtime is asked to do: the arguments after the interpreter name exactly as
/// the command line gave them (`["-c", "print(1)"]`, `["main.py", "a"]`), its
/// environment, and the text waiting on standard input.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Invocation {
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
    pub stdin: String,
    /// Standard input is a terminal: more lines may be typed after `stdin` is used up,
    /// so a read past its end suspends the run ([`Outcome::awaiting_input`]) instead of
    /// seeing end-of-file, and a bare `python3` / `node` starts the REPL. Output is one
    /// stream (the terminal's), with typed lines echoed where they were read.
    pub interactive: bool,
    /// End-of-file was typed (Ctrl+D) after `stdin`: reads past it see EOF.
    pub eof: bool,
}

/// A finished run: both streams and the process exit status.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Outcome {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
    /// An interactive run stopped because it wants a line nobody has typed yet. Rerun
    /// it with that line appended to `stdin` (and the host replaying its journal) to
    /// continue exactly where it stopped.
    pub awaiting_input: bool,
    /// Virtual time the run took (sleeps, timers, network waits, executed code), so
    /// a parent waiting on it (`subprocess` timeouts, `child_process`) can tell.
    pub elapsed_micros: u64,
}
impl Outcome {
    pub fn new(stdout: impl Into<String>, stderr: impl Into<String>, exit_code: i32) -> Self {
        Self {
            stdout: stdout.into(),
            stderr: stderr.into(),
            exit_code,
            awaiting_input: false,
            elapsed_micros: 0,
        }
    }
}

/// An in-memory host for runtime unit tests: a flat map of absolute paths.
pub mod memory {
    use super::*;
    use std::collections::{BTreeMap, BTreeSet};

    #[derive(Debug, Clone)]
    pub struct MemoryHost {
        pub files: BTreeMap<String, Vec<u8>>,
        pub dirs: BTreeSet<String>,
        pub cwd: String,
        pub now_micros: i64,
        pub rng: u64,
        /// A toy network: DNS names and their addresses. `echo.test` answers every
        /// request by describing it (method, path, headers, body) as JSON; other known
        /// hosts serve `pages` (URL path -> body) and 404 otherwise.
        pub dns: BTreeMap<String, Vec<String>>,
        pub pages: BTreeMap<String, Vec<u8>>,
        /// Every HTTP request made, for assertions.
        pub requests: Vec<HttpRequest>,
    }
    impl Default for MemoryHost {
        fn default() -> Self {
            let mut dirs = BTreeSet::new();
            for d in ["/", "/tmp", "/home", "/home/user"] {
                dirs.insert(d.to_string());
            }
            Self {
                files: BTreeMap::new(),
                dirs,
                cwd: "/home/user".into(),
                // 2026-09-17T09:00:00Z, the simulation's tick 0.
                now_micros: 1_789_635_600_000_000,
                rng: 0x243f_6a88_85a3_08d3,
                dns: BTreeMap::from([
                    ("echo.test".to_string(), vec!["192.0.2.10".to_string()]),
                    ("site.test".to_string(), vec!["192.0.2.11".to_string()]),
                ]),
                pages: BTreeMap::new(),
                requests: vec![],
            }
        }
    }
    /// `(host, port, path)` of an `http://` or `https://` URL.
    pub fn split_url(url: &str) -> Option<(String, u16, String)> {
        let (scheme, rest) = url.split_once("://")?;
        let default = match scheme {
            "http" => 80,
            "https" => 443,
            _ => return None,
        };
        let (authority, path) = match rest.find('/') {
            Some(i) => (&rest[..i], rest[i..].to_string()),
            None => (rest, "/".to_string()),
        };
        let (host, port) = match authority.rsplit_once(':') {
            Some((h, p)) if !h.ends_with(']') || p.parse::<u16>().is_ok() => {
                (h.to_string(), p.parse().ok()?)
            }
            _ => (authority.to_string(), default),
        };
        Some((
            host.trim_matches(['[', ']']).to_ascii_lowercase(),
            port,
            path,
        ))
    }
    fn json_str(s: &str) -> String {
        let mut o = String::from("\"");
        for c in s.chars() {
            match c {
                '"' => o.push_str("\\\""),
                '\\' => o.push_str("\\\\"),
                '\n' => o.push_str("\\n"),
                c if (c as u32) < 0x20 => o.push_str(&format!("\\u{:04x}", c as u32)),
                c => o.push(c),
            }
        }
        o.push('"');
        o
    }
    fn parent(path: &str) -> String {
        match path.rsplit_once('/') {
            Some(("", _)) | None => "/".into(),
            Some((p, _)) => p.into(),
        }
    }
    impl MemoryHost {
        pub fn with_file(mut self, path: &str, text: &str) -> Self {
            let p = self.resolve(path);
            self.files.insert(p, text.as_bytes().to_vec());
            self
        }
    }
    impl ScriptHost for MemoryHost {
        fn scheduler_seed(&mut self) -> u64 {
            // The toy world's seed decides the preemption quantum, as the
            // simulated machine's does.
            self.rng
        }
        fn read_file(&mut self, path: &str) -> Result<Vec<u8>, FsError> {
            let p = self.resolve(path);
            if self.dirs.contains(&p) {
                return Err(FsError::new(FsErrorKind::IsADirectory));
            }
            self.files
                .get(&p)
                .cloned()
                .ok_or(FsError::new(FsErrorKind::NotFound))
        }
        fn write_file(&mut self, path: &str, data: &[u8], append: bool) -> Result<(), FsError> {
            let p = self.resolve(path);
            if self.dirs.contains(&p) {
                return Err(FsError::new(FsErrorKind::IsADirectory));
            }
            if !self.dirs.contains(&parent(&p)) {
                return Err(FsError::new(FsErrorKind::NotFound));
            }
            let entry = self.files.entry(p).or_default();
            if !append {
                entry.clear();
            }
            entry.extend_from_slice(data);
            Ok(())
        }
        fn stat(&mut self, path: &str) -> Result<FileStat, FsError> {
            let p = self.resolve(path);
            if self.dirs.contains(&p) {
                return Ok(FileStat {
                    size: 4096,
                    is_dir: true,
                    mode: 0o755,
                    mtime_micros: self.now_micros,
                    links: 2,
                    owner: "user".into(),
                    ..FileStat::default()
                });
            }
            let f = self
                .files
                .get(&p)
                .ok_or(FsError::new(FsErrorKind::NotFound))?;
            Ok(FileStat {
                size: f.len() as u64,
                mode: 0o644,
                mtime_micros: self.now_micros,
                links: 1,
                owner: "user".into(),
                ..FileStat::default()
            })
        }
        fn list_dir(&mut self, path: &str) -> Result<Vec<String>, FsError> {
            let p = self.resolve(path);
            if !self.dirs.contains(&p) {
                return Err(FsError::new(if self.files.contains_key(&p) {
                    FsErrorKind::NotADirectory
                } else {
                    FsErrorKind::NotFound
                }));
            }
            let mut out: Vec<String> = self
                .files
                .keys()
                .chain(self.dirs.iter())
                .filter(|k| *k != "/" && parent(k) == p)
                .map(|k| k.rsplit('/').next().unwrap_or_default().to_string())
                .collect();
            out.sort();
            Ok(out)
        }
        fn mkdir(&mut self, path: &str, parents: bool) -> Result<(), FsError> {
            let p = self.resolve(path);
            if self.dirs.contains(&p) || self.files.contains_key(&p) {
                return Err(FsError::new(FsErrorKind::Exists));
            }
            if !self.dirs.contains(&parent(&p)) {
                if !parents {
                    return Err(FsError::new(FsErrorKind::NotFound));
                }
                self.mkdir(&parent(&p), true)?;
            }
            self.dirs.insert(p);
            Ok(())
        }
        fn remove(&mut self, path: &str, recursive: bool) -> Result<(), FsError> {
            let p = self.resolve(path);
            if self.files.remove(&p).is_some() {
                return Ok(());
            }
            if !self.dirs.contains(&p) {
                return Err(FsError::new(FsErrorKind::NotFound));
            }
            let prefix = format!("{p}/");
            let has_children = self.files.keys().any(|k| k.starts_with(&prefix))
                || self.dirs.iter().any(|k| k.starts_with(&prefix));
            if has_children && !recursive {
                return Err(FsError::new(FsErrorKind::NotEmpty));
            }
            self.files.retain(|k, _| !k.starts_with(&prefix));
            self.dirs.retain(|k| !k.starts_with(&prefix) && *k != p);
            Ok(())
        }
        fn rename(&mut self, from: &str, to: &str) -> Result<(), FsError> {
            let (f, t) = (self.resolve(from), self.resolve(to));
            let data = self
                .files
                .remove(&f)
                .ok_or(FsError::new(FsErrorKind::NotFound))?;
            self.files.insert(t, data);
            Ok(())
        }
        fn cwd(&self) -> String {
            self.cwd.clone()
        }
        fn chdir(&mut self, path: &str) -> Result<(), FsError> {
            let p = self.resolve(path);
            if !self.dirs.contains(&p) {
                return Err(FsError::new(FsErrorKind::NotFound));
            }
            self.cwd = p;
            Ok(())
        }
        fn resolve(&self, path: &str) -> String {
            let joined = if path.starts_with('/') {
                path.to_string()
            } else {
                format!("{}/{}", self.cwd, path)
            };
            let mut parts: Vec<&str> = vec![];
            for part in joined.split('/') {
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
        fn now_micros(&self) -> i64 {
            self.now_micros
        }
        fn random_u64(&mut self) -> u64 {
            // SplitMix64: deterministic, host-independent.
            self.rng = self.rng.wrapping_add(0x9e37_79b9_7f4a_7c15);
            let mut z = self.rng;
            z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
            z ^ (z >> 31)
        }
        fn user(&self) -> String {
            "user".into()
        }
        fn hostname(&self) -> String {
            "box".into()
        }
        fn pid(&self) -> u64 {
            100
        }
        fn local_address(&self) -> String {
            "192.0.2.2".into()
        }
        fn resolve_host(&mut self, name: &str) -> Result<Vec<String>, NetError> {
            let n = name.trim_end_matches('.').to_ascii_lowercase();
            if n == "localhost" {
                return Ok(vec!["127.0.0.1".into()]);
            }
            if n.split('.').count() == 4 && n.split('.').all(|p| p.parse::<u8>().is_ok()) {
                return Ok(vec![n]);
            }
            self.dns.get(&n).cloned().ok_or_else(|| {
                NetError::new(
                    NetErrorKind::NameNotFound,
                    format!("DNS resolution failed: {n}"),
                )
            })
        }
        fn tcp_connect(&mut self, host: &str, port: u16) -> Result<TcpConnection, NetError> {
            let addr = self.resolve_host(host)?;
            if port != 80 {
                return Err(NetError::new(
                    NetErrorKind::Refused,
                    format!("connection refused: {}:{port}", addr[0]),
                ));
            }
            Ok(TcpConnection {
                remote_address: addr[0].clone(),
                remote_port: port,
                local_address: self.local_address(),
                local_port: 40000 + self.requests.len() as u16,
                elapsed_micros: 1000,
            })
        }
        fn http(&mut self, request: &HttpRequest) -> Result<HttpResponse, NetError> {
            self.requests.push(request.clone());
            let (host, port, path) = split_url(&request.url)
                .ok_or_else(|| NetError::new(NetErrorKind::Invalid, "invalid URL"))?;
            self.tcp_connect(&host, port)?;
            let elapsed_micros = 2000;
            if host == "echo.test" {
                let headers: Vec<String> = request
                    .headers
                    .iter()
                    .map(|(k, v)| format!("[{},{}]", json_str(k), json_str(v)))
                    .collect();
                let body = format!(
                    "{{\"method\":{},\"path\":{},\"headers\":[{}],\"body\":{}}}",
                    json_str(&request.method),
                    json_str(&path),
                    headers.join(","),
                    json_str(&String::from_utf8_lossy(&request.body))
                );
                return Ok(HttpResponse {
                    status: 200,
                    headers: vec![
                        ("content-type".into(), "application/json".into()),
                        ("content-length".into(), body.len().to_string()),
                    ],
                    body: body.into_bytes(),
                    elapsed_micros,
                });
            }
            let (status, body) = match self.pages.get(&path) {
                Some(b) => (200, b.clone()),
                None => (404, b"not found".to_vec()),
            };
            Ok(HttpResponse {
                status,
                headers: vec![
                    ("content-type".into(), "text/plain; charset=utf-8".into()),
                    ("content-length".into(), body.len().to_string()),
                ],
                body,
                elapsed_micros,
            })
        }
    }
}
