//! The only window an embedded language runtime has onto the world.
//!
//! `python3` and `node` run inside the simulation, so they must never touch the
//! host: files come from the machine's VFS, time from the world clock, and entropy
//! from the world's seeded determinism. Each interpreter crate is written against
//! this trait; `cw-computer` implements it once over a `Computer`.

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
}

/// What a runtime is asked to do: the arguments after the interpreter name exactly as
/// the command line gave them (`["-c", "print(1)"]`, `["main.py", "a"]`), its
/// environment, and the text waiting on standard input.
#[derive(Debug, Clone, Default)]
pub struct Invocation {
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
    pub stdin: String,
}

/// A finished run: both streams and the process exit status.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Outcome {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
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
            }
        }
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
    }
}
