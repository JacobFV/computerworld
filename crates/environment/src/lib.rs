//! Capability-filtered actor interfaces above the canonical kernel.
//! Owner inspection and snapshots are deliberately separate from actor observations.
mod desktop_extensions;
use cw_applications::desktop_scene::{
    home_page_count, render_desktop_with_options, window_content_rect_for_kind, work_area,
    DesktopTheme, ShellOptions, WindowView,
};
use cw_applications::{AppState, DesktopState};
use cw_browser::BrowserState;
use cw_kernel::Runtime;
use cw_protocol::*;
use cw_scene::Scene;
use cw_trajectory::Journal;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

#[derive(Clone, Serialize, Deserialize)]
pub struct MachineSession {
    pub browser: BrowserState,
    #[serde(default)]
    pub browser_windows: BTreeMap<u64, BrowserState>,
    #[serde(default)]
    pub active_browser_window: Option<u64>,
    pub desktop: DesktopState,
    pub terminal: Value,
    pub focused_input: Option<String>,
    pub browser_visible: bool,
    pub registered: cw_applications::RegisteredApplications,
    pub active_app: Option<String>,
    pub custom_page: Option<Page>,
    #[serde(default)]
    pub address_focused: bool,
    #[serde(default)]
    pub touch_start: Option<(i32, i32)>,
    #[serde(default)]
    pub pointer_position: Option<(i32, i32)>,
    /// Press identity remains stable even if focusing changes the taskbar action.
    #[serde(default)]
    pub pointer_press: Option<(String, cw_scene::Rect)>,
}
impl Default for MachineSession {
    fn default() -> Self {
        Self {
            browser: BrowserState::default(),
            browser_windows: BTreeMap::new(),
            active_browser_window: None,
            desktop: DesktopState::default(),
            terminal: Value::Null,
            focused_input: None,
            browser_visible: false,
            registered: Default::default(),
            active_app: None,
            custom_page: None,
            address_focused: false,
            touch_start: None,
            pointer_position: None,
            pointer_press: None,
        }
    }
}
#[derive(Clone, Serialize, Deserialize)]
pub struct ActorSession {
    pub config: EnvironmentConfig,
    pub machines: BTreeMap<String, MachineSession>,
    pub focused_machine: String,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct RecordedStep {
    pub session: String,
    pub actions: Vec<ActionEnvelope>,
    pub state_hash: String,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Snapshot {
    pub version: u32,
    pub kernel: cw_kernel::Snapshot,
    pub sessions: Arc<BTreeMap<String, ActorSession>>,
    pub next_session: u64,
    pub journal: Arc<Journal<RecordedStep>>,
    pub app_modules: BTreeMap<String, u32>,
    pub interface_modules: BTreeMap<String, u32>,
    pub verify_steps: bool,
}
/// Trusted owner-installed extension. Extensions are privileged code; actor input is
/// still checked against its registered family and machine grants before dispatch.
pub trait ActionFamily: Send + Sync {
    fn family(&self) -> &str;
    fn version(&self) -> u32 {
        1
    }
    fn execute(&self, runtime: &mut Runtime, actor: &str, action: &ActionEnvelope)
        -> Result<Value>;
}
/// Encodes a scene to PNG and decodes an image file to pixels. The renderer is an
/// optional dependency, so a build without it has no screenshot and shows no photograph,
/// rather than a fake one of either.
pub trait Raster: Send + Sync {
    fn png(&self, scene: &Scene) -> std::result::Result<Vec<u8>, String>;
    /// Decode image bytes to `(width, height, rgba)`.
    fn decode(&self, bytes: &[u8]) -> std::result::Result<(u32, u32, Vec<u8>), String>;
    /// Encode straight RGBA pixels as a PNG file.
    fn encode(
        &self,
        _width: u32,
        _height: u32,
        _rgba: &[u8],
    ) -> std::result::Result<Vec<u8>, String> {
        Err("this build cannot encode images".into())
    }
    /// Rasterise a scene to `(width, height, rgba)` without encoding it.
    fn pixels(&self, _scene: &Scene) -> std::result::Result<(u32, u32, Vec<u8>), String> {
        Err("this build has no rasterizer".into())
    }
}
pub trait ObservationChannel: Send + Sync {
    fn channel(&self) -> &str;
    fn version(&self) -> u32 {
        1
    }
    fn observe(&self, runtime: &Runtime, session: &ActorSession) -> Result<Value>;
}
pub struct Environment {
    pub runtime: Runtime,
    sessions: Arc<BTreeMap<String, ActorSession>>,
    next_session: u64,
    journal: Arc<Journal<RecordedStep>>,
    extensions: BTreeMap<String, Arc<dyn ActionFamily>>,
    app_registry: cw_sdk::Registry,
    observation_extensions: BTreeMap<String, Arc<dyn ObservationChannel>>,
    capture: Option<Arc<dyn Raster>>,
    verify_steps: bool,
}
impl Environment {
    pub fn new(runtime: Runtime) -> Self {
        Self {
            runtime,
            sessions: Arc::new(BTreeMap::new()),
            next_session: 0,
            journal: Arc::new(Journal::default()),
            extensions: BTreeMap::new(),
            app_registry: cw_sdk::Registry::new(),
            observation_extensions: BTreeMap::new(),
            capture: None,
            verify_steps: false,
        }
    }
    /// Install the rasterizer a screenshot is taken with. Without one, the Screenshot
    /// effect refuses rather than writing something that is not a picture of the screen.
    pub fn register_raster(&mut self, capture: Arc<dyn Raster>) {
        self.capture = Some(capture);
    }
    pub fn with_registry(runtime: Runtime, registry: cw_sdk::Registry) -> Self {
        let mut environment = Self::new(runtime);
        environment.app_registry = registry;
        environment
    }
    pub fn register_observation_channel(
        &mut self,
        channel: Arc<dyn ObservationChannel>,
    ) -> Result<()> {
        let name = channel.channel().to_owned();
        if [
            "terminal.v1",
            "semantic.v1",
            "browser.v1",
            "filesystem.v1",
            "pixels.v1",
        ]
        .contains(&name.as_str())
            || self.observation_extensions.contains_key(&name)
        {
            return Err(SimError::invalid("observation channel already registered"));
        }
        self.observation_extensions.insert(name, channel);
        Ok(())
    }
    pub fn register_application<A: cw_sdk::Application + 'static>(&mut self, app: A) -> Result<()> {
        self.app_registry.register_application(app)
    }
    pub fn register_action_family(&mut self, family: Arc<dyn ActionFamily>) -> Result<()> {
        let name = family.family().to_owned();
        if BUILTIN_FAMILIES.contains(&name.as_str()) || self.extensions.contains_key(&name) {
            return Err(SimError::invalid("action family already registered"));
        }
        self.extensions.insert(name, family);
        Ok(())
    }
    pub fn add_computer(
        &mut self,
        computer: ComputerDefinition,
        node: NetworkNode,
        links: Vec<NetworkLink>,
    ) -> Result<()> {
        self.runtime.add_computer(computer, node, links)
    }
    pub fn remove_computer(&mut self, id: &str) -> Result<()> {
        self.runtime.remove_computer(id)?;
        self.prune_sessions();
        Ok(())
    }
    fn prune_sessions(&mut self) {
        let runtime = &self.runtime;
        Arc::make_mut(&mut self.sessions).retain(|_, session| {
            session
                .config
                .machines
                .retain(|id| runtime.computer(id).is_ok());
            session
                .machines
                .retain(|id, _| session.config.machines.contains(id));
            if session.config.machines.is_empty() {
                return false;
            }
            if !session.machines.contains_key(&session.focused_machine) {
                session.focused_machine = session.config.machines[0].clone();
            }
            true
        });
    }
    pub fn environment(&mut self, config: EnvironmentConfig) -> Result<String> {
        if config.actor.trim().is_empty() || config.machines.is_empty() || config.action_budget == 0
        {
            return Err(SimError::invalid("invalid actor configuration"));
        }
        for machine in &config.machines {
            self.runtime
                .computer(machine)
                .map_err(|_| SimError::invalid("unknown granted machine"))?;
        }
        for family in &config.actions {
            if !BUILTIN_FAMILIES.contains(&family.as_str()) && !self.extensions.contains_key(family)
            {
                return Err(SimError::invalid("unknown action family"));
            }
        }
        for channel in &config.observations {
            if ![
                "terminal.v1",
                "semantic.v1",
                "browser.v1",
                "filesystem.v1",
                "pixels.v1",
            ]
            .contains(&channel.as_str())
                && !self.observation_extensions.contains_key(channel)
            {
                return Err(SimError::invalid("unknown observation channel"));
            }
        }
        let focused_machine = config.machines[0].clone();
        let machines = config
            .machines
            .iter()
            .map(|m| (m.clone(), self.new_machine_session(m)))
            .collect();
        let id = format!("session-{}", self.next_session);
        self.next_session += 1;
        Arc::make_mut(&mut self.sessions).insert(
            id.clone(),
            ActorSession {
                config,
                machines,
                focused_machine,
            },
        );
        self.first_login(&id)?;
        Ok(id)
    }
    /// A desktop session's first login makes the user's standard folders, as
    /// `xdg-user-dirs-update` does on Ubuntu and a new profile does on Windows and
    /// macOS, and the trash folder deletions go to. Only missing folders are made, so
    /// a second login changes nothing, and a snapshot taken after this carries them.
    /// A session without a desktop shell logs nobody in and makes nothing.
    fn first_login(&mut self, id: &str) -> Result<()> {
        let session = self.session(id)?;
        let actor = session.config.actor.clone();
        let machines: Vec<String> = session.machines.keys().cloned().collect();
        for machine in machines {
            let Some(theme) = self.desktop_theme(id, &machine) else {
                continue;
            };
            let desktop = &self.session(id)?.machines[&machine].desktop;
            let home = desktop.home_folder();
            if theme.mobile() || home == "/" {
                continue;
            }
            let mut wanted: Vec<String> = cw_applications::standard_folders(theme)
                .iter()
                .map(|name| format!("{}/{name}", home.trim_end_matches('/')))
                .collect();
            wanted.push(desktop.trash_folder());
            for path in wanted {
                if !self.runtime.computer(&machine)?.vfs.exists(&path) {
                    self.runtime.create_directory(&machine, &actor, &path)?;
                }
            }
        }
        Ok(())
    }
    /// A fresh desktop for `machine`, seeded with the facts the shell needs about it.
    fn new_machine_session(&self, machine: &str) -> MachineSession {
        let mut session = MachineSession::default();
        if let Ok(computer) = self.runtime.computer(machine) {
            session.desktop.home = computer
                .env
                .get("HOME")
                .cloned()
                .unwrap_or_else(|| computer.cwd.clone());
            // Same source as `home`: the machine states its own prompt.
            session.desktop.prompt = cw_applications::shell_prompt_at(
                &computer.user,
                &computer.id,
                &computer.cwd,
                &session.desktop.home,
                prompt_dialect(&computer.os_family, &computer.dialect),
            );
        }
        session
    }
    pub fn session(&self, id: &str) -> Result<&ActorSession> {
        self.sessions
            .get(id)
            .ok_or_else(|| SimError::denied("unknown actor session"))
    }
    pub fn observe(&self, id: &str) -> Result<Observation> {
        let session = self.session(id)?;
        let mut channels = BTreeMap::new();
        for channel in &session.config.observations {
            if let Some(extension) = self.observation_extensions.get(channel) {
                channels.insert(
                    channel.clone(),
                    extension
                        .observe(&self.runtime, session)
                        .map_err(actor_error)?,
                );
                continue;
            }
            let mut machines = BTreeMap::new();
            for (id, machine) in &session.machines {
                let value = match channel.as_str() {
                    "terminal.v1" => machine.terminal.clone(),
                    "semantic.v1" => serde_json::to_value(self.project_page(
                        &session.config.actor,
                        id,
                        machine,
                    )?)?,
                    "browser.v1" => {
                        json!({"page":machine.browser.page(),"url":machine.browser.url(),"fields":machine.browser.tab().fields})
                    }
                    "filesystem.v1" => {
                        let c = self.runtime.computer(id)?;
                        json!({"cwd":c.cwd})
                    }
                    _ => continue,
                };
                machines.insert(id.clone(), value);
            }
            channels.insert(channel.clone(), serde_json::to_value(machines)?);
        }
        Ok(Observation {
            tick: self.runtime.tick(),
            channels,
        })
    }
    pub fn step(&mut self, id: &str, actions: Vec<ActionEnvelope>) -> Result<StepResult> {
        let config = self.session(id)?.config.clone();
        if actions.len() > config.action_budget as usize {
            return Err(SimError::denied("action batch exceeds budget"));
        }
        let mut outcomes = Vec::with_capacity(actions.len());
        for (index, action) in actions.iter().enumerate() {
            // The envelope's success says nothing about the app-level consequence, so
            // bracket the dispatch with an actor-visible projection of the target.
            let permitted = config.machines.contains(&action.machine)
                && config.actions.contains(&action.family);
            let before = permitted
                .then(|| {
                    self.session(id)
                        .ok()?
                        .machines
                        .get(&action.machine)
                        .map(visible)
                })
                .flatten();
            let result = if permitted {
                self.dispatch(id, &config.actor, action)
            } else {
                Err(SimError::denied("action is not permitted"))
            };
            // Windows that share one document (KiCad's frames) all show the copy the
            // action changed, whether the action succeeded or stopped part-way.
            if permitted
                && self
                    .session(id)
                    .ok()
                    .and_then(|s| s.machines.get(&action.machine))
                    .is_some_and(|m| m.desktop.needs_settle())
            {
                if let Ok(m) = self.machine_mut(id, &action.machine) {
                    m.desktop.settle_linked();
                }
            }
            let effect = before.and_then(|before| {
                let after = visible(self.session(id).ok()?.machines.get(&action.machine)?);
                Some(effect_of(&before, &after))
            });
            self.runtime.record_event(
                "agent.action",
                Some(&action.machine),
                Some(&config.actor),
                json!({"session":id,"action":action,"success":result.is_ok()}),
            );
            outcomes.push(match result {
                Ok(value) => ActionOutcome {
                    index,
                    success: true,
                    value,
                    error: None,
                    effect,
                },
                Err(e) => ActionOutcome {
                    index,
                    success: false,
                    value: Value::Null,
                    error: Some(actor_error(e)),
                    effect,
                },
            });
        }
        // Work an application does between actions — a video export encoding its next
        // frames — advances once per step, so its progress is a function of the steps
        // taken and replays exactly.
        let machines: Vec<String> = self.session(id)?.machines.keys().cloned().collect();
        for machine in machines {
            let busy = self
                .session(id)?
                .machines
                .get(&machine)
                .is_some_and(|m| m.desktop.busy());
            if !busy {
                continue;
            }
            let effects = self.machine_mut(id, &machine)?.desktop.background();
            if let Err(e) = self.effects(id, &machine, &config.actor, effects) {
                self.runtime.record_event(
                    "application.background_failed",
                    Some(&machine),
                    Some(&config.actor),
                    json!({"session":id,"error":actor_error(e).message}),
                );
            }
        }
        let state_hash = if self.verify_steps {
            self.state_hash()?
        } else {
            String::new()
        };
        Arc::make_mut(&mut self.journal).append(RecordedStep {
            session: id.into(),
            actions,
            state_hash,
        })?;
        Ok(StepResult {
            observation: self.observe(id)?,
            outcomes,
            tick: self.runtime.tick(),
            pending: self.runtime.pending(),
        })
    }
    pub fn snapshot(&self) -> Snapshot {
        Snapshot {
            version: 1,
            kernel: self.runtime.snapshot(),
            sessions: self.sessions.clone(),
            next_session: self.next_session,
            journal: self.journal.clone(),
            app_modules: self.app_registry.module_versions(),
            interface_modules: self.interface_versions(),
            verify_steps: self.verify_steps,
        }
    }
    pub fn restore(&mut self, snapshot: &Snapshot) -> Result<()> {
        if snapshot.version != 1 {
            return Err(SimError::invalid("unsupported environment snapshot"));
        }
        self.validate_snapshot(snapshot)?;
        self.runtime.restore(&snapshot.kernel)?;
        self.sessions = snapshot.sessions.clone();
        self.next_session = snapshot.next_session;
        self.journal = snapshot.journal.clone();
        self.verify_steps = snapshot.verify_steps;
        Ok(())
    }
    pub fn fork(&self, snapshot: &Snapshot) -> Result<Self> {
        let runtime = self.runtime.fork(&snapshot.kernel)?;
        let mut env = Self::new(runtime);
        env.extensions = self.extensions.clone();
        env.app_registry = self.app_registry.clone();
        env.observation_extensions = self.observation_extensions.clone();
        env.capture = self.capture.clone();
        env.restore(snapshot)?;
        Ok(env)
    }
    pub fn reset(&mut self, seed: u64) -> Result<()> {
        self.runtime.reset(seed)?;
        self.prune_sessions();
        let fresh: BTreeMap<String, MachineSession> = self
            .sessions
            .values()
            .flat_map(|s| s.machines.keys())
            .map(|m| (m.clone(), self.new_machine_session(m)))
            .collect();
        for session in Arc::make_mut(&mut self.sessions).values_mut() {
            for (id, machine) in session.machines.iter_mut() {
                *machine = fresh.get(id).cloned().unwrap_or_default();
            }
            session.focused_machine = session.config.machines[0].clone();
        }
        // A reset world is logged into afresh, exactly as a new one is.
        let ids: Vec<String> = self.sessions.keys().cloned().collect();
        for id in ids {
            self.first_login(&id)?;
        }
        self.journal = Arc::new(Journal::default());
        Ok(())
    }
    pub fn export_snapshot(&self) -> Result<String> {
        self.runtime.try_snapshot()?;
        Ok(serde_json::to_string(&self.snapshot())?)
    }
    pub fn import_snapshot(&mut self, json: &str) -> Result<()> {
        let snapshot: Snapshot = serde_json::from_str(json)?;
        if !snapshot.journal.verify()? {
            return Err(SimError::invalid("invalid action journal"));
        }
        self.restore(&snapshot)
    }
    pub fn state_hash(&self) -> Result<String> {
        Ok(cw_trajectory::stable_hash(&(
            self.runtime.state_hash()?,
            &self.sessions,
            self.next_session,
        ))?)
    }
    pub fn trajectory(&self) -> Vec<EventRecord> {
        self.runtime.events().to_vec()
    }
    /// Opt in to a semantic hash after every step. Default journals retain actions
    /// and a tamper-evident chain without serializing the full world on the hot path.
    pub fn enable_replay_verification(&mut self, enabled: bool) {
        self.verify_steps = enabled;
    }
    pub fn action_journal(&self) -> &Journal<RecordedStep> {
        &self.journal
    }
    pub fn replay(&mut self, initial: &Snapshot, journal: &Journal<RecordedStep>) -> Result<()> {
        if !journal.verify()? {
            return Err(SimError::invalid("invalid replay journal"));
        }
        self.restore(initial)?;
        for entry in &journal.entries {
            self.step(&entry.value.session, entry.value.actions.clone())?;
            if !entry.value.state_hash.is_empty()
                && self.state_hash() != Ok(entry.value.state_hash.clone())
            {
                return Err(SimError::new("replay_diverged", "semantic state differs"));
            }
        }
        Ok(())
    }
    pub fn inspect(&self) -> Value {
        self.runtime.inspect()
    }
}
const BUILTIN_FAMILIES: &[&str] = &[
    "terminal.v1",
    "filesystem.v1",
    "http.v1",
    "browser.v1",
    "application.v1",
    "keyboard.v1",
    "pointer.v1",
];
/// Whose prompt a machine's terminal prints: a Mac's POSIX shell is zsh, so it prints
/// zsh's; every other machine prints its own dialect's.
fn prompt_dialect<'a>(os_family: &str, dialect: &'a str) -> &'a str {
    if os_family == "macos" && dialect == "posix" {
        "zsh"
    } else {
        dialect
    }
}
/// File name a download lands under: the URL's last path segment, or the host when the
/// URL names no file. Never a guess about content type.
/// Files one `ReadFiles` may return, and bytes across all of them: a workspace search
/// reads many files, and must not make one action unbounded.
const READ_FILES_LIMIT: usize = 512;
const READ_FILES_BYTES: usize = 8 << 20;
/// Entries one `ListTree` may return; one more than an application keeps tells it the
/// listing was cut.
const LIST_TREE_LIMIT: usize = 4001;

