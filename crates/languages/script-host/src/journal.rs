//! Record and replay of a run's host calls.
//!
//! An interpreter cannot be kept alive between two actions of the world (its heap
//! is not serializable, and a snapshot may be restored anywhere), yet an interactive
//! program or a paused debuggee must continue exactly where it stopped. Runs are
//! therefore resumed by *replay*: the interpreter is deterministic, so rerunning the
//! same program on the same input makes the same host calls in the same order. The
//! first run records every result the host gave in a [`Journal`]; the rerun is
//! answered from it — a file written once is not written again, a request is not
//! sent twice, the clock reads what it read — and only calls past the recorded end
//! reach the real host (and are recorded in turn).
//!
//! Calls whose answer depends only on process-local state the replay re-creates
//! (`cwd`, `resolve`, identity strings) are not journaled; `chdir` is both
//! journaled and re-applied, since it is that process-local state.
use crate::*;
use std::cell::{Cell, RefCell};

/// The recorded results, one entry per host call, in call order.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Journal {
    pub entries: Vec<Vec<u8>>,
}

impl Journal {
    /// One flat byte string (length-prefixed entries).
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut w = Writer::default();
        w.u64(self.entries.len() as u64);
        for e in &self.entries {
            w.bytes(e);
        }
        w.0
    }
    pub fn from_bytes(b: &[u8]) -> Option<Self> {
        let mut r = Reader { b, i: 0 };
        let n = r.u64()? as usize;
        let mut entries = Vec::with_capacity(n.min(1 << 16));
        for _ in 0..n {
            entries.push(r.bytes()?);
        }
        Some(Self { entries })
    }
    /// Lowercase hex, for embedding in JSON state.
    pub fn to_hex(&self) -> String {
        self.to_bytes().iter().map(|b| format!("{b:02x}")).collect()
    }
    pub fn from_hex(s: &str) -> Option<Self> {
        if !s.len().is_multiple_of(2) {
            return None;
        }
        let bytes: Option<Vec<u8>> = (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(s.get(i..i + 2)?, 16).ok())
            .collect();
        Self::from_bytes(&bytes?)
    }
}

#[derive(Default)]
struct Writer(Vec<u8>);
impl Writer {
    fn u8(&mut self, v: u8) {
        self.0.push(v);
    }
    fn u64(&mut self, mut v: u64) {
        loop {
            let byte = (v & 0x7f) as u8;
            v >>= 7;
            if v == 0 {
                self.0.push(byte);
                return;
            }
            self.0.push(byte | 0x80);
        }
    }
    fn i64(&mut self, v: i64) {
        self.u64(((v << 1) ^ (v >> 63)) as u64);
    }
    fn bytes(&mut self, b: &[u8]) {
        self.u64(b.len() as u64);
        self.0.extend_from_slice(b);
    }
    fn str(&mut self, s: &str) {
        self.bytes(s.as_bytes());
    }
    fn pairs(&mut self, p: &[(String, String)]) {
        self.u64(p.len() as u64);
        for (k, v) in p {
            self.str(k);
            self.str(v);
        }
    }
    fn fs_err(&mut self, e: &FsError) {
        self.u8(e.kind as u8);
    }
    fn net_err(&mut self, e: &NetError) {
        self.u8(e.kind.index());
        self.str(&e.message);
    }
}
struct Reader<'a> {
    b: &'a [u8],
    i: usize,
}
impl Reader<'_> {
    fn u8(&mut self) -> Option<u8> {
        let v = *self.b.get(self.i)?;
        self.i += 1;
        Some(v)
    }
    fn u64(&mut self) -> Option<u64> {
        let mut v = 0u64;
        let mut shift = 0;
        loop {
            let byte = self.u8()?;
            v |= u64::from(byte & 0x7f) << shift;
            if byte & 0x80 == 0 {
                return Some(v);
            }
            shift += 7;
            if shift > 63 {
                return None;
            }
        }
    }
    fn i64(&mut self) -> Option<i64> {
        let u = self.u64()?;
        Some(((u >> 1) as i64) ^ -((u & 1) as i64))
    }
    fn bytes(&mut self) -> Option<Vec<u8>> {
        let n = self.u64()? as usize;
        let v = self.b.get(self.i..self.i.checked_add(n)?)?.to_vec();
        self.i += n;
        Some(v)
    }
    fn str(&mut self) -> Option<String> {
        String::from_utf8(self.bytes()?).ok()
    }
    fn pairs(&mut self) -> Option<Vec<(String, String)>> {
        let n = self.u64()? as usize;
        let mut v = Vec::with_capacity(n.min(1024));
        for _ in 0..n {
            v.push((self.str()?, self.str()?));
        }
        Some(v)
    }
    fn fs_err(&mut self) -> Option<FsError> {
        let kind = match self.u8()? {
            0 => FsErrorKind::NotFound,
            1 => FsErrorKind::Exists,
            2 => FsErrorKind::NotADirectory,
            3 => FsErrorKind::IsADirectory,
            4 => FsErrorKind::NotEmpty,
            5 => FsErrorKind::PermissionDenied,
            _ => FsErrorKind::Invalid,
        };
        Some(FsError::new(kind))
    }
    fn net_err(&mut self) -> Option<NetError> {
        let kind = NetErrorKind::from_index(self.u8()?);
        Some(NetError::new(kind, self.str()?))
    }
}

