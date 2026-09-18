//! Language runtimes (`python3`, `node`) wired to a computer: the interpreter
//! sees this machine's VFS, the world clock and the world's entropy, and nothing
//! of the host.
use crate::{normalize_path, CommandResult, Computer, NetFailure, ShellHost, VfsError};
use cw_script_host::{
    FileStat, FsError, FsErrorKind, HttpRequest, HttpResponse, Invocation, NetError, NetErrorKind,
    Outcome, ScriptHost, SpawnProgram, SpawnRequest, TcpConnection,
};

/// The script host for one interpreter run. The working directory is the
/// process's own: a program's `os.chdir` does not move the shell.
pub struct MachineHost<'a> {
    pub computer: &'a mut Computer,
    pub shell: &'a mut dyn ShellHost,
    pub cwd: String,
    pub tick: u64,
    pub pid: u64,
    /// Shell nesting depth of the command that started the interpreter; child
    /// processes run one level deeper, so runaway recursion hits the shell's cap.
    pub depth: usize,
    seed: Option<u64>,
    next_port: u16,
}

/// The world's error code for a failed exchange, in the runtimes' vocabulary.
fn net_error(f: NetFailure) -> NetError {
    let kind = match f.code.as_str() {
        "dns" => NetErrorKind::NameNotFound,
        "connection_refused" | "not_found" => NetErrorKind::Refused,
        "unreachable" => NetErrorKind::Unreachable,
        "network_denied" | "denied" => NetErrorKind::Denied,
        "packet_loss" | "would_block" => NetErrorKind::Reset,
        "timeout" => NetErrorKind::TimedOut,
        "invalid" => NetErrorKind::Invalid,
        "unavailable" => NetErrorKind::Unavailable,
        _ if f.message.contains("unavailable") => NetErrorKind::Unavailable,
        _ => NetErrorKind::Reset,
    };
    NetError::new(kind, f.message)
}

fn fs_error(e: VfsError) -> FsError {
    FsError::new(match e {
        VfsError::NotFound(_) => FsErrorKind::NotFound,
        VfsError::Exists(_) => FsErrorKind::Exists,
        VfsError::NotDirectory(_) => FsErrorKind::NotADirectory,
        VfsError::IsDirectory(_) => FsErrorKind::IsADirectory,
        VfsError::NotEmpty(_) => FsErrorKind::NotEmpty,
        VfsError::Permission(_) => FsErrorKind::PermissionDenied,
        VfsError::LinkLoop | VfsError::Invalid(_) => FsErrorKind::Invalid,
    })
}

impl<'a> MachineHost<'a> {
    pub fn new(computer: &'a mut Computer, shell: &'a mut dyn ShellHost, tick: u64) -> Self {
        let cwd = computer.cwd.clone();
        let pid = computer
            .processes
            .list()
            .iter()
            .map(|p| p.pid)
            .max()
            .unwrap_or(1);
        Self {
            computer,
            shell,
            cwd,
            tick,
            pid,
            depth: 0,
            seed: None,
            next_port: 0,
        }
    }
    /// Elapsed world time across `f`, when the host can tell.
    fn timed<T>(&mut self, f: impl FnOnce(&mut Self) -> T) -> (T, u64) {
        let before = self.shell.now_tick();
        let v = f(self);
        let after = self.shell.now_tick();
        let elapsed = match (before, after) {
            (Some(a), Some(b)) => b.saturating_sub(a),
            _ => 0,
        };
        (v, elapsed)
    }
    /// An ephemeral port, deterministic per run (Linux's range starts at 32768).
    fn ephemeral_port(&mut self) -> u16 {
        let base = 32768 + (self.pid % 20000) as u16;
        self.next_port = self.next_port.wrapping_add(1);
        base.wrapping_add(self.next_port)
    }
    fn stat_of(&self, m: crate::Metadata) -> FileStat {
        FileStat {
            size: m.size as u64,
            is_dir: m.is_dir,
            is_symlink: m.is_symlink,
            mode: (m.mode & 0o7777) as u32,
            mtime_micros: crate::shell::EPOCH_UNIX_SECONDS as i64 * 1_000_000 + m.modified as i64,
            inode: m.inode,
            links: m.links,
            owner: m.owner,
        }
    }
}