/// A folder tree as relative paths, folders marked with `/`, depth first in name order,
/// walking only what the machine's user may read. Version-control internals and
/// dependency folders are listed but not descended into.
fn list_tree(
    c: &cw_computer::Computer,
    path: &str,
    depth: u32,
) -> std::result::Result<Vec<String>, String> {
    let base = c.resolve(path);
    let meta = c
        .vfs
        .stat(&base)
        .map_err(|_| "folder not found".to_owned())?;
    if !meta.is_dir {
        return Err("not a folder".into());
    }
    c.vfs
        .check_access(&base, &c.user, true, false, true)
        .map_err(|_| "folder access denied".to_owned())?;
    let mut out = Vec::new();
    let mut stack = vec![(base, String::new(), 0u32)];
    while let Some((dir, rel, level)) = stack.pop() {
        let Ok(mut names) = c.vfs.list(&dir) else {
            continue;
        };
        names.sort();
        let mut folders = Vec::new();
        for name in names {
            if out.len() >= LIST_TREE_LIMIT {
                return Ok(out);
            }
            let full = format!("{}/{name}", dir.trim_end_matches('/'));
            let child = if rel.is_empty() {
                name.clone()
            } else {
                format!("{rel}/{name}")
            };
            if c.vfs.stat(&full).is_ok_and(|m| m.is_dir) {
                out.push(format!("{child}/"));
                let readable = c
                    .vfs
                    .check_access(&full, &c.user, true, false, true)
                    .is_ok();
                if level + 1 < depth && readable && name != ".git" && name != "node_modules" {
                    folders.push((full, child, level + 1));
                }
            } else {
                out.push(child);
            }
        }
        // Popped in name order.
        stack.extend(folders.into_iter().rev());
    }
    Ok(out)
}