/// Each journaled call starts with its method tag so a replay that drifts (a host
/// that changed under it) is detected instead of answered with the wrong type.
#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
enum Tag {
    ReadFile = 1,
    WriteFile,
    Stat,
    Lstat,
    ListDir,
    Mkdir,
    Remove,
    Rename,
    Chdir,
    Now,
    Random,
    Pid,
    Http,
    Resolve,
    Connect,
    Spawn,
    Seed,
}

fn unit(w: &mut Writer, r: &Result<(), FsError>) {
    match r {
        Ok(()) => w.u8(0),
        Err(e) => {
            w.u8(1);
            w.fs_err(e);
        }
    }
}
fn read_unit(r: &mut Reader) -> Option<Result<(), FsError>> {
    Some(match r.u8()? {
        0 => Ok(()),
        _ => Err(r.fs_err()?),
    })
}
fn stat_w(w: &mut Writer, r: &Result<FileStat, FsError>) {
    match r {
        Ok(s) => {
            w.u8(0);
            w.u64(s.size);
            w.u8(u8::from(s.is_dir) | (u8::from(s.is_symlink) << 1));
            w.u64(u64::from(s.mode));
            w.i64(s.mtime_micros);
            w.u64(s.inode);
            w.u64(u64::from(s.links));
            w.str(&s.owner);
        }
        Err(e) => {
            w.u8(1);
            w.fs_err(e);
        }
    }
}
fn stat_r(r: &mut Reader) -> Option<Result<FileStat, FsError>> {
    Some(match r.u8()? {
        0 => {
            let size = r.u64()?;
            let flags = r.u8()?;
            Ok(FileStat {
                size,
                is_dir: flags & 1 != 0,
                is_symlink: flags & 2 != 0,
                mode: r.u64()? as u32,
                mtime_micros: r.i64()?,
                inode: r.u64()?,
                links: r.u64()? as u32,
                owner: r.str()?,
            })
        }
        _ => Err(r.fs_err()?),
    })
}

/// A host that answers from a journal, then records. Wraps the real host for one run.
pub struct JournalHost<'a> {
    inner: &'a mut dyn ScriptHost,
    replay: Vec<Vec<u8>>,
    pos: Cell<usize>,
    record: RefCell<Vec<Vec<u8>>>,
    diverged: Cell<bool>,
}