impl ScriptHost for MachineHost<'_> {
    fn read_file(&mut self, path: &str) -> Result<Vec<u8>, FsError> {
        let p = self.resolve(path);
        let c = &*self.computer;
        if c.vfs.stat(&p).map_err(fs_error)?.is_dir {
            return Err(FsError::new(FsErrorKind::IsADirectory));
        }
        c.vfs.read_as(&p, &c.user).map_err(fs_error)
    }
    fn write_file(&mut self, path: &str, data: &[u8], append: bool) -> Result<(), FsError> {
        let p = self.resolve(path);
        let c = &mut *self.computer;
        if c.vfs.stat(&p).is_ok_and(|m| m.is_dir) {
            return Err(FsError::new(FsErrorKind::IsADirectory));
        }
        let user = c.user.clone();
        if append {
            c.vfs
                .append_as(&p, data, &user, self.tick)
                .map_err(fs_error)
        } else {
            c.vfs.write_as(&p, data, &user, self.tick).map_err(fs_error)
        }
    }
    fn stat(&mut self, path: &str) -> Result<FileStat, FsError> {
        let p = self.resolve(path);
        let m = self.computer.vfs.stat(&p).map_err(fs_error)?;
        Ok(self.stat_of(m))
    }
    fn lstat(&mut self, path: &str) -> Result<FileStat, FsError> {
        let p = self.resolve(path);
        let m = self.computer.vfs.lstat(&p).map_err(fs_error)?;
        Ok(self.stat_of(m))
    }
    fn list_dir(&mut self, path: &str) -> Result<Vec<String>, FsError> {
        let p = self.resolve(path);
        let c = &*self.computer;
        if !c.vfs.stat(&p).map_err(fs_error)?.is_dir {
            return Err(FsError::new(FsErrorKind::NotADirectory));
        }
        let mut names = c.vfs.list_as(&p, &c.user).map_err(fs_error)?;
        names.sort();
        Ok(names)
    }
    fn mkdir(&mut self, path: &str, parents: bool) -> Result<(), FsError> {
        let p = self.resolve(path);
        let c = &mut *self.computer;
        if c.vfs.exists(&p) {
            return Err(FsError::new(FsErrorKind::Exists));
        }
        if !parents {
            let parent = p.rsplit_once('/').map(|(a, _)| a).unwrap_or("/");
            let parent = if parent.is_empty() { "/" } else { parent };
            match c.vfs.stat(parent) {
                Ok(m) if m.is_dir => {}
                Ok(_) => return Err(FsError::new(FsErrorKind::NotADirectory)),
                Err(_) => return Err(FsError::new(FsErrorKind::NotFound)),
            }
        }
        let user = c.user.clone();
        c.vfs.mkdir_all_as(&p, &user, self.tick).map_err(fs_error)
    }
    fn remove(&mut self, path: &str, recursive: bool) -> Result<(), FsError> {
        let p = self.resolve(path);
        let c = &mut *self.computer;
        let user = c.user.clone();
        c.vfs.remove_as(&p, recursive, &user).map_err(fs_error)
    }
    fn rename(&mut self, from: &str, to: &str) -> Result<(), FsError> {
        let (f, t) = (self.resolve(from), self.resolve(to));
        let c = &mut *self.computer;
        let user = c.user.clone();
        c.vfs.rename_as(&f, &t, &user).map_err(fs_error)
    }
    fn cwd(&self) -> String {
        self.cwd.clone()
    }
    fn chdir(&mut self, path: &str) -> Result<(), FsError> {
        let p = self.resolve(path);
        let c = &*self.computer;
        if !c.vfs.stat(&p).map_err(fs_error)?.is_dir {
            return Err(FsError::new(FsErrorKind::NotADirectory));
        }
        c.vfs.list_as(&p, &c.user).map_err(fs_error)?;
        self.cwd = p;
        Ok(())
    }
    fn resolve(&self, path: &str) -> String {
        normalize_path(&self.cwd, path)
    }
    fn now_micros(&self) -> i64 {
        crate::shell::EPOCH_UNIX_SECONDS as i64 * 1_000_000 + self.tick as i64
    }
    fn random_u64(&mut self) -> u64 {
        // Drawn from the world only when a program actually wants randomness.
        let state = match self.seed {
            Some(s) => s,
            None => self.shell.entropy(),
        };
        let next = state.wrapping_add(0x9e37_79b9_7f4a_7c15);
        self.seed = Some(next);
        let mut z = next;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }
    fn user(&self) -> String {
        self.computer.user.clone()
    }
    fn hostname(&self) -> String {
        self.computer.id.clone()
    }
    fn pid(&self) -> u64 {
        self.pid
    }
    fn os_family(&self) -> String {
        self.computer.os_family.clone()
    }
    fn local_address(&self) -> String {
        self.computer.hardware.ipv4.clone()
    }
    fn http(&mut self, request: &HttpRequest) -> Result<HttpResponse, NetError> {
        let mut headers = std::collections::BTreeMap::<String, String>::new();
        for (k, v) in &request.headers {
            let key = k.to_ascii_lowercase();
            match headers.get_mut(&key) {
                Some(existing) => {
                    existing.push_str(", ");
                    existing.push_str(v);
                }
                None => {
                    headers.insert(key, v.clone());
                }
            }
        }
        let wire = cw_protocol::HttpRequest {
            method: request.method.clone(),
            url: request.url.clone(),
            headers,
            body: request.body.clone(),
        };
        let (result, elapsed) = self.timed(|h| h.shell.http_exchange(wire));
        let response = result.map_err(net_error)?;
        if request.timeout_micros.is_some_and(|t| elapsed > t) {
            return Err(NetError::new(
                NetErrorKind::TimedOut,
                format!("{} timed out", request.url),
            ));
        }
        Ok(HttpResponse {
            status: response.status,
            headers: response.headers.into_iter().collect(),
            body: response.body,
            elapsed_micros: elapsed,
        })
    }
    fn resolve_host(&mut self, name: &str) -> Result<Vec<String>, NetError> {
        self.shell.resolve_name(name).map_err(net_error)
    }
    fn tcp_connect(&mut self, host: &str, port: u16) -> Result<TcpConnection, NetError> {
        let (result, elapsed) = self.timed(|h| h.shell.probe_tcp(host, port));
        let remote_address = result.map_err(net_error)?;
        let local_address = if remote_address.starts_with("127.") {
            "127.0.0.1".to_string()
        } else {
            self.computer.hardware.ipv4.clone()
        };
        Ok(TcpConnection {
            remote_address,
            remote_port: port,
            local_address,
            local_port: self.ephemeral_port(),
            elapsed_micros: elapsed,
        })
    }
    fn spawn(&mut self, request: &SpawnRequest) -> Result<Outcome, FsError> {
        let line = match &request.program {
            SpawnProgram::Shell(line) => line.clone(),
            SpawnProgram::Argv(argv) => {
                let Some(program) = argv.first() else {
                    return Err(FsError::new(FsErrorKind::NotFound));
                };
                // The lookup sees the child's own PATH and working directory.
                let saved = (self.computer.cwd.clone(), self.computer.env.clone());
                self.computer.cwd = match &request.cwd {
                    Some(d) => normalize_path(&self.cwd, d),
                    None => self.cwd.clone(),
                };
                if let Some(env) = &request.env {
                    if let Some((_, path)) = env.iter().find(|(k, _)| k == "PATH") {
                        self.computer.env.insert("PATH".into(), path.clone());
                    }
                }
                let exists = crate::shell::command_exists(self.computer, program);
                (self.computer.cwd, self.computer.env) = saved;
                if !exists {
                    return Err(FsError::new(FsErrorKind::NotFound));
                }
                argv.iter()
                    .map(|a| cw_script_host::shell_quote(a))
                    .collect::<Vec<_>>()
                    .join(" ")
            }
        };
        let cwd = match &request.cwd {
            Some(d) => {
                let p = normalize_path(&self.cwd, d);
                match self.computer.vfs.stat(&p) {
                    Ok(m) if m.is_dir => p,
                    Ok(_) => return Err(FsError::new(FsErrorKind::NotADirectory)),
                    Err(_) => return Err(FsError::new(FsErrorKind::NotFound)),
                }
            }
            None => self.cwd.clone(),
        };
        let saved_cwd = std::mem::replace(&mut self.computer.cwd, cwd);
        let saved_env = match &request.env {
            Some(env) => Some(std::mem::replace(
                &mut self.computer.env,
                env.iter().cloned().collect(),
            )),
            None => None,
        };
        let before = self.computer.runtime_elapsed_micros;
        let r = crate::shell::execute_child(
            self.computer,
            &line,
            &request.stdin,
            self.tick,
            self.shell,
            self.depth,
        );
        self.computer.cwd = saved_cwd;
        if let Some(env) = saved_env {
            self.computer.env = env;
        }
        let elapsed = self.computer.runtime_elapsed_micros.saturating_sub(before);
        // The parent's own run accounts for this time when it reports its outcome.
        self.computer.runtime_elapsed_micros = before;
        Ok(Outcome {
            elapsed_micros: elapsed,
            ..Outcome::new(r.stdout, r.stderr, r.exit_code)
        })
    }
    fn scheduler_seed(&mut self) -> u64 {
        self.shell.entropy()
    }
}

