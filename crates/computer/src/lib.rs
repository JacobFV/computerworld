//! Pure, serializable computer substrate. No host filesystem, clock, or process access.
mod awk;
mod datautils;
pub mod debug;
pub mod git;
pub mod packages;
pub mod process;
pub mod runtimes;
mod sed;
pub mod shell;
mod sqlite;
mod textutils;
pub mod vfs;
use cw_protocol::{HttpRequest, HttpResponse};
pub use packages::*;
pub use process::*;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
pub use vfs::*;
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct CommandResult {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
    pub pid: u64,
    /// Screen action, not output: `clear` produces no bytes, so the terminal
    /// application must erase its scrollback itself when this is set.
    #[serde(default)]
    pub clear: bool,
}
impl CommandResult {
    pub fn success(stdout: impl Into<String>) -> Self {
        Self {
            stdout: stdout.into(),
            ..Self::default()
        }
    }
    /// Failure with an explicit status; see docs/shell.md for the code vocabulary.
    pub fn new(stderr: impl Into<String>, exit_code: i32) -> Self {
        Self {
            stderr: stderr.into(),
            exit_code,
            ..Self::default()
        }
    }
    pub fn error(stderr: impl Into<String>) -> Self {
        Self {
            stderr: stderr.into(),
            exit_code: 1,
            ..Self::default()
        }
    }
}
pub trait ShellHost {
    fn http(&mut self, request: HttpRequest) -> Result<HttpResponse, String>;
    fn cleanup_process(&mut self, _pid: u64) {}
    fn advance(&mut self, _ticks: u64) -> Result<u64, String> {
        Err("clock adapter unavailable".into())
    }
    fn start_service(&mut self, _name: &str, _pid: u64) -> Result<String, String> {
        Err("service adapter unavailable".into())
    }
    /// Seed material for programs that ask for randomness (an unseeded
    /// `random`, `Math.random`). The world supplies it from its seeded streams;
    /// without a world it is a constant, so runs stay reproducible.
    fn entropy(&mut self) -> u64 {
        0x5eed_c0de_2026_0917
    }
}
pub struct OfflineHost;
impl ShellHost for OfflineHost {
    fn http(&mut self, _: HttpRequest) -> Result<HttpResponse, String> {
        Err("network adapter unavailable".into())
    }
}
/// Facts the world does not simulate (cores, RAM, disk capacity, NIC) but that
/// probing commands must still report. Fixed per computer so every call and every
/// replay agrees; never sampled from the host.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Hardware {
    pub cpus: u32,
    pub memory_bytes: u64,
    /// Capacity of the single modelled filesystem; usage is summed from the VFS.
    pub disk_bytes: u64,
    pub device: String,
    pub interface: String,
    pub ipv4: String,
    pub prefix: u8,
    pub mac: String,
    pub gateway: String,
    /// Tick at which this computer booted; `uptime` is the distance from it.
    pub boot_tick: u64,
    /// Numeric identity reported by `stat`; the VFS stores owners by name only.
    pub uid: u32,
    pub gid: u32,
}
impl Default for Hardware {
    fn default() -> Self {
        Self {
            cpus: 4,
            memory_bytes: 8 << 30,
            disk_bytes: 64 << 30,
            device: "/dev/vda1".into(),
            interface: "eth0".into(),
            ipv4: "10.0.2.15".into(),
            prefix: 24,
            mac: "52:54:00:12:34:56".into(),
            gateway: "10.0.2.1".into(),
            boot_tick: 0,
            uid: 1000,
            gid: 1000,
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Computer {
    pub id: String,
    pub user: String,
    pub os_family: String,
    pub dialect: String,
    pub cwd: String,
    pub env: BTreeMap<String, String>,
    pub vfs: Vfs,
    pub processes: ProcessTable,
    pub packages: PackageManager,
    #[serde(default)]
    pub installed_apps: std::collections::BTreeSet<String>,
    #[serde(default)]
    pub hardware: Hardware,
    /// Programs paused under a debugger (`debug::DebugAdapter`).
    #[serde(default, skip_serializing_if = "debug_is_empty")]
    pub debug: debug::DebugTable,
}
fn debug_is_empty(t: &debug::DebugTable) -> bool {
    t.sessions.is_empty() && t.next == 0
}
impl Computer {
    pub fn validate(&self) -> cw_protocol::Result<()> {
        self.vfs
            .validate()
            .map_err(|e| cw_protocol::SimError::invalid(e.to_string()))?;
        self.processes
            .validate()
            .map_err(cw_protocol::SimError::invalid)?;
        if normalize_path("/", &self.cwd) != self.cwd {
            return Err(cw_protocol::SimError::invalid(
                "computer cwd is not canonical",
            ));
        }
        Ok(())
    }
    pub fn new(
        id: impl Into<String>,
        user: impl Into<String>,
        os_family: impl Into<String>,
        case_sensitive: bool,
    ) -> Self {
        let user = user.into();
        let os_family = os_family.into();
        let home = format!("/home/{user}");
        let mut vfs = Vfs::new(case_sensitive);
        for p in ["/tmp", "/bin", "/usr/bin", "/etc", "/home", home.as_str()] {
            let _ = vfs.mkdir_all(p, &user, 0);
        }
        let _ = vfs.chmod("/tmp", 0o1777);
        Self {
            id: id.into(),
            user: user.clone(),
            dialect: if os_family == "windows" {
                "powershell"
            } else {
                "posix"
            }
            .into(),
            os_family,
            cwd: home.clone(),
            env: BTreeMap::from([
                ("HOME".into(), home),
                ("USER".into(), user),
                ("PATH".into(), "/bin:/usr/bin".into()),
            ]),
            vfs,
            processes: ProcessTable::new(),
            packages: PackageManager::default(),
            installed_apps: std::collections::BTreeSet::new(),
            hardware: Hardware::default(),
            debug: debug::DebugTable::default(),
        }
    }
    pub fn from_definition(
        def: &cw_protocol::ComputerDefinition,
        profile: &cw_protocol::OsProfile,
    ) -> cw_protocol::Result<Self> {
        let mut c = Self::new(&def.id, &def.user, &profile.family, profile.case_sensitive);
        c.installed_apps = def.installed_apps.iter().cloned().collect();
        if !profile.shell.is_empty() {
            c.dialect = profile.shell.clone();
        }
        if !profile.home.is_empty() {
            c.cwd = normalize_path("/", &profile.home.replace("{user}", &def.user));
            c.env.insert("HOME".into(), c.cwd.clone());
            c.vfs
                .mkdir_all(&c.cwd, &def.user, 0)
                .map_err(|e| cw_protocol::SimError::invalid(e.to_string()))?;
        }
        for (path, content) in &def.initial_files {
            let path = c.resolve(path);
            let parent = path.rsplit_once('/').unwrap().0;
            c.vfs
                .mkdir_all(parent, &def.user, 0)
                .map_err(|e| cw_protocol::SimError::invalid(e.to_string()))?;
            c.vfs
                .write(&path, content.as_bytes(), &def.user, 0)
                .map_err(|e| cw_protocol::SimError::invalid(e.to_string()))?;
        }
        for (path, bytes) in def.binary_files()? {
            let path = c.resolve(path);
            let parent = path.rsplit_once('/').unwrap().0;
            c.vfs
                .mkdir_all(parent, &def.user, 0)
                .map_err(|e| cw_protocol::SimError::invalid(e.to_string()))?;
            c.vfs
                .write(&path, &bytes, &def.user, 0)
                .map_err(|e| cw_protocol::SimError::invalid(e.to_string()))?;
        }
        for name in &def.packages {
            c.packages.register(Package {
                name: name.clone(),
                version: "1.0.0".into(),
                dependencies: BTreeMap::new(),
                files: BTreeMap::new(),
                executables: vec![name.clone()],
            });
            c.packages
                .install(name, &mut c.vfs, &def.user, 0)
                .map_err(cw_protocol::SimError::invalid)?;
        }
        Ok(c)
    }
    pub fn application_available(&self, kind: &str) -> bool {
        self.installed_apps.contains(kind) || self.packages.installed.contains_key(kind)
    }
    pub fn execute(&mut self, command: &str, tick: u64, host: &mut dyn ShellHost) -> CommandResult {
        shell::execute(self, command, tick, host)
    }
    pub fn resolve(&self, path: &str) -> String {
        normalize_path(&self.cwd, path)
    }
}
