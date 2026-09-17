//! Pure, serializable computer substrate. No host filesystem, clock, or process access.
pub mod git;
pub mod packages;
pub mod process;
pub mod shell;
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
}
impl CommandResult {
    pub fn success(stdout: impl Into<String>) -> Self {
        Self {
            stdout: stdout.into(),
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
}
pub struct OfflineHost;
impl ShellHost for OfflineHost {
    fn http(&mut self, _: HttpRequest) -> Result<HttpResponse, String> {
        Err("network adapter unavailable".into())
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