/// Which interpreter a command name (or a shebang) selects.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Runtime {
    Python,
    Node,
}

pub fn runtime_for(name: &str) -> Option<Runtime> {
    let base = name.rsplit('/').next().unwrap_or(name);
    let dir = &name[..name.len() - base.len()];
    if !dir.is_empty() && !matches!(dir, "/usr/bin/" | "/bin/" | "/usr/local/bin/") {
        return None;
    }
    match base {
        "python3" | "python" | "python3.12" => Some(Runtime::Python),
        "node" | "nodejs" => Some(Runtime::Node),
        _ => None,
    }
}

/// `#!/usr/bin/env python3`, `#!/usr/bin/python3`, `#!/usr/bin/env node`…
pub fn shebang_runtime(source: &str) -> Option<Runtime> {
    let first = source.lines().next()?.strip_prefix("#!")?.trim();
    let mut parts = first.split_whitespace();
    let prog = parts.next()?;
    if prog.ends_with("/env") {
        let mut next = parts.next()?;
        if next == "-S" {
            next = parts.next()?;
        }
        return runtime_for(next.rsplit('/').next().unwrap_or(next));
    }
    runtime_for(prog)
}

pub fn run_runtime(
    runtime: Runtime,
    computer: &mut Computer,
    shell: &mut dyn ShellHost,
    args: &[String],
    stdin: &str,
    tick: u64,
    depth: usize,
) -> CommandResult {
    let env: Vec<(String, String)> = computer
        .env
        .iter()
        .filter(|(k, _)| {
            k.chars()
                .next()
                .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        })
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    let invocation = Invocation {
        args: args.to_vec(),
        env,
        stdin: stdin.to_string(),
        ..Invocation::default()
    };
    let mut host = MachineHost::new(computer, shell, tick);
    host.depth = depth;
    let out: Outcome = match runtime {
        Runtime::Python => cw_pyvm::run(&mut host, &invocation),
        Runtime::Node => cw_jsvm::run(&mut host, &invocation),
    };
    computer.runtime_elapsed_micros = computer
        .runtime_elapsed_micros
        .saturating_add(out.elapsed_micros);
    CommandResult {
        stdout: out.stdout,
        stderr: out.stderr,
        exit_code: out.exit_code,
        ..CommandResult::default()
    }
}
