//! Capability-filtered actor interfaces above the canonical kernel.
//! Owner inspection and snapshots are deliberately separate from actor observations.
use cw_applications::DesktopState;
use cw_browser::BrowserState;
use cw_kernel::Runtime;
use cw_protocol::*;
use cw_scene::Scene;
use cw_trajectory::Journal;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::sync::Arc;

#[derive(Clone, Serialize, Deserialize)]
pub struct MachineSession {
    pub browser: BrowserState,
    pub desktop: DesktopState,
    pub terminal: Value,
    pub focused_input: Option<String>,
    pub browser_visible: bool,
    pub registered: cw_applications::RegisteredApplications,
    pub active_app: Option<String>,
    pub custom_page: Option<Page>,
}
impl Default for MachineSession {
    fn default() -> Self {
        Self {
            browser: BrowserState::default(),
            desktop: DesktopState::default(),
            terminal: Value::Null,
            focused_input: None,
            browser_visible: false,
            registered: Default::default(),
            active_app: None,
            custom_page: None,
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
            verify_steps: false,
        }
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
            .map(|m| (m.clone(), MachineSession::default()))
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
        Ok(id)
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
            let result = if !config.machines.contains(&action.machine)
                || !config.actions.contains(&action.family)
            {
                Err(SimError::denied("action is not permitted"))
            } else {
                self.dispatch(id, &config.actor, action)
            };
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
                },
                Err(e) => ActionOutcome {
                    index,
                    success: false,
                    value: Value::Null,
                    error: Some(actor_error(e)),
                },
            });
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
        env.restore(snapshot)?;
        Ok(env)
    }
    pub fn reset(&mut self, seed: u64) -> Result<()> {
        self.runtime.reset(seed)?;
        self.prune_sessions();
        for session in Arc::make_mut(&mut self.sessions).values_mut() {
            for machine in session.machines.values_mut() {
                *machine = MachineSession::default();
            }
            session.focused_machine = session.config.machines[0].clone();
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
                let canonical = match requested {
                    "text_editor" => "editor",
                    "file_manager" => "files",
                    other => other,
                };
                let computer = self.runtime.computer(machine)?;
                if !computer.application_available(requested)
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
                self.machine_mut(id, machine)?.focused_input = None;
                let kind = string(p, "kind")?;
                let arg = p.get("argument").and_then(Value::as_str).unwrap_or("");
                let (window, effects) = self
                    .machine_mut(id, machine)?
                    .desktop
                    .launch(kind, arg)
                    .map_err(SimError::invalid)?;
                self.effects(id, machine, actor, effects)?;
                Ok(json!({"window":window}))
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
                Ok(Value::Null)
            }
            ("keyboard.v1", "type") => {
                if self.machine_mut(id, machine)?.active_app.is_some() {
                    return self.custom_event(id, machine, actor, &json!({"kind":"text","data":p}));
                }
                let text = string(p, "text")?;
                if self.machine_mut(id, machine)?.browser_visible {
                    self.machine_mut(id, machine)?.browser.text(text)?;
                } else {
                    self.machine_mut(id, machine)?
                        .desktop
                        .text(text)
                        .map_err(SimError::invalid)?;
                }
                Ok(Value::Null)
            }
            ("keyboard.v1", "key") => {
                if self.machine_mut(id, machine)?.active_app.is_some() {
                    return self.custom_event(id, machine, actor, &json!({"kind":"key","data":p}));
                }
                if self.machine_mut(id, machine)?.browser_visible {
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
            ("pointer.v1", "click") => {
                let x = p
                    .get("x")
                    .and_then(integer_i64)
                    .ok_or_else(|| SimError::invalid("x required"))? as i32;
                let y = p
                    .get("y")
                    .and_then(integer_i64)
                    .ok_or_else(|| SimError::invalid("y required"))? as i32;
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
                let scene = self.scene(id, width, height)?;
                let target = scene
                    .hit_test(x, y)
                    .and_then(|n| n.interaction.clone())
                    .ok_or_else(|| SimError::not_found("interaction"))?;
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
                            if let cw_applications::AppState::Files { entries, .. } = &window.state
                            {
                                if let Some(entry) = entries.get(index) {
                                    let kind = if entry.ends_with('/') {
                                        "files"
                                    } else {
                                        "editor"
                                    };
                                    if !self.runtime.computer(machine)?.application_available(kind)
                                    {
                                        return Err(SimError::not_found(
                                            "application is not installed",
                                        ));
                                    }
                                }
                            }
                        }
                    }
                    let effects = self
                        .machine_mut(id, machine)?
                        .desktop
                        .click(&target)
                        .map_err(SimError::invalid)?;
                    self.effects(id, machine, actor, effects)?;
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
        let runtime = &mut self.runtime;
        let machine = Arc::make_mut(&mut self.sessions)
            .get_mut(id)
            .and_then(|s| s.machines.get_mut(&a.machine))
            .ok_or_else(|| SimError::denied("machine unavailable"))?;
        machine.browser_visible = true;
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
        for effect in effects {
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
                    self.machine_mut(id, machine)?
                        .desktop
                        .file_saved(window, &content)
                        .map_err(SimError::invalid)?;
                }
                ListDirectory { window, path } => {
                    let c = self.runtime.computer(machine)?;
                    let base = c.resolve(&path);
                    c.vfs
                        .check_access(&base, &c.user, true, false, true)
                        .map_err(|_| SimError::denied("directory access denied"))?;
                    let entries = c
                        .vfs
                        .list(&base)
                        .map_err(|_| SimError::not_found("directory"))?
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
                        .directory_loaded(window, entries)
                        .map_err(SimError::invalid)?;
                }
                Execute { window, command } => {
                    let result = self.runtime.execute(machine, actor, &command)?;
                    self.machine_mut(id, machine)?
                        .desktop
                        .terminal_output(window, &format!("{}{}", result.stdout, result.stderr))
                        .map_err(SimError::invalid)?;
                    self.machine_mut(id, machine)?.terminal = serde_json::to_value(result)?;
                }
                Navigate { window: _, url } => {
                    self.browser_action(
                        id,
                        actor,
                        &ActionEnvelope::new("browser.v1", "navigate", machine, json!({"url":url})),
                    )?;
                }
            }
        }
        Ok(())
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
        Ok(if m.browser_visible {
            m.browser.scene(width, height)
        } else {
            cw_browser::layout_page(
                &self.project_page(&s.config.actor, &s.focused_machine, m)?,
                &BTreeMap::new(),
                width,
                height,
                0,
            )
        })
    }
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
        let x = node.bounds.x + 1;
        let y = node.bounds.y + 1;
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