fn download_name(url: &str) -> String {
    let trimmed = url.split(['?', '#']).next().unwrap_or(url);
    let last = trimmed
        .trim_end_matches('/')
        .rsplit('/')
        .next()
        .unwrap_or_default();
    let safe: String = last
        .chars()
        .filter(|c| c.is_alphanumeric() || matches!(c, '.' | '-' | '_'))
        .take(96)
        .collect();
    if safe.is_empty() || safe.starts_with('.') {
        let host = url
            .trim_start_matches("http://")
            .trim_start_matches("https://")
            .split('/')
            .next()
            .unwrap_or("download");
        let host: String = host
            .chars()
            .filter(|c| c.is_alphanumeric() || *c == '.' || *c == '-')
            .collect();
        format!("{}.page", if host.is_empty() { "download" } else { &host })
    } else {
        safe
    }
}
fn actor_error(e: SimError) -> SimError {
    let code = match e.code.as_str() {
        "denied" => "denied",
        "not_found" => "not_found",
        "invalid" => "invalid",
        _ => "action_failed",
    };
    SimError::new(
        code,
        match code {
            "denied" => "action is not permitted",
            "not_found" => "requested resource not found",
            "invalid" => "invalid action",
            _ => "action failed",
        },
    )
}
/// Why a document could not be read or written, in the words a dialog uses. Only the
/// kind of failure is shown, never the kernel's own message.
fn file_problem(e: SimError) -> String {
    match e.code.as_str() {
        "denied" => "you don't have permission".into(),
        "not_found" => "the file or its folder does not exist".into(),
        "quota" | "no_space" => "there is not enough space".into(),
        _ => "the file could not be accessed".into(),
    }
}
fn string<'a>(payload: &'a Value, key: &str) -> Result<&'a str> {
    payload
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| SimError::invalid("missing string argument"))
}
impl Environment {
    fn machine_mut(&mut self, id: &str, machine: &str) -> Result<&mut MachineSession> {
        Arc::make_mut(&mut self.sessions)
            .get_mut(id)
            .and_then(|s| s.machines.get_mut(machine))
            .ok_or_else(|| SimError::denied("machine unavailable"))
    }
    fn dispatch(&mut self, id: &str, actor: &str, action: &ActionEnvelope) -> Result<Value> {
        let machine = &action.machine;
        let p = &action.payload;
        Arc::make_mut(&mut self.sessions)
            .get_mut(id)
            .unwrap()
            .focused_machine = machine.clone();
        let tick = self.runtime.tick();
        if let Ok(state) = self.machine_mut(id, machine) {
            state.desktop.clock_us = tick;
        }
        match (action.family.as_str(), action.op.as_str()) {
            ("terminal.v1", "execute") => {
                let value = serde_json::to_value(self.runtime.execute(
                    machine,
                    actor,
                    string(p, "command")?,
                )?)?;
                self.machine_mut(id, machine)?.terminal = value.clone();
                Ok(value)
            }
            ("filesystem.v1", "read") => {
                let bytes = self.runtime.read_file(machine, string(p, "path")?)?;
                Ok(json!({"content":String::from_utf8_lossy(&bytes),"bytes":bytes}))
            }
            ("filesystem.v1", "write") => {
                self.runtime.write_file(
                    machine,
                    actor,
                    string(p, "path")?,
                    string(p, "content")?.as_bytes(),
                )?;
                Ok(Value::Null)
            }
            ("filesystem.v1", "list") => {
                let c = self.runtime.computer(machine)?;
                let path = c.resolve(string(p, "path")?);
                c.vfs
                    .check_access(&path, &c.user, true, false, true)
                    .map_err(|_| SimError::denied("directory access denied"))?;
                Ok(json!(c
                    .vfs
                    .list(&path)
                    .map_err(|_| SimError::not_found("directory"))?))
            }
            ("filesystem.v1", "stat") => {
                let c = self.runtime.computer(machine)?;
                let path = c.resolve(string(p, "path")?);
                Ok(serde_json::to_value(
                    c.vfs.stat(&path).map_err(|_| SimError::not_found("path"))?,
                )?)
            }
            ("http.v1", "request") => {
                let request: HttpRequest = serde_json::from_value(p.clone())?;
                Ok(serde_json::to_value(
                    self.runtime.http(machine, actor, request)?,
                )?)
            }
            ("browser.v1", _) => self.browser_action(id, actor, action),
            ("application.v1", "event") => self.custom_event(id, machine, actor, p),
            ("application.v1", "launch") => {
                let requested = string(p, "kind")?;
                let alias = self.desktop_alias(id, machine, requested)?;
                let canonical = match requested {
                    "text_editor" => "editor",
                    "file_manager" => "files",
                    other => other,
                };
                if canonical == "browser"
                    && !self
                        .session(id)?
                        .config
                        .actions
                        .iter()
                        .any(|family| family == "browser.v1")
                {
                    return Err(SimError::denied(
                        "browser application interaction is not permitted",
                    ));
                }
                let computer = self.runtime.computer(machine)?;
                if alias.is_none()
                    && !computer.application_available(requested)
                    && !computer.application_available(canonical)
                {
                    return Err(SimError::not_found("application is not installed"));
                }
                if self.app_registry.application(string(p, "kind")?).is_ok() {
                    return self.custom_launch(id, machine, actor, p);
                }
                self.machine_mut(id, machine)?.active_app = None;
                self.machine_mut(id, machine)?.custom_page = None;
                self.machine_mut(id, machine)?.browser_visible = false;
                self.machine_mut(id, machine)?.address_focused = false;
                self.machine_mut(id, machine)?.focused_input = None;
                let kind = if alias.is_some() {
                    "browser"
                } else {
                    requested
                };
                // A native application is told which service backs it in this world.
                let native = cw_applications::NativeApp::KINDS.contains(&kind);
                let supplied = p.get("argument").and_then(Value::as_str).unwrap_or("");
                let world_url = if native {
                    self.native_argument(id, machine, kind)
                } else {
                    String::new()
                };
                // A native application's argument is the service it talks to. Opening one
                // *on* something — a calendar on a date — keeps that service and carries
                // the place as a fragment, the web's own way to say "here, at this spot".
                let from_world = match (world_url.is_empty(), supplied.is_empty()) {
                    (false, true) => world_url,
                    (false, false) if world_url.starts_with("http") => {
                        format!("{world_url}#{supplied}")
                    }
                    _ => String::new(),
                };
                let arg =
                    alias
                        .as_ref()
                        .map(|a| a.url.as_str())
                        .unwrap_or(if from_world.is_empty() {
                            supplied
                        } else {
                            &from_world
                        });
                // Files, Finder and Explorer open on the user's home; a phone's file
                // browser opens on its storage root.
                let home;
                let arg = if arg.is_empty()
                    && matches!(kind, "files" | "file_manager")
                    && self.desktop_theme(id, machine).is_some_and(|t| !t.mobile())
                {
                    home = self.session(id)?.machines[machine].desktop.home_folder();
                    home.as_str()
                } else {
                    arg
                };
                let (window, effects) = self
                    .machine_mut(id, machine)?
                    .desktop
                    .launch(kind, arg)
                    .map_err(SimError::invalid)?;
                let empty_argument = arg.is_empty();
                if let Some(alias) = alias {
                    let w = self
                        .machine_mut(id, machine)?
                        .desktop
                        .windows
                        .get_mut(&window)
                        .unwrap();
                    w.app_id = alias.id;
                    w.title = alias.label;
                }
                self.sync_desktop_visibility(id, machine)?;
                self.effects(id, machine, actor, effects)?;
                if kind == "browser" && self.desktop_theme(id, machine).is_some() {
                    let state = self.machine_mut(id, machine)?;
                    state.browser_visible = true;
                    state.address_focused = empty_argument;
                }
                Ok(json!({"window":window}))
            }
            ("application.v1", "home" | "launcher" | "minimize" | "maximize" | "switcher") => {
                self.shell_action(id, machine, actor, &format!("shell:{}", action.op))
            }
            // Invoke a shell control by name. The same targets a pointer reaches by
            // hit-testing, addressable directly, through the identical handler and grants.
            ("application.v1", "shell") => {
                let target = string(p, "target")?.to_owned();
                if !target.starts_with("shell:") && !target.starts_with("window:") {
                    return Err(SimError::invalid("not a shell interaction target"));
                }
                if let Some(rest) = target.strip_prefix("window:") {
                    // Window-namespaced controls belong to the window they name.
                    let (window, operation) = rest
                        .split_once(':')
                        .ok_or_else(|| SimError::invalid("invalid window interaction"))?;
                    let window: u64 = window
                        .parse()
                        .map_err(|_| SimError::invalid("invalid window id"))?;
                    self.machine_mut(id, machine)?
                        .desktop
                        .focus(window)
                        .map_err(SimError::invalid)?;
                    self.sync_desktop_visibility(id, machine)?;
                    let inner = operation.strip_prefix("content:").unwrap_or(operation);
                    if inner.starts_with("shell:") {
                        return self.shell_action(id, machine, actor, inner);
                    }
                    let effects = self
                        .machine_mut(id, machine)?
                        .desktop
                        .click(inner)
                        .map_err(SimError::invalid)?;
                    self.effects(id, machine, actor, effects)?;
                    self.sync_desktop_visibility(id, machine)?;
                    return Ok(Value::Null);
                }
                self.shell_action(id, machine, actor, &target)
            }
            ("application.v1", "focus" | "close") => {
                self.machine_mut(id, machine)?.browser_visible = false;
                self.machine_mut(id, machine)?.active_app = None;
                self.machine_mut(id, machine)?.custom_page = None;
                self.machine_mut(id, machine)?.focused_input = None;
                let window = p
                    .get("window")
                    .and_then(integer_u64)
                    .ok_or_else(|| SimError::invalid("window required"))?;
                let d = &mut self.machine_mut(id, machine)?.desktop;
                match action.op.as_str() {
                    "focus" => d.focus(window),
                    _ => d.close(window),
                }
                .map_err(SimError::invalid)?;
                self.sync_desktop_visibility(id, machine)?;
                Ok(Value::Null)
            }
            ("keyboard.v1", "type") => {
                if self.desktop_panel_text(id, machine, string(p, "text")?)? {
                    return Ok(Value::Null);
                }
                if self.machine_mut(id, machine)?.active_app.is_some() {
                    return self.custom_event(id, machine, actor, &json!({"kind":"text","data":p}));
                }
                let text = string(p, "text")?;
                if self.machine_mut(id, machine)?.address_focused {
                    self.machine_mut(id, machine)?
                        .desktop
                        .text(text)
                        .map_err(SimError::invalid)?;
                } else if self.machine_mut(id, machine)?.browser_visible {
                    self.machine_mut(id, machine)?.browser.text(text)?;
                } else {
                    let effects = self
                        .machine_mut(id, machine)?
                        .desktop
                        .type_text(text)
                        .map_err(SimError::invalid)?;
                    self.effects(id, machine, actor, effects)?;
                }
                Ok(Value::Null)
            }
            ("keyboard.v1", "key") => {
                let key = string(p, "key")?;
                if self.desktop_panel_key(id, machine, actor, key)? {
                    return Ok(Value::Null);
                }
                if matches!(key, "Meta" | "Super" | "Meta+Space" | "Ctrl+Escape")
                    && self.desktop_theme(id, machine).is_some()
                {
                    return self.shell_action(id, machine, actor, "shell:launcher");
                }
                if key == "Alt+Tab" && self.desktop_theme(id, machine).is_some() {
                    return self.shell_action(id, machine, actor, "shell:switcher");
                }
                if self.machine_mut(id, machine)?.address_focused {
                    if key == "Enter"
                        && !self
                            .session(id)?
                            .config
                            .actions
                            .iter()
                            .any(|family| family == "browser.v1")
                    {
                        return Err(SimError::denied("browser navigation is not permitted"));
                    }
                    let effects = self
                        .machine_mut(id, machine)?
                        .desktop
                        .key(key)
                        .map_err(SimError::invalid)?;
                    self.effects(id, machine, actor, effects)?;
                    if key == "Enter" {
                        self.machine_mut(id, machine)?.address_focused = false;
                    }
                    return Ok(Value::Null);
                }
                if self.machine_mut(id, machine)?.active_app.is_some() {
                    return self.custom_event(id, machine, actor, &json!({"kind":"key","data":p}));
                }
                if self.machine_mut(id, machine)?.browser_visible {
                    // Browser zoom chords, on both platforms' modifier.
                    let step = match key {
                        "Ctrl+=" | "Ctrl++" | "Meta+=" | "Meta++" => Some("in"),
                        "Ctrl+-" | "Meta+-" => Some("out"),
                        "Ctrl+0" | "Meta+0" => Some("reset"),
                        _ => None,
                    };
                    if let Some(step) = step {
                        return self.shell_action(
                            id,
                            machine,
                            actor,
                            &format!("shell:zoom:{step}"),
                        );
                    }
                    return self.browser_action(
                        id,
                        actor,
                        &ActionEnvelope::new("browser.v1", "key", machine, p.clone()),
                    );
                }
                let effects = self
                    .machine_mut(id, machine)?
                    .desktop
                    .key(string(p, "key")?)
                    .map_err(SimError::invalid)?;
                self.effects(id, machine, actor, effects)?;
                Ok(Value::Null)
            }
            ("pointer.v1", "wheel") => {
                let coord = |k: &str| -> Result<i32> {
                    Ok(p.get(k)
                        .and_then(integer_i64)
                        .ok_or_else(|| SimError::invalid(format!("{k} required")))?
                        .clamp(-32768, 32768) as i32)
                };
                let (x, y) = (coord("x")?, coord("y")?);
                let width = p
                    .get("width")
                    .and_then(integer_u64)
                    .unwrap_or(1024)
                    .min(8192) as u32;
                let height = p
                    .get("height")
                    .and_then(integer_u64)
                    .unwrap_or(768)
                    .min(8192) as u32;
                let delta = p
                    .get("delta_y")
                    .and_then(integer_i64)
                    .unwrap_or(0)
                    .clamp(-100_000, 100_000) as i32;
                self.machine_mut(id, machine)?.pointer_position = Some((x, y));
                let scene = self.scene(id, width, height)?;
                let Some(node) = scene.hit_test(x, y) else {
                    return Ok(json!({"handled": false}));
                };
                let bounds = node.transform.bounds(node.bounds);
                let Some((window, inner)) = node
                    .interaction
                    .as_deref()
                    .and_then(|t| t.strip_prefix("window:"))
                    .and_then(|t| t.split_once(':'))
                    .and_then(|(w, op)| {
                        Some((
                            w.parse::<u64>().ok()?,
                            op.strip_prefix("content:")?.to_owned(),
                        ))
                    })
                else {
                    return Ok(json!({"handled": false}));
                };
                if !self
                    .session(id)?
                    .config
                    .actions
                    .iter()
                    .any(|family| family == "application.v1")
                {
                    return Err(SimError::denied("application interaction is not permitted"));
                }
                let handled = self
                    .machine_mut(id, machine)?
                    .desktop
                    .wheel(window, &inner, x - bounds.x, y - bounds.y, delta)
                    .map_err(SimError::invalid)?;
                Ok(json!({"handled": handled}))
            }
            ("pointer.v1", "click" | "down" | "move" | "up" | "cancel" | "double_click") => {
                let x = p
                    .get("x")
                    .and_then(integer_i64)
                    .ok_or_else(|| SimError::invalid("x required"))?
                    .clamp(-32768, 32768) as i32;
                let y = p
                    .get("y")
                    .and_then(integer_i64)
                    .ok_or_else(|| SimError::invalid("y required"))?
                    .clamp(-32768, 32768) as i32;
                let width = p
                    .get("width")
                    .and_then(integer_u64)
                    .unwrap_or(1024)
                    .min(8192) as u32;
                let height = p
                    .get("height")
                    .and_then(integer_u64)
                    .unwrap_or(768)
                    .min(8192) as u32;
                self.machine_mut(id, machine)?.pointer_position = Some((x, y));
                // Keys held with the pointer (`["ctrl"]`, `["alt"]`…): an image editor's
                // Ctrl- or Option-click sets a clone source.
                let modifiers = match p.get("modifiers") {
                    None | Some(Value::Null) => 0,
                    Some(Value::Array(names)) => {
                        let names: Vec<&str> = names.iter().filter_map(Value::as_str).collect();
                        if names.len() != p["modifiers"].as_array().map_or(0, Vec::len) {
                            return Err(SimError::invalid("modifiers are key names"));
                        }
                        cw_applications::modifier_bits(&names).map_err(SimError::invalid)?
                    }
                    Some(_) => return Err(SimError::invalid("modifiers are a list of key names")),
                };
                self.machine_mut(id, machine)?.desktop.pointer_modifiers = modifiers;
                let released_press = if action.op == "up" {
                    self.machine_mut(id, machine)?.pointer_press.take()
                } else {
                    None
                };
                if matches!(action.op.as_str(), "down" | "cancel") {
                    self.machine_mut(id, machine)?.pointer_press = None;
                }
                let theme = self.desktop_theme(id, machine);
                let area = theme.map(|theme| work_area(theme, width, height));
                let mobile = matches!(theme, Some(DesktopTheme::Ios | DesktopTheme::Android));
                if mobile && action.op == "down" {
                    self.machine_mut(id, machine)?.touch_start = Some((x, y));
                }
                if action.op == "cancel" {
                    self.machine_mut(id, machine)?.touch_start = None;
                }
                // A drag surface inside an application (a canvas, a slider) holds the
                // pointer from press to release: moves and the release go to it before
                // any shell gesture or hit test, even when they leave its bounds.
                if matches!(action.op.as_str(), "move" | "up" | "cancel")
                    && self.session(id)?.machines[machine].desktop.app_captured()
                {
                    let phase = match action.op.as_str() {
                        "move" => cw_applications::PointerPhase::Move,
                        "up" => cw_applications::PointerPhase::Up,
                        _ => cw_applications::PointerPhase::Cancel,
                    };
                    if phase != cw_applications::PointerPhase::Move {
                        self.machine_mut(id, machine)?.touch_start = None;
                    }
                    let delivered = self
                        .machine_mut(id, machine)?
                        .desktop
                        .app_pointer(phase, x, y);
                    if let Some(result) = delivered {
                        let effects = result.map_err(SimError::invalid)?;
                        self.effects(id, machine, actor, effects)?;
                        return Ok(if phase == cw_applications::PointerPhase::Move {
                            json!({"cursor":"crosshair"})
                        } else {
                            Value::Null
                        });
                    }
                }
                if mobile && action.op == "up" {
                    if let Some(start) = self.machine_mut(id, machine)?.touch_start.take() {
                        let pressed = released_press.as_ref().map(|(target, _)| target.as_str());
                        let gesture = self.touch_gesture(
                            id,
                            machine,
                            theme.expect("a phone theme"),
                            start,
                            (x, y),
                            (width, height),
                            pressed,
                        )?;
                        if let Some(target) = gesture {
                            // A card swiped up in the overview closes that application.
                            if let Some(window) = target
                                .strip_prefix("window:")
                                .and_then(|rest| rest.strip_suffix(":close"))
                                .and_then(|window| window.parse::<u64>().ok())
                            {
                                if !self
                                    .session(id)?
                                    .config
                                    .actions
                                    .iter()
                                    .any(|family| family == "application.v1")
                                {
                                    return Err(SimError::denied(
                                        "application interaction is not permitted",
                                    ));
                                }
                                self.machine_mut(id, machine)?
                                    .desktop
                                    .close(window)
                                    .map_err(SimError::invalid)?;
                                self.sync_desktop_visibility(id, machine)?;
                                return Ok(Value::Null);
                            }
                            return self.shell_action(id, machine, actor, &target);
                        }
                    }
                }
                if action.op == "down" && p.get("button").and_then(integer_u64) == Some(2) {
                    // A right-drag on an application's drag surface that uses it (a 3D
                    // view pans) belongs to the application, not the context menu.
                    let scene = self.scene(id, width, height)?;
                    let secondary = scene
                        .hit_test(x, y)
                        .and_then(|n| n.interaction.as_deref())
                        .and_then(|t| t.strip_prefix("window:"))
                        .and_then(|t| t.split_once(':'))
                        .and_then(|(w, op)| {
                            Some((
                                w.parse::<u64>().ok()?,
                                op.strip_prefix("content:")?.to_owned(),
                            ))
                        })
                        .is_some_and(|(w, inner)| {
                            self.session(id).is_ok_and(|s| {
                                s.machines[machine].desktop.app_takes_secondary(w, &inner)
                            })
                        });
                    if !secondary {
                        return self.shell_action(id, machine, actor, "shell:panel:context");
                    }
                }
                if action.op == "cancel" {
                    self.machine_mut(id, machine)?.desktop.pointer_capture = None;
                    return Ok(Value::Null);
                }
                if let Some(area) = area {
                    if matches!(action.op.as_str(), "move" | "up") {
                        let desktop = &mut self.machine_mut(id, machine)?.desktop;
                        let captured = if action.op == "move" {
                            desktop.pointer_move(x, y, area)
                        } else {
                            desktop.pointer_up(x, y, area)
                        }
                        .map_err(SimError::invalid)?;
                        if captured {
                            let cursor = self.session(id)?.machines[machine]
                                .desktop
                                .pointer_capture
                                .as_ref()
                                .map(|capture| cursor_for_target(&capture.operation, true))
                                .unwrap_or("default");
                            return Ok(json!({"cursor":cursor}));
                        }
                    }
                }
                let scene = self.scene(id, width, height)?;
                if action.op == "move" {
                    // A canvas that draws what is being placed under the pointer (a wire,
                    // a track) follows it even with no button down.
                    if let Some((window, target, bounds)) = scene.hit_test(x, y).and_then(|n| {
                        let (window, rest) = n
                            .interaction
                            .as_deref()?
                            .strip_prefix("window:")?
                            .split_once(':')?;
                        let target = rest.strip_prefix("content:")?;
                        Some((
                            window.parse::<u64>().ok()?,
                            target.to_owned(),
                            n.transform.bounds(n.bounds),
                        ))
                    }) {
                        if self.session(id)?.machines[machine]
                            .desktop
                            .app_hovers(window, &target)
                        {
                            self.machine_mut(id, machine)?.desktop.app_hover(
                                window,
                                &target,
                                x - bounds.x,
                                y - bounds.y,
                            );
                            return Ok(json!({"cursor":"crosshair"}));
                        }
                    }
                    return Ok(
                        json!({"cursor":scene.hit_test(x,y).and_then(|n|n.interaction.as_deref()).map(|target|cursor_for_target(target,false)).unwrap_or("default")}),
                    );
                }
                let (mut target, hit) = if action.op == "up" {
                    let Some((target, bounds)) = released_press else {
                        return Ok(Value::Null);
                    };
                    if !bounds.contains(x, y) {
                        return Ok(Value::Null);
                    }
                    (target, bounds)
                } else {
                    let Some(node) = scene.hit_test(x, y) else {
                        return Ok(Value::Null);
                    };
                    let Some(target) = node.interaction.clone() else {
                        return Ok(Value::Null);
                    };
                    let bounds = node.transform.bounds(node.bounds);
                    if action.op == "down" {
                        self.machine_mut(id, machine)?.pointer_press =
                            Some((target.clone(), bounds));
                    }
                    (target, bounds)
                };
                // Choosing an entry in a drop-down menu closes the menu, whatever the entry
                // does; clicks on the menu's own body or on another menu title do not.
                if !matches!(action.op.as_str(), "down" | "move" | "cancel")
                    && target != "shell:noop"
                    && !target.starts_with("shell:panel:")
                    && !target.starts_with("shell:gesture:")
                {
                    self.machine_mut(id, machine)?.desktop.close_menu();
                }
                let custom_prefix = format!("window:{}:content:", u64::MAX);
                if self.session(id)?.machines[machine].active_app.is_some()
                    && target.starts_with(&custom_prefix)
                {
                    if action.op == "down" {
                        return Ok(Value::Null);
                    }
                    return self.custom_event(id,machine,actor,&json!({"kind":"click","target":target.trim_start_matches(&custom_prefix),"data":p}));
                }
                if self.session(id)?.machines[machine].active_app.is_some()
                    && target.starts_with(&format!("window:{}:", u64::MAX))
                {
                    if action.op == "down" {
                        return Ok(Value::Null);
                    }
                    if target.ends_with(":close") || target.ends_with(":minimize") {
                        self.machine_mut(id, machine)?.active_app = None;
                        self.machine_mut(id, machine)?.custom_page = None;
                    }
                    return Ok(Value::Null);
                }
                if let Some(namespaced) = target.strip_prefix("window:") {
                    let (window, operation) = namespaced
                        .split_once(':')
                        .ok_or_else(|| SimError::invalid("invalid window interaction"))?;
                    let window: u64 = window
                        .parse()
                        .map_err(|_| SimError::invalid("invalid window id"))?;
                    let operation = operation.to_owned();
                    if !self
                        .session(id)?
                        .config
                        .actions
                        .iter()
                        .any(|family| family == "application.v1")
                    {
                        return Err(SimError::denied("application interaction is not permitted"));
                    }
                    // Pressing an application's drag surface captures the pointer at
                    // once, on a phone as on a desktop: a finger drawing on a canvas is a
                    // stroke, not a shell gesture.
                    if action.op == "down" {
                        if let Some(content) = operation.strip_prefix("content:") {
                            if self.session(id)?.machines[machine]
                                .desktop
                                .app_drags(window, content)
                            {
                                let content = content.to_owned();
                                self.machine_mut(id, machine)?.touch_start = None;
                                let button =
                                    p.get("button").and_then(integer_u64).unwrap_or(0).min(2) as u8;
                                let effects = self
                                    .machine_mut(id, machine)?
                                    .desktop
                                    .app_pointer_down_with(window, &content, x, y, hit, button)
                                    .map_err(SimError::invalid)?;
                                self.sync_desktop_visibility(id, machine)?;
                                self.effects(id, machine, actor, effects)?;
                                return Ok(Value::Null);
                            }
                        }
                    }
                    // A finger coming down only presses; the release decides, so a swipe
                    // that starts on a card or a control is still free to be a gesture.
                    if mobile && action.op == "down" {
                        return Ok(Value::Null);
                    }
                    self.machine_mut(id, machine)?
                        .desktop
                        .focus(window)
                        .map_err(SimError::invalid)?;
                    self.sync_desktop_visibility(id, machine)?;
                    let area = area
                        .ok_or_else(|| SimError::invalid("window interaction requires desktop"))?;
                    if action.op == "down" {
                        if !mobile && (operation == "drag" || operation.starts_with("resize:")) {
                            self.machine_mut(id, machine)?
                                .desktop
                                .pointer_down(window, &operation, x, y, area)
                                .map_err(SimError::invalid)?;
                        }
                        // A press in a text view anchors a drag selection; the release,
                        // delivered as the click, extends it to where the pointer let go.
                        if let Some(inner) = operation.strip_prefix("content:") {
                            self.machine_mut(id, machine)?
                                .desktop
                                .press_at(inner, x - hit.x, y - hit.y)
                                .map_err(SimError::invalid)?;
                        }
                        return Ok(Value::Null);
                    }
                    if operation == "drag" && action.op == "double_click" {
                        self.machine_mut(id, machine)?
                            .desktop
                            .maximize(window, area)
                            .map_err(SimError::invalid)?;
                        return Ok(Value::Null);
                    }
                    match operation.as_str() {
                        "maximize" => {
                            self.machine_mut(id, machine)?
                                .desktop
                                .maximize(window, area)
                                .map_err(SimError::invalid)?;
                            return Ok(Value::Null);
                        }
                        "minimize" => {
                            self.machine_mut(id, machine)?
                                .desktop
                                .minimize(window)
                                .map_err(SimError::invalid)?;
                            self.sync_desktop_visibility(id, machine)?;
                            return Ok(Value::Null);
                        }
                        "close" => {
                            self.machine_mut(id, machine)?
                                .desktop
                                .close(window)
                                .map_err(SimError::invalid)?;
                            self.sync_desktop_visibility(id, machine)?;
                            return Ok(Value::Null);
                        }
                        "focus" | "drag" => return Ok(Value::Null),
                        resize if resize.starts_with("resize:") => return Ok(Value::Null),
                        content => {
                            target = content
                                .strip_prefix("content:")
                                .ok_or_else(|| SimError::invalid("unknown window interaction"))?
                                .to_owned();
                        }
                    }
                } else if action.op == "down" {
                    return Ok(Value::Null);
                }
                // Pointers on a desktop select on the first click and open on the second;
                // a touch screen has no such distinction and opens immediately.
                let opening = mobile || action.op == "double_click";
                // A gesture affordance — the home indicator, a status bar — is reached by
                // dragging from it. Tapped, it does what the glass does: nothing.
                if target.starts_with("shell:gesture:") {
                    return Ok(Value::Null);
                }
                if target.starts_with("shell:") {
                    if let Some(kind) = target.strip_prefix("shell:open:") {
                        let kind = kind.to_owned();
                        if !opening {
                            self.machine_mut(id, machine)?.desktop.desktop_selection =
                                Some(kind.clone());
                            return Ok(Value::Null);
                        }
                        self.machine_mut(id, machine)?.desktop.desktop_selection = None;
                        // The Recycle Bin is a folder, not an application.
                        let open = if kind == "trash" {
                            "shell:trash".to_owned()
                        } else {
                            format!("shell:launch:{kind}")
                        };
                        return self.shell_action(id, machine, actor, &open);
                    }
                    if action.op == "double_click" {
                        return Ok(Value::Null);
                    }
                    self.machine_mut(id, machine)?.desktop.desktop_selection = None;
                    return self.shell_action(id, machine, actor, &target);
                }
                // Double clicks mean something to file-manager rows and to applications
                // that give them a meaning of their own (a code editor's tabs and words).
                if action.op == "double_click"
                    && !target.starts_with("open:")
                    && !target.starts_with("code:")
                    && !target.starts_with("freecad:")
                    && !target.starts_with("sheet:")
                    && !target.starts_with("db:")
                    && !target.starts_with("kicad:")
                {
                    return Ok(Value::Null);
                }
                self.machine_mut(id, machine)?.address_focused = false;
                if self.machine_mut(id, machine)?.active_app.is_some() {
                    self.custom_event(
                        id,
                        machine,
                        actor,
                        &json!({"kind":"click","target":target,"data":p}),
                    )
                } else if self.machine_mut(id, machine)?.browser_visible {
                    let a =
                        ActionEnvelope::new("browser.v1", "click", machine, json!({"id":target}));
                    self.browser_action(id, actor, &a)
                } else {
                    if let Some(index) = target
                        .strip_prefix("open:")
                        .and_then(|i| i.parse::<usize>().ok())
                    {
                        let desktop = &self.session(id)?.machines[machine].desktop;
                        if let Some(window) =
                            desktop.focused.and_then(|id| desktop.windows.get(&id))
                        {
                            if let Some(tab) = window.state.file_tab() {
                                if let Some(entry) = tab.entries.get(index).filter(|_| opening) {
                                    // Folders open in the same window; a document needs the
                                    // application that opens its kind: workbooks the
                                    // spreadsheet, databases the database client, the rest
                                    // the text editor.
                                    if !entry.ends_with('/')
                                        && !self
                                            .runtime
                                            .computer(machine)?
                                            .application_available(cw_applications::opener(entry))
                                    {
                                        return Err(SimError::not_found(
                                            "application is not installed",
                                        ));
                                    }
                                }
                            }
                        }
                    }
                    let desktop = &mut self.machine_mut(id, machine)?.desktop;
                    // A tap on a spreadsheet or database grid selects, as a click does;
                    // editing takes a second tap (a double click), as on the phones' own.
                    let grid = target.starts_with("sheet:") || target.starts_with("db:");
                    let effects = if opening && !(grid && action.op != "double_click") {
                        desktop.activate(&target)
                    } else {
                        desktop.click_at(&target, x - hit.x, y - hit.y)
                    }
                    .map_err(SimError::invalid)?;
                    self.effects(id, machine, actor, effects)?;
                    self.sync_desktop_visibility(id, machine)?;
                    Ok(Value::Null)
                }
            }
            _ => {
                if let Some(extension) = self.extensions.get(&action.family).cloned() {
                    extension.execute(&mut self.runtime, actor, action)
                } else {
                    Err(SimError::invalid("unsupported action operation"))
                }
            }
        }
    }
    fn browser_action(&mut self, id: &str, actor: &str, a: &ActionEnvelope) -> Result<Value> {
        if !self
            .session(id)?
            .config
            .actions
            .iter()
            .any(|family| family == "browser.v1")
        {
            return Err(SimError::denied("browser interaction is not permitted"));
        }
        let themed = self.desktop_theme(id, &a.machine).is_some();
        let runtime = &mut self.runtime;
        let machine = Arc::make_mut(&mut self.sessions)
            .get_mut(id)
            .and_then(|s| s.machines.get_mut(&a.machine))
            .ok_or_else(|| SimError::denied("machine unavailable"))?;
        machine.browser_visible = true;
        machine.address_focused = false;
        machine.desktop.launcher_open = false;
        if themed {
            if let Some(window) = machine
                .desktop
                .windows
                .values()
                .filter(|w| matches!(w.state, AppState::Browser { .. }))
                .max_by_key(|w| (machine.desktop.focused == Some(w.id), w.id))
                .map(|w| w.id)
            {
                machine.desktop.focus(window).map_err(SimError::invalid)?;
            } else {
                machine
                    .desktop
                    .launch("browser", "")
                    .map_err(SimError::invalid)?;
            }
        }
        if themed && machine.active_browser_window != machine.desktop.focused {
            if let Some(previous) = machine.active_browser_window {
                machine
                    .browser_windows
                    .insert(previous, std::mem::take(&mut machine.browser));
            }
            machine.browser = machine
                .desktop
                .focused
                .and_then(|window| machine.browser_windows.remove(&window))
                .unwrap_or_default();
            machine.active_browser_window = machine.desktop.focused;
        }
        machine.active_app = None;
        machine.custom_page = None;
        let mut http = |r| runtime.http(&a.machine, actor, r);
        match a.op.as_str() {
            "navigate" => machine
                .browser
                .navigate(string(&a.payload, "url")?, &mut http)?,
            "back" => machine.browser.back(&mut http)?,
            "forward" => machine.browser.forward(&mut http)?,
            "reload" => machine.browser.reload(&mut http)?,
            "fill" => {
                let input = string(&a.payload, "id")?;
                machine.browser.fill(input, string(&a.payload, "value")?)?;
                machine.focused_input = Some(input.into());
            }
            "key" => machine.browser.key(string(&a.payload, "key")?, &mut http)?,
            "new_tab" => {
                machine.browser.new_tab();
            }
            "switch_tab" | "close_tab" => {
                let tab = a
                    .payload
                    .get("tab")
                    .and_then(integer_u64)
                    .ok_or_else(|| SimError::invalid("tab required"))?
                    as usize;
                if a.op == "switch_tab" {
                    machine.browser.switch_tab(tab)?
                } else {
                    machine.browser.close_tab(tab)?
                }
            }
            "scroll" => {
                machine.browser.tab_mut().scroll_y =
                    a.payload
                        .get("y")
                        .and_then(integer_i64)
                        .unwrap_or(0)
                        .clamp(0, i32::MAX as i64) as i32;
            }
            "submit" => machine
                .browser
                .submit(string(&a.payload, "id")?, &mut http)?,
            "click" => {
                let target = string(&a.payload, "id")?;
                if page_has_input(machine.browser.page(), target) {
                    machine.focused_input = Some(target.into());
                    machine.browser.click(target, &mut http)?;
                } else {
                    machine.focused_input = None;
                    machine.browser.click(target, &mut http)?;
                }
            }
            _ => return Err(SimError::invalid("unsupported browser operation")),
        };
        machine.focused_input = machine.browser.tab().focused.clone();
        if let Some(window) = machine
            .desktop
            .focused
            .and_then(|id| machine.desktop.windows.get_mut(&id))
        {
            if let AppState::Browser { address } = &mut window.state {
                *address = machine.browser.url().unwrap_or("").to_string();
            }
        }
        Ok(serde_json::to_value(active_page(machine))?)
    }
    fn effects(
        &mut self,
        id: &str,
        machine: &str,
        actor: &str,
        effects: Vec<cw_applications::AppEffect>,
    ) -> Result<()> {
        use cw_applications::AppEffect::*;
        let mut pending: std::collections::VecDeque<_> = effects.into();
        let mut budget = 0u32;
        while let Some(effect) = pending.pop_front() {
            budget += 1;
            if budget > 64 {
                return Err(SimError::invalid("application effect budget exceeded"));
            }
            effect.validate().map_err(SimError::invalid)?;
            match effect {
                ReadFile { window, path } => {
                    let content = String::from_utf8_lossy(&self.runtime.read_file(machine, &path)?)
                        .into_owned();
                    self.machine_mut(id, machine)?
                        .desktop
                        .file_loaded(window, content)
                        .map_err(SimError::invalid)?;
                }
                WriteFile {
                    window,
                    path,
                    content,
                } => {
                    self.runtime
                        .write_file(machine, actor, &path, content.as_bytes())?;
                    let more = self
                        .machine_mut(id, machine)?
                        .desktop
                        .file_written(window, &path, &content)
                        .map_err(SimError::invalid)?;
                    pending.extend(more);
                }
                ListTree {
                    window,
                    path,
                    depth,
                } => {
                    let result = list_tree(self.runtime.computer(machine)?, &path, depth);
                    let more = self
                        .machine_mut(id, machine)?
                        .desktop
                        .tree_listed(window, &path, depth, result)
                        .map_err(SimError::invalid)?;
                    pending.extend(more);
                }
                ReadFiles { window, tag, paths } => {
                    // Each file answers for itself: one unreadable file is that file's
                    // problem, not the whole request's.
                    let mut budget_bytes = READ_FILES_BYTES;
                    let files = paths
                        .into_iter()
                        .take(READ_FILES_LIMIT)
                        .map(|path| {
                            let result = self
                                .runtime
                                .read_file(machine, &path)
                                .map_err(|e| actor_error(e).message)
                                .and_then(|bytes| {
                                    if bytes.len() > budget_bytes {
                                        return Err("the file is too large to open".into());
                                    }
                                    budget_bytes -= bytes.len();
                                    if bytes.contains(&0) {
                                        return Err("the file is binary".into());
                                    }
                                    String::from_utf8(bytes)
                                        .map_err(|_| "the file is not UTF-8 text".to_owned())
                                });
                            (path, result)
                        })
                        .collect();
                    let more = self
                        .machine_mut(id, machine)?
                        .desktop
                        .files_read(window, &tag, files)
                        .map_err(SimError::invalid)?;
                    pending.extend(more);
                }
                ShellRun {
                    window,
                    tag,
                    cwd,
                    command,
                } => {
                    let prompt_at = |env: &Self, dir: &str| -> Result<String> {
                        let c = env.runtime.computer(machine)?;
                        Ok(cw_applications::shell_prompt(
                            &c.user, &c.id, dir, &c.dialect,
                        ))
                    };
                    let before = self.runtime.computer(machine)?.resolve(&cwd);
                    let prompt = prompt_at(self, &before)?;
                    let outcome = if command.trim().is_empty() {
                        cw_applications::ShellOutcome {
                            entry: None,
                            prompt,
                            cwd: before,
                            clear: false,
                        }
                    } else {
                        match self.runtime.execute_in(machine, actor, &cwd, &command) {
                            Ok((result, after)) => {
                                let entry = cw_applications::TerminalEntry::new(
                                    &prompt,
                                    command,
                                    &result.stdout,
                                    &result.stderr,
                                    result.exit_code,
                                );
                                let clear = result.clear;
                                self.machine_mut(id, machine)?.terminal =
                                    serde_json::to_value(&result)?;
                                cw_applications::ShellOutcome {
                                    entry: Some(entry),
                                    prompt: prompt_at(self, &after)?,
                                    cwd: after,
                                    clear,
                                }
                            }
                            // The session's folder is gone: the shell says so, as a
                            // shell started in a deleted directory would.
                            Err(e) => cw_applications::ShellOutcome {
                                entry: Some(cw_applications::TerminalEntry::new(
                                    &prompt,
                                    command,
                                    "",
                                    &format!("shell: {}\n", actor_error(e).message),
                                    1,
                                )),
                                prompt,
                                cwd: before,
                                clear: false,
                            },
                        }
                    };
                    let more = self
                        .machine_mut(id, machine)?
                        .desktop
                        .shell_ran(window, &tag, outcome)
                        .map_err(SimError::invalid)?;
                    pending.extend(more);
                }
                CopyText { text, .. } => {
                    self.machine_mut(id, machine)?
                        .desktop
                        .copy_text(&text)
                        .map_err(SimError::invalid)?;
                }
                // The desktop resolves a paste against its clipboard before the effect
                // leaves it; one that reaches here has nothing left to do.
                Paste { .. } => {}
                ListDirectory { window, tab, path } => {
                    let c = self.runtime.computer(machine)?;
                    let base = c.resolve(&path);
                    let readable = c
                        .vfs
                        .check_access(&base, &c.user, true, false, true)
                        .is_ok();
                    let listing = if readable {
                        c.vfs.list(&base).ok()
                    } else {
                        None
                    };
                    let Some(listing) = listing else {
                        // An application that cannot read a folder shows that it cannot,
                        // rather than failing the click that opened it.
                        let reason = if readable {
                            "folder not found"
                        } else {
                            "folder access denied"
                        };
                        if self
                            .machine_mut(id, machine)?
                            .desktop
                            .directory_failed(window, reason)
                            .map_err(SimError::invalid)?
                        {
                            continue;
                        }
                        return Err(if readable {
                            SimError::not_found("directory")
                        } else {
                            SimError::denied("directory access denied")
                        });
                    };
                    let entries = listing
                        .into_iter()
                        .map(|name| {
                            if c.vfs
                                .stat(&format!("{}/{}", base.trim_end_matches('/'), name))
                                .is_ok_and(|m| m.is_dir)
                            {
                                format!("{name}/")
                            } else {
                                name
                            }
                        })
                        .collect();
                    self.machine_mut(id, machine)?
                        .desktop
                        .directory_loaded(window, tab, entries)
                        .map_err(SimError::invalid)?;
                    // A photo library asks for the pixels of whatever the listing put on
                    // screen; nothing else needs decoding, so nothing else asks.
                    if let Some(cw_applications::AppState::Native(
                        cw_applications::NativeApp::Photos(photos),
                    )) = self.session(id)?.machines[machine]
                        .desktop
                        .windows
                        .get(&window)
                        .map(|w| &w.state)
                    {
                        pending.extend(photos.undecoded(window));
                    }
                }
                Execute { window, command } => {
                    // The prompt is captured before the command runs, so `cd` is echoed
                    // under the directory it was typed in, not the one it moved to.
                    let prompt = self.machine_mut(id, machine)?.desktop.prompt.clone();
                    let result = self.runtime.execute(machine, actor, &command)?;
                    let entry = cw_applications::TerminalEntry::new(
                        &prompt,
                        command,
                        &result.stdout,
                        &result.stderr,
                        result.exit_code,
                    );
                    let clear = result.clear;
                    let next = {
                        let c = self.runtime.computer(machine)?;
                        let home = c.env.get("HOME").map_or("", String::as_str);
                        cw_applications::shell_prompt_at(
                            &c.user,
                            &c.id,
                            &c.cwd,
                            home,
                            prompt_dialect(&c.os_family, &c.dialect),
                        )
                    };
                    let desktop = &mut self.machine_mut(id, machine)?.desktop;
                    desktop.prompt = next;
                    if clear {
                        desktop.terminal_clear(window).map_err(SimError::invalid)?;
                    } else {
                        desktop
                            .terminal_output(window, entry)
                            .map_err(SimError::invalid)?;
                    }
                    self.machine_mut(id, machine)?.terminal = serde_json::to_value(result)?;
                }
                Navigate { window: _, url } => {
                    self.browser_action(
                        id,
                        actor,
                        &ActionEnvelope::new("browser.v1", "navigate", machine, json!({"url":url})),
                    )?;
                }
                Http {
                    window,
                    tag,
                    method,
                    url,
                    body,
                } => {
                    // Applications reach services through the same gateway as the browser.
                    if !self
                        .session(id)?
                        .config
                        .actions
                        .iter()
                        .any(|family| family == "browser.v1")
                    {
                        return Err(SimError::denied(
                            "network application interaction is not permitted",
                        ));
                    }
                    let request = HttpRequest {
                        method,
                        url,
                        headers: BTreeMap::from([(
                            "content-type".into(),
                            "application/json".into(),
                        )]),
                        body: body.into_bytes(),
                    };
                    match self.runtime.http(machine, actor, request) {
                        Ok(response) => {
                            let text = String::from_utf8_lossy(&response.body).into_owned();
                            let more = self
                                .machine_mut(id, machine)?
                                .desktop
                                .http_response(window, &tag, response.status, &text)
                                .map_err(SimError::invalid)?;
                            pending.extend(more);
                        }
                        // A transport failure is application state, not an action failure:
                        // otherwise an actor could map the gateway by watching clicks fail.
                        Err(e) => self
                            .machine_mut(id, machine)?
                            .desktop
                            .http_failed(window, &tag, &actor_error(e).message)
                            .map_err(SimError::invalid)?,
                    }
                }
                CreateDirectory { window: _, path } => {
                    self.runtime.create_directory(machine, actor, &path)?;
                    // The refresh is the caller's: it queues a listing of the folder it
                    // is actually showing. Listing the new folder here instead put the
                    // wrong contents in a file manager's tab, because the folder that
                    // was created is not usually the folder that is on screen.
                }
                // Mutations go through the kernel, which applies the same access checks
                // `read_file` and `write_file` do: a file manager is not a way around a
                // permission a shell would have refused. Each of these is followed by a
                // `ListDirectory` the application queued, so the folder on screen is
                // the machine's answer and never an assumption about what happened.
                CreateFile { window: _, path } => {
                    self.runtime.create_file(machine, actor, &path)?;
                }
                CopyPath {
                    window: _,
                    from,
                    to,
                } => self.runtime.copy_path(machine, actor, &from, &to)?,
                MovePath {
                    window: _,
                    from,
                    to,
                } => self.runtime.move_path(machine, actor, &from, &to)?,
                // Delete is a move into the machine's trash; nothing is hard-removed.
                TrashPath {
                    window: _,
                    path,
                    trash,
                } => {
                    self.runtime.trash_path(machine, actor, &path, &trash)?;
                }
                Download { window, url } => {
                    if !self
                        .session(id)?
                        .config
                        .actions
                        .iter()
                        .any(|family| family == "browser.v1")
                    {
                        return Err(SimError::denied("network access is not permitted"));
                    }
                    // Fetched through the same gateway the browser uses, then written to
                    // the machine: a download that does not leave a file is a pretence.
                    let request = HttpRequest {
                        method: "GET".into(),
                        url: url.clone(),
                        headers: BTreeMap::new(),
                        body: vec![],
                    };
                    let response = self.runtime.http(machine, actor, request)?;
                    let name = download_name(&url);
                    let home = self.session(id)?.machines[machine].desktop.home_folder();
                    let folder = format!("{}/Downloads", home.trim_end_matches('/'));
                    self.runtime.create_directory(machine, actor, &folder)?;
                    let path = format!("{folder}/{name}");
                    let bytes = response.body.len() as u64;
                    self.runtime
                        .write_file(machine, actor, &path, &response.body)?;
                    self.machine_mut(id, machine)?
                        .desktop
                        .record_download(&name, &path, &url, bytes);
                    let _ = window;
                }
                Screenshot { window, path } => {
                    if !self
                        .session(id)?
                        .config
                        .observations
                        .iter()
                        .any(|c| c == "pixels.v1")
                    {
                        return Err(SimError::denied("pixel capture is not permitted"));
                    }
                    // The screen is rasterised from the same scene an observer sees, so a
                    // screenshot cannot show something the actor could not.
                    let scene = self.scene(id, 1280, 800)?;
                    let png = self
                        .capture
                        .clone()
                        .ok_or_else(|| {
                            SimError::invalid("this build has no rasterizer to capture with")
                        })?
                        .png(&scene)
                        .map_err(SimError::invalid)?;
                    let home = self.session(id)?.machines[machine].desktop.home_folder();
                    let path = if path.is_empty() {
                        format!("{}/Pictures", home.trim_end_matches('/'))
                    } else {
                        path
                    };
                    self.runtime.create_directory(machine, actor, &path)?;
                    let tick = self.runtime.tick();
                    let file = format!("{path}/screen-{tick}.png");
                    self.runtime.write_file(machine, actor, &file, &png)?;
                    self.machine_mut(id, machine)?.desktop.notify(
                        "screenshot",
                        "Screenshot saved",
                        &file,
                        Some("shell:launch:files".into()),
                    );
                    let _ = window;
                }
                ReadBytes { window, path } => {
                    // A document the machine cannot give (missing, unreadable) is the
                    // application's to report, not a failure of the click that asked.
                    let result = self.runtime.read_file(machine, &path).map_err(file_problem);
                    let more = self
                        .machine_mut(id, machine)?
                        .desktop
                        .bytes_loaded(window, &path, result)
                        .map_err(SimError::invalid)?;
                    pending.extend(more);
                }
                WriteBytes {
                    window,
                    path,
                    bytes,
                } => {
                    let result = self
                        .runtime
                        .write_file(machine, actor, &path, &bytes)
                        .map_err(file_problem);
                    let more = self
                        .machine_mut(id, machine)?
                        .desktop
                        .bytes_saved(window, &path, result)
                        .map_err(SimError::invalid)?;
                    pending.extend(more);
                }
                ReadImage { window, path } => {
                    // A picture is only shown if the machine really holds one and this
                    // build can decode it; otherwise the application says why.
                    let outcome = self
                        .capture
                        .clone()
                        .ok_or_else(|| "this build cannot decode images".to_owned())
                        .and_then(|raster| {
                            let bytes = self
                                .runtime
                                .read_file(machine, &path)
                                .map_err(|e| actor_error(e).message)?;
                            raster.decode(&bytes)
                        });
                    match outcome {
                        Ok((width, height, rgba)) => {
                            let more = self
                                .machine_mut(id, machine)?
                                .desktop
                                .image_loaded(window, &path, width, height, rgba);
                            more.map_err(SimError::invalid)?;
                        }
                        Err(reason) => self
                            .machine_mut(id, machine)?
                            .desktop
                            .image_failed(window, &path, &reason)
                            .map_err(SimError::invalid)?,
                    }
                }
                WriteImage {
                    window,
                    path,
                    width,
                    height,
                    rgba,
                } => {
                    // Encoding needs the rasterizer's codec; the application only drew.
                    let png = self
                        .capture
                        .clone()
                        .ok_or_else(|| SimError::invalid("this build cannot encode images"))?
                        .encode(width, height, &rgba)
                        .map_err(SimError::invalid)?;
                    self.runtime.write_file(machine, actor, &path, &png)?;
                    let more = self
                        .machine_mut(id, machine)?
                        .desktop
                        .image_saved(window, &path)
                        .map_err(SimError::invalid)?;
                    pending.extend(more);
                }
                RasterText {
                    window,
                    text,
                    size,
                    bold,
                } => {
                    // The text tool stamps exactly what the renderer draws: the line is
                    // rendered white on black in this platform's font and the red channel
                    // becomes the glyph coverage.
                    let typeface = self
                        .desktop_theme(id, machine)
                        .map(DesktopTheme::typeface)
                        .unwrap_or_default();
                    let size = size.clamp(6, 400);
                    let width = cw_scene::metrics::text_width(typeface, bold, &text, size) + 4;
                    let height = u32::from(size) + u32::from(size) / 2 + 4;
                    let outcome = if width > 8192 {
                        Err("that text is too wide to draw".to_owned())
                    } else {
                        let mut scene = Scene::new(width, height);
                        scene.background = cw_scene::Color::BLACK;
                        scene.typeface = typeface;
                        let bounds = cw_scene::Rect::new(1, 1, width - 2, height - 2);
                        scene.nodes.push(if bold {
                            cw_scene::Node::ui_text_bold(
                                1,
                                bounds,
                                text,
                                size,
                                cw_scene::Color::WHITE,
                            )
                        } else {
                            cw_scene::Node::ui_text(1, bounds, text, size, cw_scene::Color::WHITE)
                        });
                        self.capture
                            .clone()
                            .ok_or_else(|| "this build cannot draw text into images".to_owned())
                            .and_then(|raster| raster.pixels(&scene))
                    };
                    let desktop = &mut self.machine_mut(id, machine)?.desktop;
                    match outcome {
                        Ok((w, h, rgba)) => desktop
                            .text_rasterized(window, w, h, rgba.chunks(4).map(|p| p[0]).collect())
                            .map_err(SimError::invalid)?,
                        Err(reason) => desktop
                            .image_failed(window, cw_applications::TEXT_IMAGE, &reason)
                            .map_err(SimError::invalid)?,
                    }
                }
                CopyImage {
                    window: _,
                    width,
                    height,
                    rgba,
                } => {
                    if u64::from(width) * u64::from(height) > 4096 * 4096 {
                        return Err(SimError::invalid("that is too large to copy"));
                    }
                    let picture = cw_raster::Canvas::from_rgba(width, height, rgba)
                        .map_err(SimError::invalid)?;
                    self.machine_mut(id, machine)?.desktop.clipboard =
                        Some(cw_applications::Clipboard::picture(picture));
                }
                PasteImage { window } => {
                    let picture = self.session(id)?.machines[machine]
                        .desktop
                        .clipboard
                        .as_ref()
                        .and_then(|c| c.image.clone());
                    let desktop = &mut self.machine_mut(id, machine)?.desktop;
                    match picture {
                        Some(picture) => desktop
                            .image_loaded(
                                window,
                                cw_applications::CLIPBOARD_IMAGE,
                                picture.width(),
                                picture.height(),
                                picture.into_pixels(),
                            )
                            .map_err(SimError::invalid)?,
                        None => desktop
                            .image_failed(
                                window,
                                cw_applications::CLIPBOARD_IMAGE,
                                "The clipboard holds no picture",
                            )
                            .map_err(SimError::invalid)?,
                    }
                }
                Launch {
                    window: _,
                    kind,
                    argument,
                } => {
                    self.dispatch(
                        id,
                        actor,
                        &ActionEnvelope::new(
                            "application.v1",
                            "launch",
                            machine,
                            json!({"kind":kind,"argument":argument}),
                        ),
                    )?;
                }
            }
        }
        Ok(())
    }
    fn desktop_theme(&self, id: &str, machine: &str) -> Option<DesktopTheme> {
        let session = self.session(id).ok()?;
        if !session
            .config
            .actions
            .iter()
            .any(|family| family == "application.v1")
        {
            return None;
        }
        let definition = self.runtime.definition();
        let computer = definition
            .computers
            .iter()
            .find(|computer| computer.id == machine)?;
        let configured = definition
            .metadata
            .get("desktop_themes")
            .and_then(|themes| themes.get(machine))
            .and_then(Value::as_str);
        // Presentation is opt-in, so adding OS chrome does not alter browser-only or
        // legacy structured-app layouts and their coordinate contracts.
        if configured.is_none() && !computer.profile.starts_with("virtual-") {
            return None;
        }
        DesktopTheme::from_profile(configured.unwrap_or(&computer.profile))
    }
    fn sync_desktop_visibility(&mut self, id: &str, machine: &str) -> Result<()> {
        let state = self.machine_mut(id, machine)?;
        state.browser_visible = state
            .desktop
            .focused
            .and_then(|id| state.desktop.windows.get(&id))
            .is_some_and(|w| matches!(w.state, AppState::Browser { .. }));
        if state.browser_visible {
            let target = state.desktop.focused;
            if state.active_browser_window != target {
                if let Some(previous) = state.active_browser_window {
                    state
                        .browser_windows
                        .insert(previous, std::mem::take(&mut state.browser));
                }
                state.browser = target
                    .and_then(|window| state.browser_windows.remove(&window))
                    .unwrap_or_default();
                state.active_browser_window = target;
            }
        }
        state
            .browser_windows
            .retain(|window, _| state.desktop.windows.contains_key(window));
        if state
            .active_browser_window
            .is_some_and(|window| !state.desktop.windows.contains_key(&window))
        {
            state.active_browser_window = None;
            state.browser = BrowserState::default();
        }
        state.address_focused = false;
        state.active_app = None;
        state.custom_page = None;
        Ok(())
    }
    fn shell_action(
        &mut self,
        id: &str,
        machine: &str,
        actor: &str,
        target: &str,
    ) -> Result<Value> {
        if !self
            .session(id)?
            .config
            .actions
            .iter()
            .any(|family| family == "application.v1")
        {
            return Err(SimError::denied("application interaction is not permitted"));
        }
        if let Some(result) = self.desktop_panel_action(id, machine, actor, target)? {
            return Ok(result);
        }
        // Insert a whole word, for a suggestion chip or a paste control. One character
        // at a time is `shell:type:`; this is the same pipeline, not a second one.
        if let Some(text) = target.strip_prefix("shell:insert:") {
            if text.is_empty() || text.chars().any(char::is_control) || text.len() > 256 {
                return Err(SimError::invalid("insertable text must be one short line"));
            }
            let text = text.to_owned();
            // An insert chosen from a menu (Notepad's Time/Date) closes the menu first,
            // so the text lands in the document rather than in an open panel.
            self.machine_mut(id, machine)?.desktop.close_menu();
            return self.dispatch(
                id,
                actor,
                &ActionEnvelope::new("keyboard.v1", "type", machine, json!({ "text": text })),
            );
        }
        if let Some(kind) = target.strip_prefix("shell:launch:") {
            // `shell:launch:<kind>/<argument>` opens an application on something: a
            // calendar on a date, a file manager on a folder.
            if let Some((kind, argument)) = kind.split_once('/') {
                let (kind, argument) = (kind.to_owned(), argument.to_owned());
                if !self.runtime.computer(machine)?.application_available(&kind) {
                    return Err(SimError::not_found("application is not installed"));
                }
                return self.dispatch(
                    id,
                    actor,
                    &ActionEnvelope::new(
                        "application.v1",
                        "launch",
                        machine,
                        json!({ "kind": kind, "argument": argument }),
                    ),
                );
            }
            if self.desktop_alias(id, machine, kind)?.is_none()
                && !self.runtime.computer(machine)?.application_available(kind)
            {
                return Err(SimError::not_found("application is not installed"));
            }
            let existing = self.session(id)?.machines[machine]
                .desktop
                .windows
                .values()
                .find(|window| {
                    window.app_id == kind
                        || (window.app_id.is_empty()
                            && matches!(
                                (&window.state, kind),
                                (AppState::Browser { .. }, "browser")
                                    | (AppState::Terminal { .. }, "terminal")
                                    | (AppState::Editor { .. }, "editor")
                                    | (AppState::Files { .. }, "files")
                            ))
                })
                .map(|window| window.id);
            if let Some(window) = existing {
                self.machine_mut(id, machine)?
                    .desktop
                    .focus(window)
                    .map_err(SimError::invalid)?;
                self.sync_desktop_visibility(id, machine)?;
                return Ok(json!({"window":window}));
            }
            return self.dispatch(
                id,
                actor,
                &ActionEnvelope::new("application.v1", "launch", machine, json!({"kind":kind})),
            );
        }
        // On-screen keyboards. A painted key dispatches the same input an actor's own
        // keyboard action would, so the two paths can never diverge.
        // Keyboard modifiers live on the desktop so a painted shift key is a real one.
        if target == "shell:key:Shift" {
            let keyboard = &mut self.machine_mut(id, machine)?.desktop.keyboard;
            keyboard.cycle_shift();
            return Ok(json!({ "shift": keyboard.shift }));
        }
        if let Some(plane) = target.strip_prefix("shell:plane:") {
            let plane = match plane {
                "letters" => cw_applications::Plane::Letters,
                "numbers" => cw_applications::Plane::Numbers,
                "symbols" => cw_applications::Plane::Symbols,
                _ => return Err(SimError::invalid("unknown keyboard plane")),
            };
            let keyboard = &mut self.machine_mut(id, machine)?.desktop.keyboard;
            keyboard.set_plane(plane);
            return Ok(json!({ "plane": plane }));
        }
        if let Some(text) = target.strip_prefix("shell:type:") {
            if text.chars().count() != 1 || text.chars().any(char::is_control) {
                return Err(SimError::invalid("a key types exactly one character"));
            }
            // A letter key types what the modifier says, and spends a one-shot shift.
            let text = match text.chars().next() {
                Some(ch) if ch.is_alphabetic() => {
                    self.machine_mut(id, machine)?.desktop.keyboard.apply(ch)
                }
                _ => text.to_owned(),
            };
            return self.dispatch(
                id,
                actor,
                &ActionEnvelope::new("keyboard.v1", "type", machine, json!({ "text": text })),
            );
        }
        if let Some(key) = target.strip_prefix("shell:key:") {
            let key = key.to_owned();
            return self.dispatch(
                id,
                actor,
                &ActionEnvelope::new("keyboard.v1", "key", machine, json!({ "key": key })),
            );
        }
        // Browser tab strip. Tabs already exist in the browser session; these are the
        // controls that reach them.
        if let Some(rest) = target.strip_prefix("shell:tab:") {
            let (op, payload) = match rest.split_once(':') {
                Some(("select", index)) => (
                    "switch_tab",
                    json!({"tab":index.parse::<u64>().map_err(|_| SimError::invalid("invalid tab"))?}),
                ),
                Some(("close", index)) => (
                    "close_tab",
                    json!({"tab":index.parse::<u64>().map_err(|_| SimError::invalid("invalid tab"))?}),
                ),
                None if rest == "new" => ("new_tab", json!({})),
                _ => return Err(SimError::invalid("unknown tab interaction")),
            };
            return self.browser_action(
                id,
                actor,
                &ActionEnvelope::new("browser.v1", op, machine, payload),
            );
        }
        if matches!(target, "shell:back" | "shell:forward" | "shell:reload") {
            if !self
                .session(id)?
                .config
                .actions
                .iter()
                .any(|family| family == "browser.v1")
            {
                return Err(SimError::denied("browser action is not permitted"));
            }
            return self.browser_action(
                id,
                actor,
                &ActionEnvelope::new(
                    "browser.v1",
                    target.trim_start_matches("shell:"),
                    machine,
                    json!({}),
                ),
            );
        }
        let state = self.machine_mut(id, machine)?;
        match target {
            "shell:launcher" => {
                state.desktop.launcher_open = !state.desktop.launcher_open;
                return Ok(Value::Null);
            }
            "shell:maximize" => {
                if let Some(window) = state.desktop.focused {
                    state
                        .desktop
                        .maximize(window, cw_scene::Rect::new(0, 28, 1024, 660))
                        .map_err(SimError::invalid)?;
                }
                return Ok(Value::Null);
            }
            "shell:home" => state.desktop.home(),
            "shell:minimize" => {
                if let Some(window) = state.desktop.focused {
                    state.desktop.minimize(window).map_err(SimError::invalid)?;
                }
            }
            "shell:close" => {
                if let Some(window) = state.desktop.focused {
                    state.desktop.close(window).map_err(SimError::invalid)?;
                }
            }
            "shell:switcher" => state.desktop.cycle().map_err(SimError::invalid)?,
            "shell:address" => {
                if state.browser_visible {
                    state.address_focused = true;
                    if let Some(window) = state
                        .desktop
                        .focused
                        .and_then(|id| state.desktop.windows.get_mut(&id))
                    {
                        if let AppState::Browser { address } = &mut window.state {
                            address.clear();
                        }
                    }
                }
                return Ok(Value::Null);
            }
            _ => return Err(SimError::invalid("unknown shell interaction")),
        }
        self.sync_desktop_visibility(id, machine)?;
        Ok(Value::Null)
    }
    pub fn scene(&self, id: &str, width: u32, height: u32) -> Result<Scene> {
        if width == 0 || height == 0 || u64::from(width) * u64::from(height) > 16_777_216 {
            return Err(SimError::invalid("invalid viewport"));
        }
        let s = self.session(id)?;
        if !s
            .config
            .observations
            .iter()
            .any(|c| c == "semantic.v1" || c == "pixels.v1")
        {
            return Err(SimError::denied("visual observation is not permitted"));
        }
        let m = s
            .machines
            .get(&s.focused_machine)
            .ok_or_else(|| SimError::denied("machine unavailable"))?;
        if let Some(theme) = self.desktop_theme(id, &s.focused_machine) {
            let area = work_area(theme, width, height);
            // What a Share control inside a window would hand things to, resolved the
            // same way `shell:share` resolves it.
            let share_to = self
                .runtime
                .computer(&s.focused_machine)
                .ok()
                .and_then(|c| {
                    ["chat", "mail"]
                        .into_iter()
                        .find(|k| c.application_available(k))
                });
            // The platform's own image editor, first one installed, for Photos' Edit.
            let editor = self
                .runtime
                .computer(&s.focused_machine)
                .ok()
                .and_then(|c| {
                    let kinds: &[&'static str] = match theme {
                        DesktopTheme::Windows => &["paint"],
                        DesktopTheme::Macos => &["preview", "pixelmator"],
                        DesktopTheme::Ubuntu => &["gimp", "pinta"],
                        DesktopTheme::Ios | DesktopTheme::Android => &[],
                    };
                    kinds.iter().copied().find(|k| c.application_available(k))
                });
            // The places a file manager's sidebar may offer, read from the machine now.
            let home = m.desktop.home_folder();
            let trash = m.desktop.trash_folder();
            let files_env = cw_applications::FilesEnv {
                home: &home,
                folders: self
                    .runtime
                    .computer(&s.focused_machine)
                    .map(|c| {
                        cw_applications::standard_folders(theme)
                            .iter()
                            .filter(|name| {
                                c.vfs
                                    .stat(&format!("{}/{name}", home.trim_end_matches('/')))
                                    .is_ok_and(|m| m.is_dir)
                            })
                            .map(|name| (*name).to_owned())
                            .collect()
                    })
                    .unwrap_or_default(),
                trash: trash.clone(),
                starred: &m.desktop.starred,
            };
            let mut views = Vec::new();
            for window_id in m.desktop.ordered_windows() {
                let window = &m.desktop.windows[&window_id];
                // A window on another virtual desktop is elsewhere, not minimised: it is
                // not composed, and a frame never sees it.
                if window.workspace != m.desktop.workspace {
                    continue;
                }
                // Phones present every application edge to edge; retained desktop
                // frames only apply to freely positioned windows.
                let rect = if theme.mobile() {
                    area
                } else {
                    m.desktop.effective_frame(window_id, area)
                };
                let kind = match &window.state {
                    AppState::Browser { .. } => "browser",
                    AppState::Files { .. } => "files",
                    AppState::Editor { .. } => "editor",
                    AppState::Terminal { .. } => "terminal",
                    AppState::Native(app) => app.kind(),
                };
                let content_rect = window_content_rect_for_kind(theme, rect, kind);
                let content = if kind == "browser" {
                    let browser = if m.active_browser_window == Some(window_id) {
                        &m.browser
                    } else {
                        m.browser_windows.get(&window_id).unwrap_or(&m.browser)
                    };
                    browser.scene(content_rect.width.max(1), content_rect.height.max(1))
                } else {
                    cw_applications::desktop_scene::app_content_with(
                        &window.state,
                        &cw_applications::AppEnv {
                            theme,
                            width: content_rect.width.max(1),
                            height: content_rect.height.max(1),
                            clock_us: m.desktop.clock_us,
                            settings: &m.desktop.settings,
                            clipboard: m.desktop.clipboard.as_ref(),
                            share_to,
                            files: files_env.clone(),
                            editor,
                            // Only the window in front sees the pointer, and only over
                            // its own content.
                            pointer: m
                                .pointer_position
                                .filter(|_| m.desktop.focused == Some(window_id))
                                .filter(|(x, y)| content_rect.contains(*x, *y))
                                .map(|(x, y)| (x - content_rect.x, y - content_rect.y)),
                        },
                    )
                };
                let (document, caption, modified) = match &window.state {
                    AppState::Browser { address } => {
                        let browser = if m.active_browser_window == Some(window_id) {
                            &m.browser
                        } else {
                            m.browser_windows.get(&window_id).unwrap_or(&m.browser)
                        };
                        (
                            browser.url().unwrap_or(address).to_owned(),
                            browser.page().map(|p| p.title.clone()).unwrap_or_default(),
                            false,
                        )
                    }
                    // A list, a view or the Trash is named, not traced as a path.
                    AppState::Files { .. } => {
                        let caption = window
                            .state
                            .file_tab()
                            .filter(|tab| tab.is_place(&trash))
                            .map(|tab| tab.title(theme, &home, &trash))
                            .unwrap_or_default();
                        (window.state.file_path().to_owned(), caption, false)
                    }
                    AppState::Editor { path, dirty, .. } => (path.clone(), String::new(), *dirty),
                    // The prompt names the user, the host and the directory; a frame titles
                    // the window from it the way each platform's terminal does.
                    AppState::Terminal { prompt, .. } => (String::new(), prompt.clone(), false),
                    AppState::Native(app) => (app.document(), app.caption(), app.modified()),
                };
                let title = match &window.state {
                    AppState::Browser { address } => format!("{} — {}", window.title, address),
                    AppState::Native(app) => app.title(theme),
                    _ => window.title.clone(),
                };
                // Tab strips are drawn by the window frame, so the labels travel with the view.
                let (tabs, active_tab) = match &window.state {
                    AppState::Browser { .. } => {
                        let browser = if m.active_browser_window == Some(window_id) {
                            &m.browser
                        } else {
                            m.browser_windows.get(&window_id).unwrap_or(&m.browser)
                        };
                        (
                            browser
                                .tabs
                                .iter()
                                .map(|tab| match tab.history.get(tab.position) {
                                    Some(entry) if !entry.page.title.is_empty() => {
                                        entry.page.title.clone()
                                    }
                                    _ => "New tab".into(),
                                })
                                .collect(),
                            browser.active,
                        )
                    }
                    AppState::Files { tabs, active } => (
                        tabs.iter()
                            .map(|tab| tab.title(theme, &home, &trash))
                            .collect(),
                        *active,
                    ),
                    _ => (vec![], 0),
                };
                let (can_go_back, can_go_forward) = match &window.state {
                    AppState::Browser { .. } => {
                        let browser = if m.active_browser_window == Some(window_id) {
                            &m.browser
                        } else {
                            m.browser_windows.get(&window_id).unwrap_or(&m.browser)
                        };
                        let tab = browser.tab();
                        (tab.position > 0, tab.position + 1 < tab.history.len())
                    }
                    AppState::Files { .. } => window
                        .state
                        .file_tab()
                        .map(|tab| (tab.can_go_back(), tab.can_go_forward()))
                        .unwrap_or((false, false)),
                    _ => (false, false),
                };
                let (dark_chrome, chrome) = match &window.state {
                    AppState::Native(app) => (app.dark_chrome(), app.chrome()),
                    _ => (false, vec![]),
                };
                views.push(WindowView {
                    dark_chrome,
                    chrome,
                    id: window_id,
                    title,
                    kind: kind.into(),
                    rect,
                    focused: m.desktop.focused == Some(window_id),
                    maximized: window.maximized,
                    minimized: window.minimized,
                    content: Some(content),
                    document,
                    caption,
                    home: home.clone(),
                    modified,
                    // A browser's address field, or a file manager's search field.
                    editing: (kind == "browser"
                        && m.address_focused
                        && m.desktop.focused == Some(window_id))
                        || window
                            .state
                            .file_tab()
                            .is_some_and(|t| t.searching && t.rename.is_none()),
                    tabs,
                    active_tab,
                    can_go_back,
                    can_go_forward,
                    workspace: window.workspace,
                    view_grid: window
                        .state
                        .file_tab()
                        .is_some_and(|t| t.view == cw_applications::FileView::Grid),
                    sort_key: window
                        .state
                        .file_tab()
                        .map(|t| format!("{:?}", t.sort).to_lowercase())
                        .unwrap_or_default(),
                    query: window
                        .state
                        .file_tab()
                        .map(|t| t.query.clone())
                        .unwrap_or_default(),
                    selection: window
                        .state
                        .file_tab()
                        .and_then(|t| t.selected_path())
                        .unwrap_or_default(),
                    zoom: match &window.state {
                        AppState::Browser { .. } if m.active_browser_window == Some(window_id) => {
                            m.browser.zoom()
                        }
                        AppState::Browser { .. } => m
                            .browser_windows
                            .get(&window_id)
                            .unwrap_or(&m.browser)
                            .zoom(),
                        _ => 100,
                    },
                });
            }
            if m.active_app.is_some() {
                let page = self.project_page(&s.config.actor, &s.focused_machine, m)?;
                let rect = area;
                let inner = window_content_rect_for_kind(theme, rect, "custom");
                views.push(WindowView {
                    id: u64::MAX,
                    title: page.title.clone(),
                    kind: "custom".into(),
                    rect,
                    focused: true,
                    maximized: true,
                    minimized: false,
                    content: Some(cw_browser::layout_page(
                        &page,
                        &BTreeMap::new(),
                        inner.width.max(1),
                        inner.height.max(1),
                        0,
                    )),
                    ..Default::default()
                });
            }
            let published = scene_windows(theme, &views);
            let mut scene = render_desktop_with_options(
                theme,
                width,
                height,
                self.runtime.tick(),
                m.desktop.launcher_open,
                views,
                ShellOptions {
                    installed_apps: self
                        .desktop_catalog(id, &s.focused_machine)
                        .into_iter()
                        .map(|app| app.id)
                        .collect(),
                    panel: m.desktop.panel.clone(),
                    search: m.desktop.search.clone(),
                    hover: m.pointer_position,
                    desktop_selection: m.desktop.desktop_selection.clone(),
                    settings: m.desktop.settings,
                    screen: m.desktop.screen,
                    panel_month: m.desktop.panel_month,
                    text_entry: text_entry_of(m, theme.mobile()),
                    keyboard: m.desktop.keyboard,
                    bookmarks: m.desktop.bookmarks.clone(),
                    downloads: m.desktop.downloads.clone(),
                    notifications: m.desktop.notifications.clone(),
                    workspaces: m.desktop.workspace_count(),
                    workspace: m.desktop.workspace,
                    library_group: m.desktop.library_group.clone(),
                    bookmarked: m.browser.url().is_some_and(|url| m.desktop.bookmarked(url)),
                    panel_over_launcher: m.desktop.panel_over_launcher,
                    typed: typed_of(m, theme.mobile()),
                    home_page: m.desktop.home_page,
                    user: self
                        .runtime
                        .computer(&s.focused_machine)
                        .map(|c| c.user.clone())
                        .unwrap_or_default(),
                    recents: m.desktop.recents.clone(),
                    home: m.desktop.home_folder(),
                },
            );
            self.decorate(&mut scene, m, published, theme.mobile());
            return Ok(scene);
        }
        let mut scene = if m.browser_visible {
            m.browser.scene(width, height)
        } else {
            cw_browser::layout_page(
                &self.project_page(&s.config.actor, &s.focused_machine, m)?,
                &BTreeMap::new(),
                width,
                height,
                0,
            )
        };
        // No compositor here, but revisions and the focused field still apply.
        scene.focus = Some(Focus {
            interaction: m.focused_input.clone(),
            keyboard: Keyboard {
                route: if m.focused_input.is_some() {
                    "page"
                } else {
                    "none"
                }
                .into(),
                target: m.focused_input.clone(),
                text_entry: m.focused_input.is_some(),
                window: None,
            },
            ..Focus::default()
        });
        if let Some((node, caret)) = m
            .focused_input
            .as_deref()
            .and_then(|f| Some((f, m.browser.tab().fields.get(f)?.clone())))
            .and_then(|(f, v)| tail_caret(&scene, f, &v))
        {
            let focus = scene.focus.as_mut().expect("focus was just set");
            focus.node = Some(node);
            focus.caret = Some(caret);
        }
        scene.stamp();
        Ok(scene)
    }
}
fn cursor_for_target(target: &str, captured: bool) -> &'static str {
    if let Some(edge) = target.rsplit_once("resize:").map(|(_, edge)| edge) {
        return match edge {
            "n" | "s" => "ns-resize",
            "e" | "w" => "ew-resize",
            "ne" | "sw" => "nesw-resize",
            "nw" | "se" => "nwse-resize",
            _ => "default",
        };
    }
    if target == "drag" || target.ends_with(":drag") {
        return if captured { "grabbing" } else { "grab" };
    }
    // An image editor's canvas takes aim, not a click.
    if target.contains(":canvas:") {
        return "crosshair";
    }
    if target.ends_with("editor-text")
        || target.ends_with("terminal-input")
        || target.ends_with("shell:address")
    {
        return "text";
    }
    "pointer"
}
fn page_has_input(page: Option<&Page>, id: &str) -> bool {
    fn scan(elements: &[PageElement], id: &str) -> bool {
        elements.iter().any(|e| match e {
            PageElement::Input { id: i, .. } => i == id,
            PageElement::Form { children, .. } | PageElement::Group { children, .. } => {
                scan(children, id)
            }
            _ => false,
        })
    }
    page.is_some_and(|p| scan(&p.elements, id))
}

