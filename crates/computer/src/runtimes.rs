//! Language runtimes (`python3`, `node`) wired to a computer: the interpreter
//! sees this machine's VFS, the world clock and the world's entropy, and nothing
//! of the host.
use crate::{normalize_path, CommandResult, Computer, ShellHost, VfsError};
use cw_script_host::{FileStat, FsError, FsErrorKind, Invocation, Outcome, ScriptHost};

/// The script host for one interpreter run. The working directory is the
/// process's own: a program's `os.chdir` does not move the shell.
pub struct MachineHost<'a> {
    pub computer: &'a mut Computer,
    pub shell: &'a mut dyn ShellHost,
    pub cwd: String,
    pub tick: u64,
    pub pid: u64,
    seed: Option<u64>,
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
            seed: None,
        }
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
            c.vfs.append_as(&p, data, &user, self.tick).map_err(fs_error)
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
) -> CommandResult {
    let env: Vec<(String, String)> = computer
        .env
        .iter()
        .filter(|(k, _)| k.chars().next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_'))
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    let invocation = Invocation {
        args: args.to_vec(),
        env,
        stdin: stdin.to_string(),
    };
    let mut host = MachineHost::new(computer, shell, tick);
    let out: Outcome = match runtime {
        Runtime::Python => cw_pyvm::run(&mut host, &invocation),
        Runtime::Node => Outcome {
            stdout: String::new(),
            stderr: "node: this runtime is not available on this machine\n".into(),
            exit_code: 127,
        },
    };
    CommandResult {
        stdout: out.stdout,
        stderr: out.stderr,
        exit_code: out.exit_code,
        ..CommandResult::default()
    }
}