impl<'a> JournalHost<'a> {
    pub fn new(inner: &'a mut dyn ScriptHost, replay: Journal) -> Self {
        Self {
            inner,
            replay: replay.entries,
            pos: Cell::new(0),
            record: RefCell::new(vec![]),
            diverged: Cell::new(false),
        }
    }
    /// The complete journal after the run: the replayed prefix plus what was new.
    pub fn into_journal(self) -> Journal {
        let mut entries = self.replay;
        let used = self.pos.get();
        entries.truncate(used);
        entries.extend(self.record.into_inner());
        Journal { entries }
    }
    /// Whether the run asked for something the journal did not record at that point
    /// (the world changed under a suspended program); from there on it ran live.
    pub fn diverged(&self) -> bool {
        self.diverged.get()
    }
    /// Calls still to be replayed.
    pub fn replaying(&self) -> bool {
        !self.diverged.get() && self.pos.get() < self.replay.len()
    }
    /// The next recorded entry if it answers `tag`.
    fn next(&self, tag: Tag) -> Option<Vec<u8>> {
        if self.diverged.get() {
            return None;
        }
        let i = self.pos.get();
        let e = self.replay.get(i)?;
        if e.first() != Some(&(tag as u8)) {
            self.diverged.set(true);
            return None;
        }
        self.pos.set(i + 1);
        Some(e[1..].to_vec())
    }
    fn push(&self, tag: Tag, w: Writer) {
        let mut e = vec![tag as u8];
        e.extend(w.0);
        if self.pos.get() < self.replay.len() {
            // Past a divergence the rest of the old journal is stale.
            self.diverged.set(true);
        }
        self.record.borrow_mut().push(e);
    }
    fn replayed<T>(&self, tag: Tag, f: impl FnOnce(&mut Reader) -> Option<T>) -> Option<T> {
        let e = self.next(tag)?;
        let mut r = Reader { b: &e, i: 0 };
        let v = f(&mut r);
        if v.is_none() {
            self.diverged.set(true);
        }
        v
    }
}