fn active_page(m: &MachineSession) -> Page {
    if let Some(page) = &m.custom_page {
        return page.clone();
    }
    if m.browser_visible {
        let mut page = m
            .browser
            .page()
            .cloned()
            .unwrap_or_else(|| Page::new("Browser"));
        fn fields(elements: &mut [PageElement], values: &BTreeMap<String, String>) {
            for element in elements {
                match element {
                    PageElement::Input { id, value, .. } => {
                        if let Some(current) = values.get(id) {
                            *value = current.clone()
                        }
                    }
                    PageElement::Group { children, .. } | PageElement::Form { children, .. } => {
                        fields(children, values)
                    }
                    _ => {}
                }
            }
        }
        fields(&mut page.elements, &m.browser.tab().fields);
        return page;
    }
    if !m.desktop.windows.is_empty() {
        return m.desktop.page();
    }
    let mut page = Page::new("Terminal");
    if !m.terminal.is_null() {
        page.elements.push(PageElement::Text {
            id: "terminal-output".into(),
            text: format!(
                "{}{}",
                m.terminal
                    .get("stdout")
                    .and_then(Value::as_str)
                    .unwrap_or(""),
                m.terminal
                    .get("stderr")
                    .and_then(Value::as_str)
                    .unwrap_or("")
            ),
        });
    }
    page
}
#[cfg(test)]
mod tests {
    use super::*;
    use cw_sdk::Registry;
    fn world() -> Environment {
        let definition=WorldDefinition::from_json(r#"{"id":"test","profiles":[{"id":"linux","family":"linux"}],"computers":[{"id":"a","profile":"linux","address":"10.0.0.1","user":"alice","initial_files":{"/home/alice/file":"visible"}},{"id":"b","profile":"linux","address":"10.0.0.2","user":"bob","initial_files":{"/home/bob/private":"OTHER_MACHINE_SECRET"}}],"metadata":{"evaluator_secret":"PRIVATE_OBJECTIVE_CANARY"}}"#).unwrap();
        Environment::new(Runtime::new(definition, 42, Registry::new()).unwrap())
    }
    #[test]
    fn actor_grants_and_private_metadata() {
        let mut e = world();
        let id = e
            .environment(EnvironmentConfig::terminal("alice", "a"))
            .unwrap();
        let r = e
            .step(
                &id,
                vec![
                    ActionEnvelope::new(
                        "terminal.v1",
                        "execute",
                        "a",
                        json!({"command":"cat /home/alice/file"}),
                    ),
                    ActionEnvelope::new(
                        "terminal.v1",
                        "execute",
                        "b",
                        json!({"command":"cat /home/bob/private"}),
                    ),
                    ActionEnvelope::new(
                        "filesystem.v1",
                        "read",
                        "a",
                        json!({"path":"/home/alice/file"}),
                    ),
                ],
            )
            .unwrap();
        assert!(r.outcomes[0].success);
        assert!(!r.outcomes[1].success);
        assert!(!r.outcomes[2].success);
        let serialized = serde_json::to_string(&r).unwrap();
        assert!(serialized.contains("visible"));
        assert!(!serialized.contains("OTHER_MACHINE_SECRET"));
        assert!(!serialized.contains("PRIVATE_OBJECTIVE_CANARY"));
        assert!(e.inspect().to_string().contains("PRIVATE_OBJECTIVE_CANARY"));
        assert!(e.scene(&id, 200, 100).is_err());
    }
    #[test]
    fn snapshot_replay_and_fork() {
        let mut e = world();
        e.enable_replay_verification(true);
        let id = e
            .environment(EnvironmentConfig::desktop("alice", "a"))
            .unwrap();
        let initial = e.snapshot();
        e.step(
            &id,
            vec![ActionEnvelope::new(
                "filesystem.v1",
                "write",
                "a",
                json!({"path":"/home/alice/file","content":"changed"}),
            )],
        )
        .unwrap();
        let expected = e.state_hash().unwrap();
        let journal = e.action_journal().clone();
        let restored = e.export_snapshot().unwrap();
        e.replay(&initial, &journal).unwrap();
        assert_eq!(e.state_hash().unwrap(), expected);
        let mut fork = e.fork(&initial).unwrap();
        assert_eq!(
            fork.runtime.read_file("a", "/home/alice/file").unwrap(),
            b"visible"
        );
        fork.import_snapshot(&restored).unwrap();
        assert_eq!(fork.state_hash().unwrap(), expected);
    }
    #[test]
    fn uninstalled_application_is_rejected() {
        let mut e = world();
        let id = e
            .environment(EnvironmentConfig::desktop("alice", "a"))
            .unwrap();
        let result = e
            .step(
                &id,
                vec![ActionEnvelope::new(
                    "application.v1",
                    "launch",
                    "a",
                    json!({"kind":"editor"}),
                )],
            )
            .unwrap();
        assert!(!result.outcomes[0].success);
        assert!(e.session(&id).unwrap().machines["a"]
            .desktop
            .windows
            .is_empty());
    }
    struct Poison;
    impl ActionFamily for Poison {
        fn family(&self) -> &str {
            "poison.v1"
        }
        fn execute(&self, _: &mut Runtime, _: &str, _: &ActionEnvelope) -> Result<Value> {
            Err(SimError::new("PRIVATE_ERROR_CODE", "PRIVATE_ERROR_MESSAGE"))
        }
    }
    #[test]
    fn errors_cannot_reveal_private_plugin_details() {
        let mut e = world();
        e.register_action_family(Arc::new(Poison)).unwrap();
        let mut config = EnvironmentConfig::terminal("alice", "a");
        config.actions.push("poison.v1".into());
        let id = e.environment(config).unwrap();
        let result = e
            .step(
                &id,
                vec![ActionEnvelope::new("poison.v1", "run", "a", Value::Null)],
            )
            .unwrap();
        assert!(!serde_json::to_string(&result).unwrap().contains("PRIVATE_"));
    }
    #[test]
    fn structured_observation_and_inspection_are_read_only() {
        let mut e = world();
        let id = e
            .environment(EnvironmentConfig::desktop("alice", "a"))
            .unwrap();
        let hash = e.state_hash().unwrap();
        let _ = e.observe(&id).unwrap();
        let _ = e.scene(&id, 320, 240).unwrap();
        let _ = e.inspect();
        assert_eq!(hash, e.state_hash().unwrap());
    }
}
impl Environment {
    fn project_page(&self, actor: &str, machine: &str, state: &MachineSession) -> Result<Page> {
        if let Some(instance) = &state.active_app {
            state
                .registered
                .page(
                    &self.app_registry,
                    instance,
                    &self.app_context(actor, machine, instance),
                )
                .map_err(actor_error)
        } else {
            Ok(active_page(state))
        }
    }
    fn app_context(&self, actor: &str, machine: &str, instance: &str) -> cw_sdk::AppContext {
        cw_sdk::AppContext {
            actor: actor.into(),
            machine: machine.into(),
            instance: instance.into(),
            tick: self.runtime.tick(),
            seed: self.runtime.seed(),
        }
    }
    fn custom_launch(&mut self, id: &str, machine: &str, actor: &str, p: &Value) -> Result<Value> {
        let kind = string(p, "kind")?;
        let instance = p.get("instance").and_then(Value::as_str).unwrap_or(kind);
        let ctx = self.app_context(actor, machine, instance);
        let registry = self.app_registry.clone();
        let m = self.machine_mut(id, machine)?;
        m.registered.launch(
            &registry,
            kind,
            instance,
            p.get("initial").cloned().unwrap_or(Value::Null),
            &ctx,
        )?;
        m.custom_page = Some(m.registered.page(&registry, instance, &ctx)?);
        m.active_app = Some(instance.into());
        m.browser_visible = false;
        m.focused_input = None;
        Ok(json!({"instance":instance}))
    }
    fn custom_event(&mut self, id: &str, machine: &str, actor: &str, p: &Value) -> Result<Value> {
        let instance = p
            .get("instance")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .or_else(|| {
                self.sessions
                    .get(id)?
                    .machines
                    .get(machine)?
                    .active_app
                    .clone()
            })
            .ok_or_else(|| SimError::invalid("application instance required"))?;
        let event: cw_sdk::AppEvent =
            serde_json::from_value(p.get("event").cloned().unwrap_or_else(|| p.clone()))?;
        let registry = self.app_registry.clone();
        let ctx = self.app_context(actor, machine, &instance);
        let effects = self
            .machine_mut(id, machine)?
            .registered
            .event(&registry, &instance, &ctx, &event)?;
        self.custom_effects(id, machine, actor, &instance, effects)?;
        let ctx = self.app_context(actor, machine, &instance);
        let m = self.machine_mut(id, machine)?;
        m.custom_page = Some(m.registered.page(&registry, &instance, &ctx)?);
        Ok(serde_json::to_value(&m.custom_page)?)
    }
    fn custom_effects(
        &mut self,
        id: &str,
        machine: &str,
        actor: &str,
        instance: &str,
        effects: Vec<cw_sdk::AppEffect>,
    ) -> Result<()> {
        let mut pending: std::collections::VecDeque<_> = effects.into();
        let mut count = 0;
        while let Some(effect) = pending.pop_front() {
            count += 1;
            if count > 1024 {
                return Err(SimError::invalid("application effect budget exceeded"));
            }
            let response = match effect {
                cw_sdk::AppEffect::ReadFile { path } => Some(
                    json!({"operation":"read_file","path":path,"bytes":self.runtime.read_file(machine,&path)?}),
                ),
                cw_sdk::AppEffect::WriteFile { path, bytes } => {
                    self.runtime.write_file(machine, actor, &path, &bytes)?;
                    Some(json!({"operation":"write_file","path":path}))
                }
                cw_sdk::AppEffect::Http { request } => Some(
                    json!({"operation":"http","response":self.runtime.http(machine,actor,request)?}),
                ),
                cw_sdk::AppEffect::Emit { name, data } => {
                    self.runtime
                        .record_event(&name, Some(machine), Some(actor), data);
                    None
                }
                cw_sdk::AppEffect::Launch { application } => {
                    let action = ActionEnvelope::new(
                        "application.v1",
                        "launch",
                        machine,
                        json!({"kind":application}),
                    );
                    self.dispatch(id, actor, &action)?;
                    None
                }
            };
            if let Some(data) = response {
                let registry = self.app_registry.clone();
                let ctx = self.app_context(actor, machine, instance);
                let event = cw_sdk::AppEvent {
                    kind: "effect_result".into(),
                    target: None,
                    data,
                };
                pending.extend(
                    self.machine_mut(id, machine)?
                        .registered
                        .event(&registry, instance, &ctx, &event)?,
                );
            }
        }
        Ok(())
    }
}
impl Environment {
    fn interface_versions(&self) -> BTreeMap<String, u32> {
        self.extensions
            .iter()
            .map(|(k, v)| (format!("action:{k}"), v.version()))
            .chain(
                self.observation_extensions
                    .iter()
                    .map(|(k, v)| (format!("observation:{k}"), v.version())),
            )
            .collect()
    }
    fn validate_snapshot(&self, snapshot: &Snapshot) -> Result<()> {
        if snapshot.interface_modules != self.interface_versions() {
            return Err(SimError::invalid("interface module versions differ"));
        }
        if snapshot.app_modules != self.app_registry.module_versions() {
            return Err(SimError::invalid("application module versions differ"));
        }
        for session in snapshot.sessions.values() {
            if !session.machines.contains_key(&session.focused_machine)
                || session.config.machines.len() != session.machines.len()
            {
                return Err(SimError::invalid("invalid actor snapshot"));
            }
            for (id, m) in &session.machines {
                if !session.config.machines.contains(id)
                    || !snapshot
                        .kernel
                        .definition()
                        .computers
                        .iter()
                        .any(|c| &c.id == id)
                {
                    return Err(SimError::invalid("invalid machine grant"));
                }
                if m.browser.tabs.is_empty()
                    || m.browser.active >= m.browser.tabs.len()
                    || m.browser
                        .tabs
                        .iter()
                        .any(|t| !t.history.is_empty() && t.position >= t.history.len())
                {
                    return Err(SimError::invalid("invalid browser checkpoint"));
                }
                m.browser.validate_assets()?;
                for (window, browser) in &m.browser_windows {
                    if !m
                        .desktop
                        .windows
                        .get(window)
                        .is_some_and(|w| matches!(w.state, AppState::Browser { .. }))
                        || browser.tabs.is_empty()
                        || browser.active >= browser.tabs.len()
                        || browser
                            .tabs
                            .iter()
                            .any(|t| !t.history.is_empty() && t.position >= t.history.len())
                    {
                        return Err(SimError::invalid("invalid background browser checkpoint"));
                    }
                    browser.validate_assets()?;
                }
                if m.desktop
                    .stacking
                    .iter()
                    .any(|id| !m.desktop.windows.contains_key(id))
                    || m.desktop
                        .stacking
                        .iter()
                        .collect::<std::collections::BTreeSet<_>>()
                        .len()
                        != m.desktop.stacking.len()
                {
                    return Err(SimError::invalid("invalid window stacking"));
                }
                if m.desktop
                    .focused
                    .is_some_and(|id| !m.desktop.windows.get(&id).is_some_and(|w| !w.minimized))
                {
                    return Err(SimError::invalid("invalid window focus"));
                }
                for window in m.desktop.windows.values() {
                    for frame in [window.frame, window.restored_frame].into_iter().flatten() {
                        if frame.width == 0
                            || frame.height == 0
                            || frame.width > 32768
                            || frame.height > 32768
                            || frame.x.unsigned_abs() > 32768
                            || frame.y.unsigned_abs() > 32768
                        {
                            return Err(SimError::invalid("invalid window geometry"));
                        }
                    }
                }
                if let Some(capture) = &m.desktop.pointer_capture {
                    if !m.desktop.windows.contains_key(&capture.window)
                        || !matches!(
                            capture.operation.as_str(),
                            "drag"
                                | "resize:n"
                                | "resize:ne"
                                | "resize:e"
                                | "resize:se"
                                | "resize:s"
                                | "resize:sw"
                                | "resize:w"
                                | "resize:nw"
                        )
                    {
                        return Err(SimError::invalid("invalid pointer capture"));
                    }
                }

                for instance in m.registered.instances.values() {
                    if self.app_registry.application(&instance.kind)?.version() != instance.version
                    {
                        return Err(SimError::invalid("application version mismatch"));
                    }
                }
            }
        }
        Ok(())
    }
}

fn integer_i64(value: &Value) -> Option<i64> {
    value.as_i64().or_else(|| {
        value
            .as_f64()
            .filter(|v| {
                v.is_finite() && v.fract() == 0.0 && *v >= i64::MIN as f64 && *v < i64::MAX as f64
            })
            .map(|v| v as i64)
    })
}
fn integer_u64(value: &Value) -> Option<u64> {
    value.as_u64().or_else(|| {
        value
            .as_f64()
            .filter(|v| v.is_finite() && v.fract() == 0.0 && *v >= 0.0 && *v < u64::MAX as f64)
            .map(|v| v as u64)
    })
}
#[cfg(test)]
mod application_tests {
    use super::*;
    struct Reader;
    impl cw_sdk::Application for Reader {
        fn kind(&self) -> &str {
            "reader"
        }
        fn initialize(&self, _: Value, _: &cw_sdk::AppContext) -> Result<Value> {
            Ok(json!({"secret":"PRIVATE_APP_CANARY","display":"unread"}))
        }
        fn event(
            &self,
            state: &mut Value,
            _: &cw_sdk::AppContext,
            event: &cw_sdk::AppEvent,
        ) -> Result<Vec<cw_sdk::AppEffect>> {
            if event.kind == "click" {
                return Ok(vec![cw_sdk::AppEffect::ReadFile {
                    path: "/home/alice/file".into(),
                }]);
            }
            if event.kind == "effect_result" {
                let bytes: Vec<u8> = serde_json::from_value(event.data["bytes"].clone())?;
                state["display"] = String::from_utf8_lossy(&bytes).into_owned().into();
            }
            Ok(vec![])
        }
        fn page(&self, state: &Value, _: &cw_sdk::AppContext) -> Result<Page> {
            let mut page = Page::new(state["display"].as_str().unwrap());
            page.elements.push(PageElement::Button {
                id: "read".into(),
                text: "Read local file".into(),
                action: PageAction {
                    method: "APP".into(),
                    url: "read".into(),
                    fields: BTreeMap::new(),
                },
            });
            Ok(page)
        }
    }
    #[test]
    fn custom_app_effects_pixels_hit_tests_and_private_state() {
        let definition=WorldDefinition::from_json(r#"{"id":"apps","profiles":[{"id":"linux"}],"computers":[{"id":"a","profile":"linux","address":"10.0.0.1","user":"alice","installed_apps":["reader"],"initial_files":{"/home/alice/file":"real file content"}}]}"#).unwrap();
        let mut e = Environment::new(Runtime::new(definition, 1, cw_sdk::Registry::new()).unwrap());
        e.register_application(Reader).unwrap();
        let id = e
            .environment(EnvironmentConfig::desktop("alice", "a"))
            .unwrap();
        let launch = e
            .step(
                &id,
                vec![ActionEnvelope::new(
                    "application.v1",
                    "launch",
                    "a",
                    json!({"kind":"reader"}),
                )],
            )
            .unwrap();
        assert!(launch.outcomes[0].success);
        assert!(!serde_json::to_string(&launch)
            .unwrap()
            .contains("PRIVATE_APP_CANARY"));
        let scene = e.scene(&id, 400, 200).unwrap();
        let node = scene
            .nodes
            .iter()
            .find(|n| n.interaction.as_deref() == Some("read"))
            .unwrap();
        // Rounded controls intentionally exclude the rectangular corner.
        let x = node.bounds.x + node.bounds.width as i32 / 2;
        let y = node.bounds.y + node.bounds.height as i32 / 2;
        let clicked = e
            .step(
                &id,
                vec![ActionEnvelope::new(
                    "pointer.v1",
                    "click",
                    "a",
                    json!({"x":x as f64,"y":y as f64,"width":400.0,"height":200.0}),
                )],
            )
            .unwrap();
        assert!(clicked.outcomes[0].success, "{:?}", clicked.outcomes[0]);
        assert!(serde_json::to_string(&clicked)
            .unwrap()
            .contains("real file content"));
        assert!(!serde_json::to_string(&clicked)
            .unwrap()
            .contains("PRIVATE_APP_CANARY"));
        let state = e.state_hash().unwrap();
        let snap = e.snapshot();
        e.restore(&snap).unwrap();
        assert_eq!(e.state_hash().unwrap(), state);
    }
}

#[cfg(test)]
mod window_interaction_tests {
    use super::*;
    fn desktop() -> (Environment, String) {
        let definition = WorldDefinition::from_json(r#"{"id":"windows","profiles":[{"id":"virtual-macos-golden-gate","family":"macos"}],"computers":[{"id":"a","profile":"virtual-macos-golden-gate","address":"10.0.0.1","user":"alice","installed_apps":["terminal","editor","browser","files"]}]}"#).unwrap();
        let mut env =
            Environment::new(Runtime::new(definition, 42, cw_sdk::Registry::new()).unwrap());
        let id = env
            .environment(EnvironmentConfig::desktop("alice", "a"))
            .unwrap();
        (env, id)
    }
    fn action(env: &mut Environment, id: &str, family: &str, op: &str, p: Value) -> Value {
        let result = env
            .step(id, vec![ActionEnvelope::new(family, op, "a", p)])
            .unwrap();
        assert!(result.outcomes[0].success, "{op}: {:?}", result.outcomes);
        serde_json::to_value(&result.outcomes[0]).unwrap()
    }
    fn position(env: &Environment, id: &str, target: &str) -> (i32, i32) {
        let scene = env.scene(id, 1200, 800).unwrap();
        let node = scene
            .nodes
            .iter()
            .find(|n| n.interaction.as_deref() == Some(target))
            .unwrap_or_else(|| panic!("missing {target}"));
        (
            node.bounds.x + node.bounds.width as i32 / 2,
            node.bounds.y + node.bounds.height as i32 / 2,
        )
    }
    fn pointer(env: &mut Environment, id: &str, op: &str, x: i32, y: i32) {
        action(
            env,
            id,
            "pointer.v1",
            op,
            json!({"x":x,"y":y,"width":1200,"height":800}),
        );
    }
    #[test]
    fn real_scene_drag_resize_and_minimize_restore_preserve_other_window() {
        let (mut env, id) = desktop();
        action(
            &mut env,
            &id,
            "application.v1",
            "launch",
            json!({"kind":"terminal"}),
        );
        action(
            &mut env,
            &id,
            "application.v1",
            "launch",
            json!({"kind":"editor"}),
        );
        let area = work_area(DesktopTheme::Macos, 1200, 800);
        let before = env.session(&id).unwrap().machines["a"]
            .desktop
            .effective_frame(1, area);
        let (x, y) = position(&env, &id, "window:1:drag");
        pointer(&mut env, &id, "down", x, y);
        pointer(&mut env, &id, "move", x + 40, y + 20);
        let snapshot = env.snapshot();
        pointer(&mut env, &id, "up", x + 40, y + 20);
        assert_eq!(
            env.session(&id).unwrap().machines["a"]
                .desktop
                .effective_frame(1, area)
                .x,
            before.x + 40
        );
        let hash = env.state_hash().unwrap();
        env.restore(&snapshot).unwrap();
        pointer(&mut env, &id, "up", x + 40, y + 20);
        assert_eq!(env.state_hash().unwrap(), hash);
        let (x, y) = position(&env, &id, "window:1:resize:se");
        pointer(&mut env, &id, "down", x, y);
        pointer(&mut env, &id, "up", x - 50, y - 50);
        assert_eq!(
            env.session(&id).unwrap().machines["a"]
                .desktop
                .effective_frame(1, area)
                .width,
            before.width - 50
        );
        let (x, y) = position(&env, &id, "window:1:maximize");
        pointer(&mut env, &id, "down", x, y);
        pointer(&mut env, &id, "up", x, y);
        assert!(env.session(&id).unwrap().machines["a"].desktop.windows[&1].maximized);
        assert!(!env.session(&id).unwrap().machines["a"].desktop.windows[&0].maximized);
        let (x, y) = position(&env, &id, "window:1:minimize");
        pointer(&mut env, &id, "click", x, y);
        assert_eq!(
            env.session(&id).unwrap().machines["a"].desktop.focused,
            Some(0)
        );
        action(
            &mut env,
            &id,
            "application.v1",
            "focus",
            json!({"window":1}),
        );
        assert!(env.session(&id).unwrap().machines["a"].desktop.windows[&1].maximized);
    }
    #[test]
    fn taskbar_press_keeps_original_target_when_focus_changes_release_scene() {
        let definition=WorldDefinition::from_json(r#"{"id":"desktop","profiles":[{"id":"virtual-windows-11","family":"windows"}],"computers":[{"id":"a","profile":"virtual-windows-11","address":"10.0.0.1","user":"alice","installed_apps":["terminal","editor"]}]}"#).unwrap();
        let mut env =
            Environment::new(Runtime::new(definition, 42, cw_sdk::Registry::new()).unwrap());
        let id = env
            .environment(EnvironmentConfig::desktop("alice", "a"))
            .unwrap();
        action(
            &mut env,
            &id,
            "application.v1",
            "launch",
            json!({"kind":"terminal"}),
        );
        action(&mut env, &id, "application.v1", "minimize", json!({}));
        let (x, y) = position(&env, &id, "window:0:focus");
        pointer(&mut env, &id, "down", x, y);
        assert!(!env.session(&id).unwrap().machines["a"].desktop.windows[&0].minimized);
        let snapshot = env.snapshot();
        assert_eq!(
            env.scene(&id, 1200, 800)
                .unwrap()
                .hit_test(x, y)
                .unwrap()
                .interaction
                .as_deref(),
            Some("window:0:minimize")
        );
        pointer(&mut env, &id, "up", x, y);
        assert!(!env.session(&id).unwrap().machines["a"].desktop.windows[&0].minimized);
        env.restore(&snapshot).unwrap();
        pointer(&mut env, &id, "up", x, y);
        assert!(!env.session(&id).unwrap().machines["a"].desktop.windows[&0].minimized);
        pointer(&mut env, &id, "down", x, y);
        pointer(&mut env, &id, "up", 0, 0);
        assert!(
            !env.session(&id).unwrap().machines["a"].desktop.windows[&0].minimized,
            "releasing outside a taskbar button cancels its press"
        );
        pointer(&mut env, &id, "down", x, y);
        pointer(&mut env, &id, "up", x, y);
        assert!(env.session(&id).unwrap().machines["a"].desktop.windows[&0].minimized);
    }
    fn phone(profile: &str) -> (Environment, String) {
        let definition = WorldDefinition::from_json(&format!(
            r#"{{"id":"phone","profiles":[{{"id":"{profile}","family":"linux"}}],"computers":[{{"id":"a","profile":"{profile}","address":"10.0.0.1","user":"alice","installed_apps":["terminal","editor","browser","files"]}}]}}"#
        ))
        .unwrap();
        let mut env =
            Environment::new(Runtime::new(definition, 42, cw_sdk::Registry::new()).unwrap());
        let id = env
            .environment(EnvironmentConfig::desktop("alice", "a"))
            .unwrap();
        (env, id)
    }
    fn swipe(env: &mut Environment, id: &str, from: (i32, i32), to: (i32, i32)) {
        pointer(env, id, "down", from.0, from.1);
        pointer(env, id, "up", to.0, to.1);
    }
    fn shell_of(env: &Environment, id: &str) -> (Option<u64>, bool, Option<String>, u32) {
        let d = &env.session(id).unwrap().machines["a"].desktop;
        (d.focused, d.launcher_open, d.panel.clone(), d.home_page)
    }
    #[test]
    fn mobile_swipes_home_drawer_overview_and_shade_are_serialized_actions() {
        let (mut env, id) = phone("virtual-ios-18");
        action(
            &mut env,
            &id,
            "application.v1",
            "launch",
            json!({"kind":"editor"}),
        );
        // Up from the home indicator goes home.
        swipe(&mut env, &id, (600, 760), (600, 620));
        assert_eq!(shell_of(&env, &id), (None, false, None, 0));
        // Up across the middle of the home screen does nothing on an iPhone.
        swipe(&mut env, &id, (600, 650), (600, 450));
        assert_eq!(shell_of(&env, &id), (None, false, None, 0));
        // Right to left past the last (only) page is the App Library; back again is the
        // last page.
        swipe(&mut env, &id, (900, 400), (500, 400));
        assert_eq!(shell_of(&env, &id), (None, true, None, 0));
        swipe(&mut env, &id, (300, 400), (700, 400));
        assert_eq!(shell_of(&env, &id), (None, false, None, 0));
        // Left to right from the first page is Today View, and back is the first page.
        swipe(&mut env, &id, (300, 400), (700, 400));
        assert_eq!(shell_of(&env, &id).2.as_deref(), Some("calendar"));
        swipe(&mut env, &id, (900, 400), (500, 400));
        assert_eq!(shell_of(&env, &id), (None, false, None, 0));
        // Down across the home screen is Search; the home gesture puts it away.
        swipe(&mut env, &id, (600, 300), (600, 500));
        assert_eq!(shell_of(&env, &id).2.as_deref(), Some("search"));
        swipe(&mut env, &id, (600, 780), (600, 700));
        assert_eq!(shell_of(&env, &id).2, None);
        // Down from the right of the status bar is Control Center, from the left
        // Notification Center; a swipe up puts either away.
        swipe(&mut env, &id, (1100, 20), (1100, 200));
        assert_eq!(shell_of(&env, &id).2.as_deref(), Some("quick"));
        swipe(&mut env, &id, (600, 500), (600, 300));
        assert_eq!(shell_of(&env, &id).2, None);
        swipe(&mut env, &id, (100, 20), (100, 200));
        assert_eq!(shell_of(&env, &id).2.as_deref(), Some("notifications"));
        swipe(&mut env, &id, (600, 500), (600, 300));
        assert_eq!(shell_of(&env, &id).2, None);
        // A long swipe up from the bottom is the App Switcher, and a card swiped up in
        // it closes that application.
        swipe(&mut env, &id, (600, 780), (600, 300));
        assert_eq!(shell_of(&env, &id).2.as_deref(), Some("overview"));
        let (x, y) = position(&env, &id, "window:0:focus");
        swipe(&mut env, &id, (x, y), (x, y - 200));
        let d = &env.session(&id).unwrap().machines["a"].desktop;
        assert!(d.windows.is_empty());
        assert_eq!(d.panel.as_deref(), Some("overview"));
    }
    #[test]
    fn android_swipes_open_the_shade_then_quick_settings_and_the_drawer() {
        let (mut env, id) = phone("virtual-android-15");
        swipe(&mut env, &id, (600, 20), (600, 200));
        assert_eq!(shell_of(&env, &id).2.as_deref(), Some("notifications"));
        // A second pull expands the shade into Quick Settings; a swipe up closes it.
        swipe(&mut env, &id, (600, 100), (600, 400));
        assert_eq!(shell_of(&env, &id).2.as_deref(), Some("quick"));
        swipe(&mut env, &id, (600, 500), (600, 300));
        assert_eq!(shell_of(&env, &id).2, None);
        // Up the home screen is the app drawer, and down closes it.
        swipe(&mut env, &id, (600, 600), (600, 300));
        assert!(shell_of(&env, &id).1);
        swipe(&mut env, &id, (600, 300), (600, 600));
        assert!(!shell_of(&env, &id).1);
        // Swipes that start on the navigation bar are presses of its buttons.
        swipe(&mut env, &id, (600, 790), (600, 400));
        assert_eq!(shell_of(&env, &id), (None, false, None, 0));
    }
    #[test]
    fn independent_browser_windows_keep_separate_history_and_storage() {
        let (mut env, id) = desktop();
        action(
            &mut env,
            &id,
            "application.v1",
            "launch",
            json!({"kind":"browser"}),
        );
        env.machine_mut(&id, "a")
            .unwrap()
            .browser
            .tab_mut()
            .fields
            .insert("first".into(), "value".into());
        action(
            &mut env,
            &id,
            "application.v1",
            "launch",
            json!({"kind":"browser"}),
        );
        assert!(!env
            .machine_mut(&id, "a")
            .unwrap()
            .browser
            .tab()
            .fields
            .contains_key("first"));
        action(
            &mut env,
            &id,
            "application.v1",
            "focus",
            json!({"window":0}),
        );
        assert_eq!(
            env.machine_mut(&id, "a")
                .unwrap()
                .browser
                .tab()
                .fields
                .get("first")
                .map(String::as_str),
            Some("value")
        );
    }
}

// ---- Perception contract: window identity, focus, occlusion, line provenance. ----
// Everything below derives from session state, so the same state yields the same
// scene and the same revision ids after a restore, a fork or a process restart.
use cw_scene::{Caret, Focus, Keyboard, Node, NodeState, Rect, SceneWindow, TextBuffer};

/// Publish what the compositor already knows, so spatial reasoning does not have to be
/// reconstructed from painted text. `views` is bottom-to-top.
fn scene_windows(theme: DesktopTheme, views: &[WindowView]) -> Vec<SceneWindow> {
    let mut windows: Vec<SceneWindow> = views
        .iter()
        .enumerate()
        .map(|(z, v)| SceneWindow {
            id: v.id,
            title: v.title.clone(),
            app: v.kind.clone(),
            bounds: v.rect,
            content: window_content_rect_for_kind(theme, v.rect, &v.kind),
            z: z as u32,
            focused: v.focused,
            minimized: v.minimized,
            maximized: v.maximized,
            document: v.document.clone(),
            tabs: v.tabs.clone(),
            active_tab: v.active_tab,
            occluded_by: Vec::new(),
            exposed: None,
        })
        .collect();
    for i in 0..windows.len() {
        if windows[i].minimized {
            continue;
        }
        let bounds = windows[i].bounds;
        let above: Vec<(u64, Rect)> = windows[i + 1..]
            .iter()
            .filter(|w| !w.minimized && w.bounds.intersection(bounds).is_some())
            .map(|w| (w.id, w.bounds))
            .collect();
        windows[i].occluded_by = above.iter().map(|(id, _)| *id).collect();
        let covers: Vec<Rect> = above.iter().map(|(_, r)| *r).collect();
        windows[i].exposed = cw_scene::exposed(bounds, &covers);
    }
    windows
}

/// Attribute every node to a window. A `window:<id>:` interaction is authoritative; the
/// rest follow the compositor's per-window run, which opens with that window's focus
/// region. Shell chrome paints above every window, so the topmost window node bounds it.
fn attribute_windows(scene: &mut Scene) {
    let Some(top) = scene
        .nodes
        .iter()
        .filter_map(|n| {
            cw_scene::window_of(n.interaction.as_deref()?)?;
            Some(n.z)
        })
        .max()
    else {
        return;
    };
    let mut current = None;
    for n in &mut scene.nodes {
        if let Some(action) = n.interaction.as_deref() {
            current = cw_scene::window_of(action);
        }
        n.window = current.filter(|_| n.z <= top);
    }
}

/// Indices of the text nodes one window's content paints, in reading order.
fn painted_lines(scene: &Scene, window: u64, content: Rect) -> Vec<usize> {
    let mut rows: Vec<usize> = scene
        .nodes
        .iter()
        .enumerate()
        .filter(|(_, n)| {
            n.window == Some(window)
                && n.painted_text().is_some()
                && content.intersection(n.painted_bounds()).is_some()
        })
        .map(|(i, _)| i)
        .collect();
    rows.sort_by_key(|i| {
        let b = scene.nodes[*i].painted_bounds();
        (b.y, b.x, *i)
    });
    rows
}

/// Attach the pane's painted lines to its buffer, marking hard-wrapped continuations.
/// Without this a consumer that joins painted lines reads `initial commi` + `t`.
fn attach_buffer(scene: &mut Scene, buffer: &mut TextBuffer, content: Rect) {
    let Some(window) = buffer.window else { return };
    // One node per visual row: the column the most rows share. Line-number gutters and
    // shell prompts sit in other columns.
    let mut columns: BTreeMap<i32, Vec<usize>> = BTreeMap::new();
    for i in painted_lines(scene, window, content) {
        columns
            .entry(scene.nodes[i].painted_bounds().x)
            .or_default()
            .push(i);
    }
    let Some((_, mut column)) = columns
        .into_iter()
        .max_by_key(|(x, rows)| (rows.len(), std::cmp::Reverse(*x)))
    else {
        return;
    };
    let logical: Vec<&str> = buffer.lines.iter().map(String::as_str).collect();
    let mut lines = Vec::new();
    // Trailing rows a pane paints below its buffer (a shell prompt) are not in it.
    for _ in 0..4 {
        if column.is_empty() {
            return;
        }
        let visual: Vec<&str> = column
            .iter()
            .map(|i| scene.nodes[*i].painted_text().unwrap_or(""))
            .collect();
        lines = cw_scene::reflow(&logical, &visual);
        if !lines.is_empty() {
            break;
        }
        column.pop();
    }
    if lines.is_empty() {
        return;
    }
    buffer.first_visible = lines.first().map(|l| l.logical).unwrap_or(0);
    buffer.visible = lines.last().map(|l| l.logical + 1).unwrap_or(0) - buffer.first_visible;
    let mut head: BTreeMap<u32, u64> = BTreeMap::new();
    for (index, mut line) in lines.into_iter().enumerate() {
        line.pane = Some(buffer.handle.clone());
        line.wrapped_from = head.get(&line.logical).copied();
        let node = &mut scene.nodes[column[index]];
        head.entry(line.logical).or_insert(node.id);
        node.line = Some(line);
    }
}

/// Caret cell at the end of the text `anchor` paints, on that node's own grid.
fn caret_after(anchor: &Node, offset: u32) -> Caret {
    let (cw, ch) = anchor.cell().unwrap_or_else(|| cw_scene::text_cell(13));
    let b = anchor.painted_bounds();
    let painted = anchor.painted_text().unwrap_or("").chars().count() as u32;
    Caret {
        bounds: Rect::new(
            b.x.saturating_add((painted * cw) as i32),
            b.y,
            cw,
            b.height.max(ch),
        ),
        line: 0,
        column: offset,
        offset,
    }
}

/// Caret at the end of the value a single-line control shows. Address bars and page
/// fields always append, so the world knows the insertion point is the end.
fn tail_caret(scene: &Scene, action: &str, value: &str) -> Option<(u64, Caret)> {
    let field = scene
        .nodes
        .iter()
        .find(|n| n.interaction.as_deref() == Some(action))?
        .painted_bounds();
    let offset = value.chars().count() as u32;
    let text = scene
        .nodes
        .iter()
        .filter(|n| {
            let b = n.painted_bounds();
            n.painted_text().is_some()
                && b.area() > 0
                && field.intersection(b).map(|i| i.area()) == Some(b.area())
        })
        .max_by_key(|n| n.painted_bounds().x);
    match text {
        Some(n) => Some((n.id, caret_after(n, offset))),
        // An empty field paints no text; place the caret at its text inset.
        None => {
            let (cw, _) = cw_scene::text_cell(13);
            Some((
                0,
                Caret {
                    bounds: Rect::new(
                        field.x.saturating_add(8),
                        field.y.saturating_add(4),
                        cw,
                        field.height.saturating_sub(8).max(1),
                    ),
                    line: 0,
                    column: offset,
                    offset,
                },
            ))
        }
    }
}

/// Terminal caret: `DesktopState` keeps the shell caret at the end of the input, which
/// the prompt row paints last.
fn terminal_caret(scene: &Scene, window: u64, content: Rect, input: &str) -> Option<(u64, Caret)> {
    let rows = painted_lines(scene, window, content);
    let bottom = rows
        .iter()
        .map(|i| scene.nodes[*i].painted_bounds().y)
        .max()?;
    let last = *rows
        .iter()
        .filter(|i| scene.nodes[**i].painted_bounds().y == bottom)
        .max_by_key(|i| scene.nodes[**i].painted_bounds().x)?;
    let n = &scene.nodes[last];
    Some((n.id, caret_after(n, input.chars().count() as u32)))
}

/// Editor caret: the model owns the offset, the scene owns the grid. The text region's
/// bounds start at the first painted line, and its action carries the scroll position.
fn editor_caret(
    scene: &Scene,
    window: u64,
    content: Rect,
    text: &str,
    cursor: usize,
) -> Option<(u64, Caret)> {
    let mut end = cursor.min(text.len());
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    let before = &text[..end];
    let row = before.bytes().filter(|b| *b == b'\n').count() as u32;
    let column = before.rsplit('\n').next().unwrap_or("").chars().count() as u32;
    let region = scene.nodes.iter().find(|n| {
        n.window == Some(window)
            && n.interaction
                .as_deref()
                .is_some_and(|a| a.contains(":content:editor-text:"))
    })?;
    // `…editor-text:<first row>[:<columns>]`, exactly as the view painted it.
    let mut grid = region
        .interaction
        .as_deref()?
        .split_once("editor-text:")?
        .1
        .split(':');
    let first: u32 = grid.next()?.parse().ok()?;
    let columns: usize = grid.next().and_then(|c| c.parse().ok()).unwrap_or(0);
    // Soft-wrapped rows put the caret on its visual row; `line`/`column` stay logical.
    let (visual_row, visual_column) = cw_applications::editor_caret_cell(text, end, columns);
    // Only the document's own rows: a toolbar or status bar in the same window paints
    // text too, and its spacing is not the document's line pitch.
    let text_area = region.painted_bounds();
    let rows: Vec<usize> = painted_lines(scene, window, content)
        .into_iter()
        .filter(|i| {
            let b = scene.nodes[*i].painted_bounds();
            b.x == text_area.x && b.y >= text_area.y
        })
        .collect();
    let (cw, ch) = rows
        .iter()
        .find_map(|i| scene.nodes[*i].cell())
        .unwrap_or_else(|| cw_scene::text_cell(13));
    // Row pitch is whatever the pane actually painted, not a constant copied from it.
    let mut ys: Vec<i32> = rows
        .iter()
        .map(|i| scene.nodes[*i].painted_bounds().y)
        .collect();
    ys.dedup();
    let pitch = ys
        .windows(2)
        .map(|w| (w[1] - w[0]).unsigned_abs())
        .find(|p| *p > 0)
        .unwrap_or(ch);
    let origin = region.painted_bounds();
    Some((
        region.id,
        Caret {
            bounds: Rect::new(
                origin.x.saturating_add((visual_column as u32 * cw) as i32),
                origin
                    .y
                    .saturating_add(((visual_row as u32).saturating_sub(first) * pitch) as i32),
                cw,
                pitch,
            ),
            line: row,
            column,
            offset: before.chars().count() as u32,
        },
    ))
}

/// The terminal's logical scrollback, in the order the pane paints it: echoed
/// command, streams, and the exit marker a failure adds.
fn transcript_lines(transcript: &[cw_applications::TerminalEntry]) -> Vec<String> {
    let mut lines = Vec::new();
    for entry in transcript {
        lines.push(entry.echo());
        lines.extend(entry.stdout.lines().map(str::to_owned));
        lines.extend(entry.stderr.lines().map(str::to_owned));
        if entry.failed() {
            lines.push(entry.status());
        }
    }
    lines
}

fn control<'a>(scene: &'a Scene, action: &str) -> Option<&'a Node> {
    scene
        .nodes
        .iter()
        .find(|n| n.interaction.as_deref() == Some(action))
}

/// Whether the next keystroke inserts text rather than invoking a command. The shells
/// need this *before* the scene exists, to decide whether to paint a soft keyboard, so
/// it cannot be read back off the composed scene; `focus_and_text_entry_agree` asserts
/// this and `focus_of` never disagree.
fn text_entry_of(m: &MachineSession, phone: bool) -> bool {
    if m.desktop.launcher_open || m.desktop.panel.as_deref() == Some("search") {
        return true;
    }
    if phone && phone_overlay(m) {
        return false;
    }
    if m.active_app.is_some() {
        return m.focused_input.is_some();
    }
    let Some(id) = m.desktop.focused else {
        return false;
    };
    if m.address_focused {
        return true;
    }
    if m.browser_visible {
        return m.focused_input.is_some();
    }
    match m.desktop.windows.get(&id).map(|w| &w.state) {
        Some(AppState::Terminal { .. } | AppState::Editor { .. } | AppState::Browser { .. }) => {
            true
        }
        // A native application takes text while it has a field focused; a music player
        // with none takes no text, so a phone paints no keyboard over it.
        Some(AppState::Native(app)) => app.takes_text(),
        // A file manager takes text only while it is searching or renaming, which is
        // exactly the condition `DesktopState::text` checks.
        Some(state @ AppState::Files { .. }) => {
            state.file_tab().is_some_and(|tab| tab.editing_text())
        }
        _ => false,
    }
}
/// The end of what the field taking keystrokes holds before its caret, following the
/// same order as `text_entry_of`. Empty where the text is an application's own.
fn typed_of(m: &MachineSession, phone: bool) -> String {
    let text = if !text_entry_of(m, phone) {
        String::new()
    } else if m.desktop.launcher_open || m.desktop.panel.as_deref() == Some("search") {
        m.desktop.search.clone()
    } else if m.active_app.is_some() {
        String::new()
    } else if m.address_focused || !m.browser_visible {
        let state = m
            .desktop
            .focused
            .and_then(|id| m.desktop.windows.get(&id))
            .map(|w| &w.state);
        match state {
            Some(AppState::Browser { address }) => address.clone(),
            Some(AppState::Terminal { input, .. }) => input.clone(),
            Some(AppState::Editor { text, cursor, .. }) => {
                let mut end = (*cursor).min(text.len());
                while !text.is_char_boundary(end) {
                    end -= 1;
                }
                text[..end].to_owned()
            }
            Some(files @ AppState::Files { .. }) => files
                .file_tab()
                .map(|tab| match &tab.rename {
                    Some(rename) => rename.name.clone(),
                    None => tab.query.clone(),
                })
                .unwrap_or_default(),
            _ => String::new(),
        }
    } else {
        m.focused_input
            .as_deref()
            .and_then(|f| m.browser.tab().fields.get(f).cloned())
            .unwrap_or_default()
    };
    let skip = text.chars().count().saturating_sub(64);
    text.chars().skip(skip).collect()
}
/// A phone's system surface over the screen — Control Center, the shade, the App
/// Switcher, Settings, a sheet — other than Search. It is modal on the device: it has
/// no text field, the soft keyboard goes down under it, and keystrokes reach nothing
/// behind it until it is put away.
pub(crate) fn phone_overlay(m: &MachineSession) -> bool {
    m.desktop
        .panel
        .as_deref()
        .is_some_and(|panel| panel != "search")
}
/// Where the next keystroke is delivered. Mirrors the `keyboard.v1` dispatch order
/// exactly, so the published answer is what typing would actually do.
fn focus_of(m: &MachineSession, scene: &Scene, windows: &[SceneWindow], phone: bool) -> Focus {
    let window = m
        .desktop
        .focused
        .filter(|id| windows.iter().any(|w| w.id == *id && !w.minimized));
    let mut focus = Focus {
        window,
        ..Focus::default()
    };
    let bind = |focus: &mut Focus, route: &str, action: Option<String>, text_entry: bool| {
        focus.keyboard = Keyboard {
            route: route.into(),
            window: focus.window,
            target: action.clone(),
            text_entry,
        };
        if let Some(node) = action.as_deref().and_then(|a| control(scene, a)) {
            focus.node = Some(node.id);
            if let Some(s) = &node.semantic {
                focus.role = s.role.clone();
                focus.label = s.label.clone();
            }
        }
        focus.interaction = action;
    };
    if m.desktop.launcher_open || m.desktop.panel.as_deref() == Some("search") {
        focus.window = None;
        bind(&mut focus, "panel", None, true);
        focus.role = "searchbox".into();
        focus.label = "Search".into();
        focus.value = Some(m.desktop.search.clone());
        return focus;
    }
    if phone && phone_overlay(m) {
        bind(&mut focus, "panel", None, false);
        return focus;
    }
    if m.active_app.is_some() {
        // A registered application draws into a full-screen window of its own.
        focus.window = Some(u64::MAX);
        let target = m.focused_input.clone();
        let text = target.is_some();
        bind(&mut focus, "application", target, text);
        return focus;
    }
    let Some(id) = window else {
        bind(&mut focus, "none", None, false);
        return focus;
    };
    let state = m.desktop.windows.get(&id).map(|w| &w.state);
    let content = windows
        .iter()
        .find(|w| w.id == id)
        .map(|w| w.content)
        .unwrap_or_default();
    if m.address_focused {
        let address = match state {
            Some(AppState::Browser { address }) => address.clone(),
            _ => String::new(),
        };
        let action = format!("window:{id}:content:shell:address");
        let caret = tail_caret(scene, &action, &address);
        bind(&mut focus, "address", Some(action), true);
        focus.value = Some(address);
        focus.caret = caret.map(|(_, c)| c);
        return focus;
    }
    if m.browser_visible {
        let field = m.focused_input.clone();
        let value = field
            .as_deref()
            .and_then(|f| m.browser.tab().fields.get(f).cloned());
        let action = field.map(|f| format!("window:{id}:content:{f}"));
        let caret = action
            .as_deref()
            .zip(value.as_deref())
            .and_then(|(a, v)| tail_caret(scene, a, v));
        let text = action.is_some();
        bind(&mut focus, "page", action, text);
        focus.value = value;
        focus.caret = caret.map(|(_, c)| c);
        return focus;
    }
    match state {
        Some(AppState::Terminal { input, .. }) => {
            let caret = terminal_caret(scene, id, content, input);
            bind(
                &mut focus,
                "terminal",
                Some(format!("window:{id}:content:terminal-input")),
                true,
            );
            focus.value = Some(input.clone());
            if let Some((node, c)) = caret {
                focus.node = Some(node);
                focus.caret = Some(c);
            }
        }
        Some(AppState::Editor { text, cursor, .. }) => {
            let caret = editor_caret(scene, id, content, text, *cursor);
            let action = scene
                .nodes
                .iter()
                .find(|n| {
                    n.window == Some(id)
                        && n.interaction
                            .as_deref()
                            .is_some_and(|a| a.contains(":content:editor-text:"))
                })
                .and_then(|n| n.interaction.clone());
            bind(&mut focus, "editor", action, true);
            // The document is in `Scene::buffers`; repeating it here would double the
            // cost of every scene for a large file.
            if let Some((node, c)) = caret {
                focus.node = Some(node);
                focus.caret = Some(c);
            }
        }
        Some(AppState::Browser { address }) => {
            let action = format!("window:{id}:content:shell:address");
            bind(&mut focus, "address", Some(action), true);
            focus.value = Some(address.clone());
        }
        Some(AppState::Native(app)) => {
            bind(
                &mut focus,
                "application",
                Some(format!("window:{id}:focus")),
                app.takes_text(),
            );
        }
        // A file manager takes text only while it is searching or renaming.
        Some(files @ AppState::Files { .. }) if text_entry_of(m, phone) => {
            let tab = files.file_tab();
            let (control, value) = match tab {
                Some(tab) if tab.rename.is_some() => {
                    ("files-rename", tab.rename.as_ref().map(|r| r.name.clone()))
                }
                Some(tab) => ("files-search", Some(tab.query.clone())),
                None => ("files-search", None),
            };
            let action = format!("window:{id}:content:{control}");
            let caret = value.as_deref().and_then(|v| tail_caret(scene, &action, v));
            bind(&mut focus, "files", Some(action), true);
            focus.value = value;
            focus.caret = caret.map(|(_, c)| c);
        }
        _ => bind(
            &mut focus,
            "window",
            Some(format!("window:{id}:focus")),
            false,
        ),
    }
    focus
}

/// Publish the state a shell otherwise encodes only in its painting: switch positions,
/// open panels, the selected tab and window, and which control has focus.
fn annotate_states(scene: &mut Scene, m: &MachineSession, windows: &[SceneWindow], focus: &Focus) {
    let focused = focus.interaction.clone();
    // The page a paged home screen really shows: the one asked for, within the pages
    // the shell painted a dot for.
    let last_page = scene
        .nodes
        .iter()
        .filter_map(|n| n.interaction.as_deref()?.strip_prefix("shell:home-page:"))
        .filter_map(|page| page.parse::<u32>().ok())
        .max();
    let shown_page = last_page.map(|last| m.desktop.home_page.min(last));
    for n in &mut scene.nodes {
        let Some(action) = n.interaction.clone() else {
            continue;
        };
        // `window:<id>:<local>`; unnamespaced shell controls are their own local name.
        let local = match cw_scene::window_of(&action) {
            Some(_) => action.splitn(3, ':').nth(2).unwrap_or_default(),
            None => action.as_str(),
        };
        let inner = local.strip_prefix("content:").unwrap_or(local);
        let tab = |prefix: &str| -> Option<bool> {
            let index: usize = inner.strip_prefix(prefix)?.parse().ok()?;
            Some(windows.iter().find(|w| Some(w.id) == n.window)?.active_tab == index)
        };
        let mut state = NodeState {
            focused: focused.as_deref() == Some(action.as_str()),
            checked: inner
                .strip_prefix("shell:toggle:")
                .and_then(|name| m.desktop.settings.flag(name).ok()),
            expanded: match inner {
                "shell:launcher" => Some(m.desktop.launcher_open),
                _ => inner
                    .strip_prefix("shell:panel:")
                    .map(|name| m.desktop.panel.as_deref() == Some(name)),
            },
            selected: tab("shell:tab:select:")
                .or_else(|| tab("files-tab:"))
                .or_else(|| {
                    let page: u32 = inner.strip_prefix("shell:home-page:")?.parse().ok()?;
                    Some(shown_page == Some(page))
                })
                .or_else(|| {
                    (local == "focus").then(|| n.window.is_some() && n.window == m.desktop.focused)
                })
                .or_else(|| {
                    let icon = inner
                        .strip_prefix("shell:open:")
                        .or_else(|| inner.strip_prefix("shell:launch:"))?;
                    Some(m.desktop.desktop_selection.as_deref() == Some(icon))
                }),
        };
        if state.focused && n.semantic.is_none() {
            state.focused = false;
        }
        n.state = (!state.is_empty()).then_some(state);
    }
}

impl Environment {
    /// Items 1-6 of the perception contract, applied to a composed desktop scene.
    fn decorate(
        &self,
        scene: &mut Scene,
        m: &MachineSession,
        windows: Vec<SceneWindow>,
        phone: bool,
    ) {
        attribute_windows(scene);
        let mut buffers = Vec::new();
        for w in windows.iter().filter(|w| !w.minimized) {
            let Some(window) = m.desktop.windows.get(&w.id) else {
                continue;
            };
            let (kind, lines) = match &window.state {
                AppState::Terminal { transcript, .. } => ("terminal", transcript_lines(transcript)),
                AppState::Editor { text, .. } => (
                    "editor",
                    text.split('\n').map(str::to_owned).collect::<Vec<_>>(),
                ),
                _ => continue,
            };
            let mut buffer = TextBuffer {
                handle: format!("window:{}:{kind}", w.id),
                window: Some(w.id),
                kind: kind.into(),
                lines,
                first_visible: 0,
                visible: 0,
                truncated: false,
            }
            .bound();
            attach_buffer(scene, &mut buffer, w.content);
            buffers.push(buffer);
        }
        let focus = focus_of(m, scene, &windows, phone);
        annotate_states(scene, m, &windows, &focus);
        scene.windows = windows;
        scene.buffers = buffers;
        scene.focus = Some(focus);
        scene.stamp();
    }
}

// ---- Action-result contract: what an action changed, not just whether it was accepted.
/// Actor-visible projection of one machine: everything an observation or a scene could
/// reveal, cheap enough to take before and after every action in a batch.
#[derive(Clone, PartialEq, Eq, Serialize)]
struct Visible {
    focused: Option<u64>,
    windows: Vec<VisibleWindow>,
    url: Option<String>,
    app: Option<String>,
    page: u64,
    panel: Option<String>,
    launcher: bool,
    home_page: u32,
    address_focused: bool,
    focused_input: Option<String>,
    terminal: u64,
}
#[derive(Clone, PartialEq, Eq, Serialize)]
struct VisibleWindow {
    id: u64,
    title: String,
    frame: Option<Rect>,
    minimized: bool,
    maximized: bool,
    document: String,
    /// Digest of the window's whole application state: text, caret, tabs, dirty flag.
    content: u64,
}
/// Path, URL or document a window presents; empty when it presents none.
fn presented(state: &AppState) -> String {
    match state {
        AppState::Browser { address } => address.clone(),
        AppState::Editor { path, .. } => path.clone(),
        AppState::Files { .. } => state.file_path().to_owned(),
        AppState::Native(app) => app.document(),
        AppState::Terminal { .. } => String::new(),
    }
}
fn visible(m: &MachineSession) -> Visible {
    Visible {
        focused: m.desktop.focused,
        windows: m
            .desktop
            .windows
            .values()
            .map(|w| VisibleWindow {
                id: w.id,
                title: w.title.clone(),
                frame: w.frame,
                minimized: w.minimized,
                maximized: w.maximized,
                document: presented(&w.state),
                content: cw_scene::digest(&w.state),
            })
            .collect(),
        url: m.browser.url().map(str::to_owned),
        app: m.active_app.clone(),
        page: cw_scene::digest(&m.custom_page),
        panel: m.desktop.panel.clone(),
        launcher: m.desktop.launcher_open,
        home_page: m.desktop.home_page,
        address_focused: m.address_focused,
        focused_input: m.focused_input.clone(),
        terminal: cw_scene::digest(&m.terminal),
    }
}
fn effect_of(before: &Visible, after: &Visible) -> ActionEffect {
    let mut changed: BTreeSet<&str> = BTreeSet::new();
    let ids = |v: &Visible| v.windows.iter().map(|w| w.id).collect::<BTreeSet<_>>();
    let opened: Vec<u64> = ids(after).difference(&ids(before)).copied().collect();
    let closed: Vec<u64> = ids(before).difference(&ids(after)).copied().collect();
    if !opened.is_empty() {
        changed.insert(effect::WINDOW_OPENED);
    }
    if !closed.is_empty() {
        changed.insert(effect::WINDOW_CLOSED);
    }
    for w in &after.windows {
        let Some(old) = before.windows.iter().find(|o| o.id == w.id) else {
            continue;
        };
        if old.title != w.title {
            changed.insert(effect::WINDOW_TITLE);
        }
        if (old.frame, old.minimized, old.maximized) != (w.frame, w.minimized, w.maximized) {
            changed.insert(effect::WINDOW_MOVED);
        }
        if old.document != w.document {
            changed.insert(effect::DOCUMENT);
        }
        if old.content != w.content {
            changed.insert(effect::CONTENT);
        }
    }
    if before.focused != after.focused {
        changed.insert(effect::WINDOW_FOCUSED);
    }
    if before.url != after.url {
        changed.insert(effect::NAVIGATE);
    }
    if (
        &before.panel,
        before.launcher,
        before.home_page,
        before.address_focused,
        &before.focused_input,
    ) != (
        &after.panel,
        after.launcher,
        after.home_page,
        after.address_focused,
        &after.focused_input,
    ) {
        changed.insert(effect::FOCUS);
    }
    if (&before.app, before.page) != (&after.app, after.page) {
        changed.insert(effect::APPLICATION);
    }
    if before.terminal != after.terminal {
        changed.insert(effect::TERMINAL);
    }
    ActionEffect {
        changed: changed.into_iter().map(str::to_owned).collect(),
        windows_opened: opened,
        windows_closed: closed,
        focused_window: after.focused,
        url: after.url.clone(),
        state: cw_scene::digest(after),
    }
}

#[cfg(test)]
mod perception_tests {
    use super::*;
    use cw_sdk::Registry;
    fn desktop() -> (Environment, String) {
        let definition = WorldDefinition::from_json(
            r#"{"id":"t","profiles":[{"id":"linux","family":"linux"}],"computers":[{"id":"a",
            "profile":"linux","address":"10.0.0.1","user":"alice","installed_apps":["terminal",
            "editor","files","browser"],"initial_files":{"/home/alice/notes.txt":"initial commit of a long line that the pane will hard wrap\nsecond"}}],
            "metadata":{"desktop_themes":{"a":"ubuntu"}}}"#,
        )
        .unwrap();
        let mut e = Environment::new(Runtime::new(definition, 7, Registry::new()).unwrap());
        let id = e
            .environment(EnvironmentConfig::desktop("alice", "a"))
            .unwrap();
        (e, id)
    }
    fn launch(e: &mut Environment, id: &str, kind: &str, argument: &str) -> u64 {
        let r = e
            .step(
                id,
                vec![ActionEnvelope::new(
                    "application.v1",
                    "launch",
                    "a",
                    json!({"kind":kind,"argument":argument}),
                )],
            )
            .unwrap();
        assert!(r.outcomes[0].success, "{:?}", r.outcomes[0].error);
        r.outcomes[0].value["window"].as_u64().unwrap()
    }
    #[test]
    fn windows_are_published_with_identity_stacking_and_occlusion() {
        let (mut e, id) = desktop();
        let first = launch(&mut e, &id, "terminal", "");
        let second = launch(&mut e, &id, "editor", "/home/alice/notes.txt");
        let scene = e.scene(&id, 1280, 800).unwrap();
        assert_eq!(scene.windows.len(), 2);
        let bottom = scene.window(first).unwrap();
        let top = scene.window(second).unwrap();
        assert_eq!(bottom.app, "terminal");
        assert_eq!(top.app, "editor");
        assert_eq!(top.document, "/home/alice/notes.txt");
        assert!(top.focused && !bottom.focused);
        assert!(bottom.z < top.z);
        assert!(bottom.bounds.area() > 0 && bottom.content.area() > 0);
        // The overlap is reported as geometry, not as a boolean the agent must guess.
        assert_eq!(bottom.occluded_by, vec![second]);
        assert!(top.occluded_by.is_empty());
        let exposed = bottom.exposed.expect("partly visible");
        assert!(exposed.area() > 0 && exposed.area() < bottom.bounds.area());
        // A point inside the exposed part really does reach the lower window.
        let point = (
            exposed.x + exposed.width as i32 / 2,
            exposed.y + exposed.height as i32 / 2,
        );
        let stack = scene.hit_stack(point.0, point.1);
        assert!(stack
            .iter()
            .find(|h| h.interaction.is_some())
            .is_some_and(|h| h.window == Some(first)));
        // ... and the covered part reaches the upper one.
        let covered = top.bounds;
        let over = (covered.x + 4, covered.y + covered.height as i32 / 2);
        assert_eq!(
            scene.hit_test(over.0, over.1).and_then(|n| n.window),
            Some(second)
        );
    }
    #[test]
    fn accessibility_tree_names_window_owned_controls() {
        let (mut e, id) = desktop();
        let window = launch(&mut e, &id, "terminal", "");
        let scene = e.scene(&id, 1280, 800).unwrap();
        let ax = scene.accessibility();
        let close = ax
            .iter()
            .find(|a| a.id == format!("window:{window}:close"))
            .expect("close control");
        assert_eq!(close.window, Some(window));
        assert!(!close.role.is_empty() && !close.name.is_empty());
        assert!(close.enabled && close.hit.is_some() && close.revision != 0);
        // Merging is by interaction id: a control painted as several nodes is one entry.
        assert_eq!(ax.iter().filter(|a| a.id == close.id).count(), 1);
        let focus = scene.focus.as_ref().unwrap();
        assert_eq!(focus.window, Some(window));
        assert_eq!(focus.keyboard.route, "terminal");
        assert!(focus.keyboard.text_entry);
        assert!(ax.iter().any(|a| a.focused));
    }
    #[test]
    fn caret_is_published_instead_of_being_found_in_the_pixels() {
        let (mut e, id) = desktop();
        let window = launch(&mut e, &id, "terminal", "");
        e.step(
            &id,
            vec![ActionEnvelope::new(
                "keyboard.v1",
                "type",
                "a",
                json!({"text":"echo hi"}),
            )],
        )
        .unwrap();
        let scene = e.scene(&id, 1280, 800).unwrap();
        let focus = scene.focus.clone().unwrap();
        assert_eq!(focus.value.as_deref(), Some("echo hi"));
        let caret = focus.caret.expect("caret");
        assert_eq!(caret.offset, 7);
        assert!(caret.bounds.area() > 0);
        assert!(scene
            .window(window)
            .unwrap()
            .content
            .intersection(caret.bounds)
            .is_some());
        // The caret advances by exactly one cell per character typed.
        e.step(
            &id,
            vec![ActionEnvelope::new(
                "keyboard.v1",
                "type",
                "a",
                json!({"text":"!"}),
            )],
        )
        .unwrap();
        let next = e
            .scene(&id, 1280, 800)
            .unwrap()
            .focus
            .unwrap()
            .caret
            .unwrap();
        assert_eq!(next.offset, 8);
        assert_eq!(next.bounds.y, caret.bounds.y);
        assert_eq!(next.bounds.x - caret.bounds.x, caret.bounds.width as i32);
    }
    #[test]
    fn editor_caret_and_buffer_expose_the_whole_document() {
        let (mut e, id) = desktop();
        let window = launch(&mut e, &id, "editor", "/home/alice/notes.txt");
        let scene = e.scene(&id, 1000, 700).unwrap();
        let buffer = scene
            .buffers
            .iter()
            .find(|b| b.window == Some(window))
            .expect("editor buffer");
        assert_eq!(buffer.kind, "editor");
        assert_eq!(buffer.lines.len(), 2);
        assert!(buffer.lines[0].starts_with("initial commit"));
        let focus = scene.focus.as_ref().unwrap();
        assert_eq!(focus.keyboard.route, "editor");
        let caret = focus.caret.expect("caret");
        // The editor opens with the caret at the end of the file, and says so.
        assert_eq!(
            (caret.line, caret.column, caret.offset),
            (1, 6, buffer.lines.join("\n").chars().count() as u32)
        );
    }
    #[test]
    fn hard_wrapped_terminal_lines_are_marked_as_continuations() {
        let (mut e, id) = desktop();
        let window = launch(&mut e, &id, "terminal", "");
        e.step(
            &id,
            vec![
                ActionEnvelope::new(
                    "keyboard.v1",
                    "type",
                    "a",
                    json!({"text":"cat /home/alice/notes.txt"}),
                ),
                ActionEnvelope::new("keyboard.v1", "key", "a", json!({"key":"Enter"})),
            ],
        )
        .unwrap();
        // A pane narrow enough to hard wrap the file's first line mid-word.
        let scene = e.scene(&id, 560, 520).unwrap();
        let buffer = scene
            .buffers
            .iter()
            .find(|b| b.window == Some(window))
            .expect("terminal buffer");
        assert!(buffer
            .lines
            .iter()
            .any(|l| l.starts_with("initial commit of a long line")));
        let mut lines: Vec<&Node> = scene
            .nodes
            .iter()
            .filter(|n| {
                n.line
                    .as_ref()
                    .is_some_and(|l| l.pane.as_deref() == Some(&buffer.handle))
            })
            .collect();
        assert!(
            lines.len() > buffer.lines.len(),
            "the pane must have wrapped"
        );
        lines.sort_by_key(|n| n.painted_bounds().y);
        // Joining painted lines using the flags reproduces the buffer exactly.
        let mut joined: Vec<String> = Vec::new();
        for n in &lines {
            let line = n.line.as_ref().unwrap();
            if line.continuation {
                joined
                    .last_mut()
                    .unwrap()
                    .push_str(n.painted_text().unwrap());
            } else {
                joined.push(n.painted_text().unwrap().to_owned());
            }
        }
        let start = buffer.first_visible as usize;
        assert_eq!(joined, buffer.lines[start..start + joined.len()]);
        // Continuations point back at the fragment they continue.
        let wrapped = lines
            .iter()
            .find(|n| n.line.as_ref().unwrap().continuation)
            .unwrap();
        let line = wrapped.line.as_ref().unwrap();
        assert!(line.offset > 0);
        assert_eq!(
            line.wrapped_from,
            lines
                .iter()
                .find(|n| {
                    let l = n.line.as_ref().unwrap();
                    l.logical == line.logical && !l.continuation
                })
                .map(|n| n.id)
        );
    }
    #[test]
    fn revisions_are_stable_across_snapshot_restore_and_detect_no_op_steps() {
        let (mut e, id) = desktop();
        launch(&mut e, &id, "terminal", "");
        let before = e.scene(&id, 1024, 768).unwrap();
        assert!(before.digest != 0 && before.nodes.iter().all(|n| n.revision != 0));
        // Re-asking for the same viewport in the same state is bit-identical.
        assert_eq!(e.scene(&id, 1024, 768).unwrap(), before);
        assert!(!e.scene(&id, 1024, 768).unwrap().diff(&before).changed);
        let snapshot = e.snapshot();
        e.step(
            &id,
            vec![ActionEnvelope::new(
                "keyboard.v1",
                "type",
                "a",
                json!({"text":"ls"}),
            )],
        )
        .unwrap();
        let typed = e.scene(&id, 1024, 768).unwrap();
        let delta = typed.diff(&before);
        assert!(delta.changed && delta.focus);
        assert!(!delta.updated.is_empty());
        // Restoring reproduces the same ids: the counter is the content, not a tick.
        e.restore(&snapshot).unwrap();
        let restored = e.scene(&id, 1024, 768).unwrap();
        assert_eq!(restored.digest, before.digest);
        assert!(!restored.diff(&before).changed);
    }
    #[test]
    fn outcomes_report_the_app_level_consequence() {
        let (mut e, id) = desktop();
        let launched = e
            .step(
                &id,
                vec![ActionEnvelope::new(
                    "application.v1",
                    "launch",
                    "a",
                    json!({"kind":"terminal"}),
                )],
            )
            .unwrap();
        let effect = launched.outcomes[0].effect.clone().expect("effect");
        assert!(effect.changed.contains(&effect::WINDOW_OPENED.to_owned()));
        assert_eq!(effect.windows_opened.len(), 1);
        assert_eq!(effect.focused_window, Some(effect.windows_opened[0]));
        // Typing changes pane content and nothing structural.
        let typed = e
            .step(
                &id,
                vec![ActionEnvelope::new(
                    "keyboard.v1",
                    "type",
                    "a",
                    json!({"text":"ls"}),
                )],
            )
            .unwrap();
        let typed = typed.outcomes[0].effect.clone().unwrap();
        assert_eq!(typed.changed, vec![effect::CONTENT.to_owned()]);
        assert!(typed.state != effect.state);
        // An accepted action that changes nothing observable says so.
        let noop = e
            .step(
                &id,
                vec![ActionEnvelope::new(
                    "application.v1",
                    "focus",
                    "a",
                    json!({"window":effect.windows_opened[0]}),
                )],
            )
            .unwrap();
        let noop = noop.outcomes[0].effect.clone().unwrap();
        assert!(noop.is_noop(), "{:?}", noop.changed);
        assert_eq!(noop.state, typed.state);
        // A denied action never reached a machine, so it attributes no effect.
        let denied = e
            .step(
                &id,
                vec![ActionEnvelope::new("poison.v1", "run", "a", Value::Null)],
            )
            .unwrap();
        assert!(!denied.outcomes[0].success && denied.outcomes[0].effect.is_none());
    }
}
