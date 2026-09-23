//! Capability-filtered actor interfaces above the canonical kernel.
//! Owner inspection and snapshots are deliberately separate from actor observations.
mod custom_apps;
mod desktop_extensions;
mod dispatch;
mod scene;
mod snapshot;
use cw_applications::desktop_scene::{
    home_page_count, render_desktop_with_options, window_content_rect_for_kind, work_area,
    DesktopTheme, ShellOptions, WindowView,
};
use cw_applications::{AppState, CursorKind, DesktopState};
use cw_browser::BrowserState;
use cw_kernel::Runtime;
use cw_protocol::*;
use cw_scene::Scene;
use cw_trajectory::Journal;
#[cfg(test)]
use scene::browser_view;
pub(crate) use scene::phone_overlay;
use scene::{active_page, effect_of, visible};
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
    /// The CSS cursor the HTML document under the pointer asked for, so the pointer
    /// painted into the frame agrees with the hint the move action returned.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pointer_cursor: Option<String>,
    /// Press identity remains stable even if focusing changes the taskbar action.
    #[serde(default)]
    pub pointer_press: Option<(String, cw_scene::Rect)>,
    /// A finger dragging a list on a phone: it follows the finger move by move, and
    /// flings on release.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub touch_scroll: Option<TouchScroll>,
    /// The screen size the actor last addressed this machine at (the `width` and
    /// `height` its pointer actions state), which is the screen a screenshot captures.
    /// Recorded from actions, never from observations, so it replays exactly.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub viewport: Option<(u32, u32)>,
    /// Something scrolled during the action being dispatched, so its effect reports
    /// `scroll` even where the view that moved is an application's own (a grid's rows).
    /// Cleared before each action; never part of the state.
    #[serde(skip)]
    pub scrolled: bool,
    /// The machine process each open window runs as, so an open application is a real
    /// entry in `ps` and `kill` on it really closes the window.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub window_processes: BTreeMap<u64, u64>,
}
/// A list under a finger. The pane it took (or the application's own wheel use when
/// no pane can move there), where the finger and the list were when it took it, and
/// the last two finger samples with the world clock at each, from which the release
/// velocity is measured.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TouchScroll {
    pub window: u64,
    /// `None`: no published pane moves here, so the application's own wheel use
    /// (a grid by rows, a terminal's scrollback) is driven instead.
    pub pane: Option<String>,
    /// Where the drag started, which is where the application's wheel is aimed.
    pub at: (i32, i32),
    /// The pane's offset, and the finger's y, when the drag took it.
    pub origin: i32,
    pub anchor: i32,
    /// The pane's furthest offset and viewport height when it was taken.
    pub max: i32,
    pub height: u32,
    /// The finger's y already handed to the application's wheel.
    pub applied: i32,
    /// The latest finger sample and the one before it: (y, world clock in µs).
    pub last: (i32, u64),
    pub prev: Option<(i32, u64)>,
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
            pointer_cursor: None,
            pointer_press: None,
            touch_scroll: None,
            viewport: None,
            scrolled: false,
            window_processes: BTreeMap::new(),
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
            // Applications that follow the platform learn it from the desktop.
            self.machine_mut(id, &machine)?.desktop.theme = Some(theme);
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
        self.sessions.get(id).ok_or_else(|| {
            SimError::denied("unknown actor session").because(cw_protocol::reason::UNKNOWN_SESSION)
        })
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
                        let mut v = json!({"page":machine.browser.current_page(),"url":machine.browser.url(),"fields":machine.browser.tab().fields});
                        // The tab's console (page script output, uncaught errors,
                        // budget interruptions): the browser's diagnostics.
                        if !machine.browser.console().is_empty() {
                            v["console"] = serde_json::to_value(machine.browser.console())?;
                        }
                        v
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
            return Err(SimError::denied("action batch exceeds budget")
                .because(cw_protocol::reason::BUDGET_EXCEEDED));
        }
        let mut outcomes = Vec::with_capacity(actions.len());
        for (index, action) in actions.iter().enumerate() {
            // The envelope's success says nothing about the app-level consequence, so
            // bracket the dispatch with an actor-visible projection of the target.
            let permitted = config.machines.contains(&action.machine)
                && config.actions.contains(&action.family);
            if let Ok(m) = self.machine_mut(id, &action.machine) {
                m.scrolled = false;
            }
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
            } else if config.machines.contains(&action.machine) {
                Err(SimError::denied("action is not permitted")
                    .because(cw_protocol::reason::FAMILY_NOT_GRANTED))
            } else {
                Err(SimError::denied("action is not permitted")
                    .because(cw_protocol::reason::MACHINE_NOT_GRANTED))
            };
            // Windows and machine processes are two views of one fact; reconcile them
            // after every action, so a `kill` from the shell closes the window it named
            // and a window that opened is in `ps` before the next observation.
            if permitted {
                self.sync_window_processes(id, &action.machine)?;
            }
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
        // A page that asked to be refreshed (a music site's player bar, whose position
        // moves with the clock) is fetched again once its interval of world time has
        // passed, so what the next observation shows is what the site shows now.
        let machines: Vec<String> = self.session(id)?.machines.keys().cloned().collect();
        for machine in machines {
            self.refresh_pages(id, &machine, &config.actor);
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
        self.adopt(snapshot);
        Ok(())
    }
    /// The environment half of `restore`, for a caller that has already installed the
    /// kernel half. Everything `restore` checks is still checked; only the kernel
    /// checkpoint's own validation is the caller's, which is what keeps a fork from
    /// validating one checkpoint twice.
    fn restore_environment(&mut self, snapshot: &Snapshot) -> Result<()> {
        if snapshot.version != 1 {
            return Err(SimError::invalid("unsupported environment snapshot"));
        }
        self.validate_snapshot(snapshot)?;
        self.adopt(snapshot);
        Ok(())
    }
    fn adopt(&mut self, snapshot: &Snapshot) {
        self.sessions = snapshot.sessions.clone();
        self.next_session = snapshot.next_session;
        self.journal = snapshot.journal.clone();
        self.verify_steps = snapshot.verify_steps;
    }
    pub fn fork(&self, snapshot: &Snapshot) -> Result<Self> {
        // `fork_and_restore` is `Runtime::fork` and `Runtime::restore` in one call: the
        // fork used to restore into itself, which walked and compared the whole world
        // definition a second time for every branch.
        let runtime = self.runtime.fork_and_restore(&snapshot.kernel)?;
        let mut env = Self::new(runtime);
        env.extensions = self.extensions.clone();
        env.app_registry = self.app_registry.clone();
        env.observation_extensions = self.observation_extensions.clone();
        env.capture = self.capture.clone();
        env.restore_environment(snapshot)?;
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

/// The name a download is saved under: the one the response suggested, cut to the
/// characters every filesystem here accepts, or else one made from the URL.
fn download_file_name(suggested: &str, url: &str) -> String {
    let safe: String = suggested
        .chars()
        .map(|c| if c == ' ' { '-' } else { c })
        .filter(|c| c.is_alphanumeric() || matches!(c, '.' | '-' | '_'))
        .take(96)
        .collect();
    if safe.is_empty() || safe.starts_with('.') {
        download_name(url)
    } else {
        safe
    }
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
/// Flatten a refusal for the actor. The code is one of four, and the message is a
/// compile-time constant, so a failing action can never leak world state through its
/// error text. A `reason` from `cw_protocol::reason` survives — and carries that
/// vocabulary's own fixed message — so the refusal still says what to do about it.
/// Resident memory a desktop application is modelled to hold while a window of it is
/// open, over and above the document that window has loaded. Published, not measured:
/// the table is in `docs/shell.md` and an agent can predict every number in it.
fn window_footprint(app: &str) -> u64 {
    const MB: u64 = 1 << 20;
    match app {
        "browser" => 320 * MB,
        "code" => 180 * MB,
        "gimp" | "pixelmator" | "sketchbook" | "pinta" | "paint" => 140 * MB,
        "kdenlive" | "imovie" | "clipchamp" | "videoeditor" => 210 * MB,
        "freecad" | "kicad" => 240 * MB,
        "docs" | "spreadsheet" | "excel" | "database" => 120 * MB,
        "photos" | "preview" | "music" | "maps" => 90 * MB,
        "files" => 45 * MB,
        "editor" => 30 * MB,
        "terminal" => 12 * MB,
        "calculator" | "clock" | "weather" | "notes" | "contacts" => 24 * MB,
        _ => 60 * MB,
    }
}
fn actor_error(e: SimError) -> SimError {
    if let Some((name, code, message)) = e
        .reason
        .as_deref()
        .and_then(|r| cw_protocol::reason::ALL.iter().find(|(n, _, _)| *n == r))
    {
        return SimError::new(*code, *message).because(name);
    }
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
#[cfg(test)]
mod tests {
    use super::*;
    use cw_sdk::Registry;
    fn world() -> Environment {
        let definition=WorldDefinition::from_json(r#"{"id":"test","profiles":[{"id":"linux","family":"linux"}],"computers":[{"id":"a","profile":"linux","address":"10.0.0.1","user":"alice","initial_files":{"/home/alice/file":"visible"}},{"id":"b","profile":"linux","address":"10.0.0.2","user":"bob","initial_files":{"/home/bob/private":"OTHER_MACHINE_SECRET"}}],"metadata":{"evaluator_secret":"PRIVATE_OBJECTIVE_CANARY"}}"#).unwrap();
        Environment::new(Runtime::new(definition, 42, Registry::new()).unwrap())
    }
    /// `browser_view` writes the hash input itself so a tab's stack can resume from
    /// the state it left behind. What it writes has to be, byte for byte, what
    /// serialising the same tuple would write — otherwise every `ActionEffect::state`
    /// an actor is handed would quietly change value.
    #[test]
    fn browser_view_hashes_what_serializing_would() {
        use cw_browser::{Content, HistoryEntry, Tab};
        use cw_protocol::Page;
        let entry = |url: &str| HistoryEntry {
            url: url.into(),
            content: Content::Page(Page::new(url)),
            status: 200,
            images: BTreeMap::new(),
            image_errors: BTreeMap::new(),
            refresh: None,
        };
        // The composition this replaced, verbatim.
        let legacy = |b: &BrowserState| {
            let tabs: Vec<_> = b
                .tabs
                .iter()
                .map(|t| (t.history.as_slice(), t.position, &t.focused, &t.fields))
                .collect();
            cw_scene::digest(&(tabs, b.active, &b.zoom, &b.pending))
        };
        let mut state = BrowserState::default();
        state.tabs.clear();
        let check = |state: &BrowserState| {
            // Twice: cold, then with every cached hash state in play.
            assert_eq!(browser_view(state).0, legacy(state));
            assert_eq!(browser_view(state).0, legacy(state));
        };
        check(&state);
        state.tabs.push(Tab::default());
        check(&state);
        state.tabs[0].history.push(entry("/a"));
        state.tabs[0].history.push(entry("/b"));
        state.tabs[0].position = 1;
        state.tabs[0].focused = Some("q".into());
        state.tabs[0]
            .fields
            .insert("q".into(), "typed ünïcode".into());
        check(&state);
        // A second tab, whose stack follows the first one's bytes in the same hash.
        state.tabs.push(Tab::default());
        state.tabs[1].history.push(entry("/other"));
        state.tabs[1].history.push(entry("/tab"));
        state.active = 1;
        check(&state);
        // Changing the first tab moves what the second one is hashed after.
        state.tabs[0].history.push(entry("/c"));
        state.tabs[0].position = 2;
        check(&state);
        state.tabs[0].history[0].status = 404;
        check(&state);
        state.tabs[0].history.truncate(1);
        state.tabs[0].position = 0;
        check(&state);
        state.zoom.insert("example.com".into(), 150);
        state.pending = Some(cw_protocol::HttpRequest::get("http://example.com/"));
        check(&state);
        // Distinct states stay distinct.
        let one = browser_view(&state).0;
        state.tabs[0].history[0].url = "/moved".into();
        assert_ne!(one, browser_view(&state).0);
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
                style: None,
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