impl ScriptHost for JournalHost<'_> {
    fn read_file(&mut self, path: &str) -> Result<Vec<u8>, FsError> {
        if let Some(v) = self.replayed(Tag::ReadFile, |r| {
            Some(match r.u8()? {
                0 => Ok(r.bytes()?),
                _ => Err(r.fs_err()?),
            })
        }) {
            return v;
        }
        let v = self.inner.read_file(path);
        let mut w = Writer::default();
        match &v {
            Ok(b) => {
                w.u8(0);
                w.bytes(b);
            }
            Err(e) => {
                w.u8(1);
                w.fs_err(e);
            }
        }
        self.push(Tag::ReadFile, w);
        v
    }
    fn write_file(&mut self, path: &str, data: &[u8], append: bool) -> Result<(), FsError> {
        if let Some(v) = self.replayed(Tag::WriteFile, read_unit) {
            return v;
        }
        let v = self.inner.write_file(path, data, append);
        let mut w = Writer::default();
        unit(&mut w, &v);
        self.push(Tag::WriteFile, w);
        v
    }
    fn stat(&mut self, path: &str) -> Result<FileStat, FsError> {
        if let Some(v) = self.replayed(Tag::Stat, stat_r) {
            return v;
        }
        let v = self.inner.stat(path);
        let mut w = Writer::default();
        stat_w(&mut w, &v);
        self.push(Tag::Stat, w);
        v
    }
    fn lstat(&mut self, path: &str) -> Result<FileStat, FsError> {
        if let Some(v) = self.replayed(Tag::Lstat, stat_r) {
            return v;
        }
        let v = self.inner.lstat(path);
        let mut w = Writer::default();
        stat_w(&mut w, &v);
        self.push(Tag::Lstat, w);
        v
    }
    fn list_dir(&mut self, path: &str) -> Result<Vec<String>, FsError> {
        if let Some(v) = self.replayed(Tag::ListDir, |r| {
            Some(match r.u8()? {
                0 => {
                    let n = r.u64()? as usize;
                    let mut out = Vec::with_capacity(n.min(4096));
                    for _ in 0..n {
                        out.push(r.str()?);
                    }
                    Ok(out)
                }
                _ => Err(r.fs_err()?),
            })
        }) {
            return v;
        }
        let v = self.inner.list_dir(path);
        let mut w = Writer::default();
        match &v {
            Ok(names) => {
                w.u8(0);
                w.u64(names.len() as u64);
                for n in names {
                    w.str(n);
                }
            }
            Err(e) => {
                w.u8(1);
                w.fs_err(e);
            }
        }
        self.push(Tag::ListDir, w);
        v
    }
    fn mkdir(&mut self, path: &str, parents: bool) -> Result<(), FsError> {
        if let Some(v) = self.replayed(Tag::Mkdir, read_unit) {
            return v;
        }
        let v = self.inner.mkdir(path, parents);
        let mut w = Writer::default();
        unit(&mut w, &v);
        self.push(Tag::Mkdir, w);
        v
    }
    fn remove(&mut self, path: &str, recursive: bool) -> Result<(), FsError> {
        if let Some(v) = self.replayed(Tag::Remove, read_unit) {
            return v;
        }
        let v = self.inner.remove(path, recursive);
        let mut w = Writer::default();
        unit(&mut w, &v);
        self.push(Tag::Remove, w);
        v
    }
    fn rename(&mut self, from: &str, to: &str) -> Result<(), FsError> {
        if let Some(v) = self.replayed(Tag::Rename, read_unit) {
            return v;
        }
        let v = self.inner.rename(from, to);
        let mut w = Writer::default();
        unit(&mut w, &v);
        self.push(Tag::Rename, w);
        v
    }
    fn cwd(&self) -> String {
        self.inner.cwd()
    }
    fn chdir(&mut self, path: &str) -> Result<(), FsError> {
        if let Some(v) = self.replayed(Tag::Chdir, read_unit) {
            // The working directory is the process's own: re-create it.
            if v.is_ok() {
                let _ = self.inner.chdir(path);
            }
            return v;
        }
        let v = self.inner.chdir(path);
        let mut w = Writer::default();
        unit(&mut w, &v);
        self.push(Tag::Chdir, w);
        v
    }
    fn resolve(&self, path: &str) -> String {
        self.inner.resolve(path)
    }
    fn now_micros(&self) -> i64 {
        if let Some(v) = self.replayed(Tag::Now, |r| r.i64()) {
            return v;
        }
        let v = self.inner.now_micros();
        let mut w = Writer::default();
        w.i64(v);
        self.push(Tag::Now, w);
        v
    }
    fn random_u64(&mut self) -> u64 {
        if let Some(v) = self.replayed(Tag::Random, |r| r.u64()) {
            return v;
        }
        let v = self.inner.random_u64();
        let mut w = Writer::default();
        w.u64(v);
        self.push(Tag::Random, w);
        v
    }
    fn user(&self) -> String {
        self.inner.user()
    }
    fn hostname(&self) -> String {
        self.inner.hostname()
    }
    fn pid(&self) -> u64 {
        if let Some(v) = self.replayed(Tag::Pid, |r| r.u64()) {
            return v;
        }
        let v = self.inner.pid();
        let mut w = Writer::default();
        w.u64(v);
        self.push(Tag::Pid, w);
        v
    }
    fn os_family(&self) -> String {
        self.inner.os_family()
    }
    fn http(&mut self, request: &HttpRequest) -> Result<HttpResponse, NetError> {
        if let Some(v) = self.replayed(Tag::Http, |r| {
            Some(match r.u8()? {
                0 => Ok(HttpResponse {
                    status: r.u64()? as u16,
                    headers: r.pairs()?,
                    body: r.bytes()?,
                    elapsed_micros: r.u64()?,
                }),
                _ => Err(r.net_err()?),
            })
        }) {
            return v;
        }
        let v = self.inner.http(request);
        let mut w = Writer::default();
        match &v {
            Ok(resp) => {
                w.u8(0);
                w.u64(u64::from(resp.status));
                w.pairs(&resp.headers);
                w.bytes(&resp.body);
                w.u64(resp.elapsed_micros);
            }
            Err(e) => {
                w.u8(1);
                w.net_err(e);
            }
        }
        self.push(Tag::Http, w);
        v
    }
    fn resolve_host(&mut self, name: &str) -> Result<Vec<String>, NetError> {
        if let Some(v) = self.replayed(Tag::Resolve, |r| {
            Some(match r.u8()? {
                0 => {
                    let n = r.u64()? as usize;
                    let mut out = vec![];
                    for _ in 0..n.min(256) {
                        out.push(r.str()?);
                    }
                    Ok(out)
                }
                _ => Err(r.net_err()?),
            })
        }) {
            return v;
        }
        let v = self.inner.resolve_host(name);
        let mut w = Writer::default();
        match &v {
            Ok(a) => {
                w.u8(0);
                w.u64(a.len() as u64);
                for s in a {
                    w.str(s);
                }
            }
            Err(e) => {
                w.u8(1);
                w.net_err(e);
            }
        }
        self.push(Tag::Resolve, w);
        v
    }
    fn tcp_connect(&mut self, host: &str, port: u16) -> Result<TcpConnection, NetError> {
        if let Some(v) = self.replayed(Tag::Connect, |r| {
            Some(match r.u8()? {
                0 => Ok(TcpConnection {
                    remote_address: r.str()?,
                    remote_port: r.u64()? as u16,
                    local_address: r.str()?,
                    local_port: r.u64()? as u16,
                    elapsed_micros: r.u64()?,
                }),
                _ => Err(r.net_err()?),
            })
        }) {
            return v;
        }
        let v = self.inner.tcp_connect(host, port);
        let mut w = Writer::default();
        match &v {
            Ok(c) => {
                w.u8(0);
                w.str(&c.remote_address);
                w.u64(u64::from(c.remote_port));
                w.str(&c.local_address);
                w.u64(u64::from(c.local_port));
                w.u64(c.elapsed_micros);
            }
            Err(e) => {
                w.u8(1);
                w.net_err(e);
            }
        }
        self.push(Tag::Connect, w);
        v
    }
    fn local_address(&self) -> String {
        self.inner.local_address()
    }
    fn spawn(&mut self, request: &SpawnRequest) -> Result<Outcome, FsError> {
        if let Some(v) = self.replayed(Tag::Spawn, |r| {
            Some(match r.u8()? {
                0 => Ok(Outcome {
                    stdout: r.str()?,
                    stderr: r.str()?,
                    exit_code: r.i64()? as i32,
                    awaiting_input: false,
                    elapsed_micros: r.u64()?,
                }),
                _ => Err(r.fs_err()?),
            })
        }) {
            return v;
        }
        let v = self.inner.spawn(request);
        let mut w = Writer::default();
        match &v {
            Ok(o) => {
                w.u8(0);
                w.str(&o.stdout);
                w.str(&o.stderr);
                w.i64(i64::from(o.exit_code));
                w.u64(o.elapsed_micros);
            }
            Err(e) => {
                w.u8(1);
                w.fs_err(e);
            }
        }
        self.push(Tag::Spawn, w);
        v
    }
    fn scheduler_seed(&mut self) -> u64 {
        if let Some(v) = self.replayed(Tag::Seed, |r| r.u64()) {
            return v;
        }
        let v = self.inner.scheduler_seed();
        let mut w = Writer::default();
        w.u64(v);
        self.push(Tag::Seed, w);
        v
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::MemoryHost;

    #[test]
    fn replay_answers_without_repeating_side_effects() {
        let mut host = MemoryHost::default();
        let journal = {
            let mut j = JournalHost::new(&mut host, Journal::default());
            j.write_file("/tmp/a", b"one", true).unwrap();
            assert_eq!(j.read_file("/tmp/a").unwrap(), b"one");
            let _ = j.now_micros();
            j.into_journal()
        };
        assert_eq!(journal.entries.len(), 3);
        let restored = Journal::from_hex(&journal.to_hex()).unwrap();
        assert_eq!(restored, journal);
        host.now_micros += 5;
        let mut j = JournalHost::new(&mut host, restored);
        j.write_file("/tmp/a", b"one", true).unwrap();
        assert_eq!(j.read_file("/tmp/a").unwrap(), b"one");
        assert_eq!(j.now_micros(), 1_789_635_600_000_000);
        // Past the journal: live again.
        j.write_file("/tmp/a", b"two", true).unwrap();
        assert!(!j.diverged());
        let journal = j.into_journal();
        assert_eq!(journal.entries.len(), 4);
        assert_eq!(host.files["/tmp/a"], b"onetwo");
    }

    #[test]
    fn drift_is_detected_and_runs_live() {
        let mut host = MemoryHost::default();
        let journal = {
            let mut j = JournalHost::new(&mut host, Journal::default());
            let _ = j.random_u64();
            j.into_journal()
        };
        let mut j = JournalHost::new(&mut host, journal);
        let _ = j.read_file("/nope");
        assert!(j.diverged());
    }
}
