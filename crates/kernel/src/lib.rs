//! One deterministic, synthetic-only semantic runtime. Bindings and worlds are consumers.
use cw_computer::{CommandResult, Computer, ShellHost};
use cw_determinism::{Clock, Determinism, Scheduler};
use cw_network::{Network, ResolvedRequest};
use cw_protocol::{EventRecord, HttpRequest, HttpResponse, Result, SimError, WorldDefinition};
use cw_sdk::{Registry, ServiceContext, ServiceEffect, ServiceEffectResult};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, sync::Arc};

pub const ENGINE_VERSION: &str = concat!("computerworld/", env!("CARGO_PKG_VERSION"));
const SNAPSHOT_VERSION: u32 = 1;
const MAX_EVENTS_PER_ADVANCE: usize = 100_000;

#[derive(Clone, Debug, Serialize, Deserialize)]
struct ServiceInstance {
    kind: String,
    seed: u64,
    state: Arc<Value>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
struct PendingHttp {
    id: String,
    machine: String,
    actor: String,
    request: HttpRequest,
    resolved: ResolvedRequest,
    reply: Option<ReplyTarget>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
struct ReplyTarget {
    service: String,
    token: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
enum Task {
    Http(PendingHttp),
    Delivery {
        pending: PendingHttp,
        result: Result<HttpResponse>,
    },
    Callback {
        service: String,
        actor: String,
        source: String,
        token: String,
        result: ServiceEffectResult,
    },
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ExternalEffect {
    pub id: String,
    pub generation: u64,
    pub authorization: cw_network::HostAuthorization,
    pub request: HttpRequest,
}
#[derive(Clone)]
struct LiveEffect {
    effect: ExternalEffect,
    machine: String,
    actor: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
struct State {
    clock: Clock,
    determinism: Determinism,
    computers: BTreeMap<String, Arc<Computer>>,
    network: Arc<Network>,
    services: BTreeMap<String, ServiceInstance>,
    scheduler: Scheduler<Task>,
    responses: BTreeMap<String, Result<HttpResponse>>,
    events: Arc<Vec<EventRecord>>,
}
/// Cheap complete kernel checkpoint. Actor/application sessions are owned by the environment.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Snapshot {
    version: u32,
    engine: String,
    definition: Arc<WorldDefinition>,
    #[serde(default)]
    baseline: Option<Arc<WorldDefinition>>,
    modules: BTreeMap<String, String>,
    state: Arc<State>,
    #[serde(default)]
    external_pending: bool,
}
impl Snapshot {
    pub fn to_json(&self) -> Result<String> {
        if self.external_pending {
            return Err(SimError::new(
                "external_effect_pending",
                "cannot export a live external continuation",
            ));
        }
        Ok(serde_json::to_string(self)?)
    }
    pub fn from_json(value: &str) -> Result<Self> {
        Ok(serde_json::from_str(value)?)
    }
    pub fn tick(&self) -> u64 {
        self.state.clock.now()
    }
    pub fn definition(&self) -> &WorldDefinition {
        &self.definition
    }
}
pub struct Runtime {
    definition: Arc<WorldDefinition>,
    baseline: Arc<WorldDefinition>,
    registry: Registry,
    state: Arc<State>,
    initial: Arc<State>,
    modules: BTreeMap<String, String>,
    live: BTreeMap<String, LiveEffect>,
    generation: u64,
}

fn determinism_error(message: String) -> SimError {
    SimError::new("determinism", message)
}
fn computer_error(message: String) -> SimError {
    SimError::new("computer", message)
}
/// The home folder the machine states, or the one a user of that name would have. The
/// trash lives under it, so every trash entry point starts here.
fn home_folder(computer: &Computer) -> String {
    computer
        .env
        .get("HOME")
        .cloned()
        .unwrap_or_else(|| format!("/home/{}", computer.user))
        .trim_end_matches('/')
        .to_owned()
}
/// The home whose trash `root` is. A desktop hands its own
/// `<home>/.local/share/Trash/files`, and taking the home back out of it keeps the
/// record beside the data even when the caller's idea of home differs from the
/// machine's; anything else falls back to the machine's own `HOME`.
fn trash_home(computer: &Computer, root: &str) -> String {
    root.trim_end_matches('/')
        .strip_suffix("/.local/share/Trash/files")
        .map(str::to_owned)
        .unwrap_or_else(|| home_folder(computer))
}
pub fn canonical_hash<T: Serialize>(value: &T) -> Result<String> {
    let canonical = serde_json::to_value(value)?;
    Ok(format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(&canonical)?)
    ))
}
impl Runtime {
    pub fn new(definition: WorldDefinition, seed: u64, registry: Registry) -> Result<Self> {
        definition.validate()?;
        let mut network = Network::with_seed(&definition, seed)?;
        let mut computers = BTreeMap::new();
        for computer in &definition.computers {
            computers.insert(
                computer.id.clone(),
                Arc::new(Computer::from_definition(
                    computer,
                    definition.profile(&computer.profile)?,
                )?),
            );
        }
        for service in &definition.services {
            if let Some(machine) = definition
                .computers
                .iter()
                .find(|c| c.node_id() == service.node)
            {
                let computer = Arc::make_mut(computers.get_mut(&machine.id).unwrap());
                let pid = computer.processes.spawn(
                    1,
                    &computer.user,
                    &format!("service {}", service.id),
                    0,
                );
                for port in cw_network::service_ports(service) {
                    computer
                        .processes
                        .own_listener(pid, format!("{}:{}", service.node, port))
                        .map_err(computer_error)?;
                }
                network.own_service_listener(&service.id, &service.node, pid)?;
            }
        }
        let mut determinism = Determinism::new(seed);
        let mut services = BTreeMap::new();
        let mut modules = BTreeMap::new();
        for service in &definition.services {
            let implementation = registry.service(&service.kind)?;
            modules.insert(service.kind.clone(), implementation.version().to_string());
            let instance_seed = determinism.next_u64(&format!("service/{}/initialize", service.id));
            let context = ServiceContext {
                actor: String::new(),
                source: String::new(),
                tick: 0,
                seed: instance_seed,
                instance: service.id.clone(),
            };
            let state = implementation.initialize(service.initial_state.clone(), &context)?;
            services.insert(
                service.id.clone(),
                ServiceInstance {
                    kind: service.kind.clone(),
                    seed: instance_seed,
                    state: Arc::new(state),
                },
            );
        }
        let state = Arc::new(State {
            clock: Clock::default(),
            determinism,
            computers,
            network: Arc::new(network),
            services,
            scheduler: Scheduler::default(),
            responses: BTreeMap::new(),
            events: Arc::new(vec![]),
        });
        Ok(Self {
            baseline: Arc::new(definition.clone()),
            definition: Arc::new(definition),
            registry,
            modules,
            live: BTreeMap::new(),
            generation: 0,
            initial: state.clone(),
            state,
        })
    }
    pub fn definition(&self) -> &WorldDefinition {
        &self.definition
    }
    /// Owner-only topology edit. Existing computer/service state is retained.
    /// Edits require drained continuations so in-flight routing is unambiguous.
    pub fn add_computer(
        &mut self,
        computer: cw_protocol::ComputerDefinition,
        node: cw_protocol::NetworkNode,
        links: Vec<cw_protocol::NetworkLink>,
    ) -> Result<()> {
        if self.pending() != 0 {
            return Err(SimError::invalid(
                "drain pending work before editing topology",
            ));
        }
        if computer.node_id() != node.id || computer.address != node.address {
            return Err(SimError::invalid("computer and network identity differ"));
        }
        if self
            .state
            .network
            .config
            .nodes
            .iter()
            .any(|n| n.id == node.id)
        {
            return Err(SimError::invalid("network node already exists"));
        }
        let edit = json!({"computer":computer,"node":node,"links":links});
        let mut definition = (*self.definition).clone();
        definition.computers.push(computer.clone());
        definition.network.nodes.push(node);
        definition.network.links.extend(links);
        definition.validate()?;
        let instance =
            Computer::from_definition(&computer, definition.profile(&computer.profile)?)?;
        let mut network = (*self.state.network).clone();
        network.reconfigure_topology(&definition)?;
        let state = Arc::make_mut(&mut self.state);
        state
            .computers
            .insert(computer.id.clone(), Arc::new(instance));
        state.network = Arc::new(network);
        self.definition = Arc::new(definition);
        self.event("topology.computer_added", Some(&computer.id), None, edit);
        Ok(())
    }
    /// Remove a computer and its network node; service hosts require migration first.
    pub fn remove_computer(&mut self, id: &str) -> Result<()> {
        if self.pending() != 0 {
            return Err(SimError::invalid(
                "drain pending work before editing topology",
            ));
        }
        let computer = self
            .definition
            .computers
            .iter()
            .find(|c| c.id == id)
            .ok_or_else(|| SimError::not_found("computer"))?;
        let node = computer.node_id().to_owned();
        let address = computer.address.clone();
        if self.definition.services.iter().any(|s| s.node == node) {
            return Err(SimError::invalid(
                "migrate hosted services before removing computer",
            ));
        }
        let mut definition = (*self.definition).clone();
        definition.computers.retain(|c| c.id != id);
        definition.network.nodes.retain(|n| n.id != node);
        definition
            .network
            .links
            .retain(|l| l.from != node && l.to != node);
        definition
            .network
            .routes
            .retain(|r| r.from != node && r.to != node && r.via.as_deref() != Some(node.as_str()));
        definition
            .network
            .dns
            .retain(|d| d.address != address && d.resolver.as_deref() != Some(&node));
        definition.validate()?;
        let mut network = (*self.state.network).clone();
        network.reconfigure_topology(&definition)?;
        let state = Arc::make_mut(&mut self.state);
        state.computers.remove(id);
        state.network = Arc::new(network);
        self.definition = Arc::new(definition);
        self.event(
            "topology.computer_removed",
            Some(id),
            None,
            json!({"node":node}),
        );
        Ok(())
    }
    pub fn seed(&self) -> u64 {
        self.state.determinism.seed()
    }
    pub fn tick(&self) -> u64 {
        self.state.clock.now()
    }
    pub fn pending(&self) -> usize {
        self.state.scheduler.len() + self.live.len()
    }
    pub fn events(&self) -> &[EventRecord] {
        &self.state.events
    }
    pub fn network(&self) -> &Network {
        &self.state.network
    }
    pub fn computer(&self, machine: &str) -> Result<&Computer> {
        self.state
            .computers
            .get(machine)
            .map(Arc::as_ref)
            .ok_or_else(|| SimError::not_found(format!("computer {machine}")))
    }
    pub fn service_state(&self, service: &str) -> Result<&Value> {
        self.state
            .services
            .get(service)
            .map(|s| s.state.as_ref())
            .ok_or_else(|| SimError::not_found(format!("service {service}")))
    }
    pub fn inspect(&self) -> Value {
        json!({"engine":ENGINE_VERSION,"definition":self.definition,"state":self.state})
    }
    fn event(&mut self, kind: &str, machine: Option<&str>, actor: Option<&str>, data: Value) {
        let state = Arc::make_mut(&mut self.state);
        let events = Arc::make_mut(&mut state.events);
        events.push(EventRecord {
            sequence: events.len() as u64,
            tick: state.clock.now(),
            kind: kind.into(),
            machine: machine.map(str::to_owned),
            actor: actor.map(str::to_owned),
            data,
        });
    }
    fn capture_network_events(&mut self, start: usize, actor: Option<&str>) {
        let traces = self.state.network.traces()[start..].to_vec();
        for trace in traces {
            self.event(
                &format!("network.{}", trace.kind),
                Some(&trace.source),
                actor,
                json!(trace),
            );
        }
    }
    pub fn record_event(
        &mut self,
        kind: &str,
        machine: Option<&str>,
        actor: Option<&str>,
        data: Value,
    ) {
        self.event(kind, machine, actor, data)
    }
    pub fn snapshot(&self) -> Snapshot {
        Snapshot {
            version: SNAPSHOT_VERSION,
            engine: ENGINE_VERSION.into(),
            definition: self.definition.clone(),
            baseline: Some(self.baseline.clone()),
            modules: self.modules.clone(),
            state: self.state.clone(),
            external_pending: !self.live.is_empty(),
        }
    }
    fn validate_snapshot(&self, snapshot: &Snapshot) -> Result<()> {
        if snapshot.external_pending {
            return Err(SimError::new(
                "external_effect_pending",
                "checkpoint contains live host effects",
            ));
        }
        if snapshot.version != SNAPSHOT_VERSION || snapshot.engine != ENGINE_VERSION {
            return Err(SimError::invalid("incompatible kernel checkpoint version"));
        }
        snapshot.definition.validate()?;
        if **snapshot.baseline.as_ref().unwrap_or(&snapshot.definition) != *self.baseline {
            return Err(SimError::invalid(
                "checkpoint belongs to a different world definition",
            ));
        }
        if snapshot.modules != self.modules {
            return Err(SimError::invalid(
                "checkpoint service module versions differ",
            ));
        }
        snapshot
            .state
            .scheduler
            .validate(snapshot.tick())
            .map_err(determinism_error)?;
        for computer in snapshot.state.computers.values() {
            if computer
                .processes
                .next_deadline()
                .is_some_and(|due| due < snapshot.tick())
            {
                return Err(SimError::invalid(
                    "checkpoint process deadline is in the past",
                ));
            }
        }
        for event in snapshot.state.scheduler.iter() {
            if event.due < snapshot.state.clock.now() {
                return Err(SimError::invalid("checkpoint continuation is in the past"));
            }
            match &event.value {
                Task::Http(p) | Task::Delivery { pending: p, .. } => {
                    if !snapshot.state.services.contains_key(&p.resolved.service_id) {
                        return Err(SimError::invalid("checkpoint continuation service missing"));
                    }
                }
                Task::Callback { service, .. } => {
                    if !snapshot.state.services.contains_key(service) {
                        return Err(SimError::invalid("checkpoint callback service missing"));
                    }
                }
            }
        }
        if snapshot.state.computers.len() != snapshot.definition.computers.len()
            || snapshot.state.services.len() != snapshot.definition.services.len()
        {
            return Err(SimError::invalid("checkpoint instance set differs"));
        }
        for computer in snapshot.state.computers.values() {
            computer.validate()?;
        }
        for c in &snapshot.definition.computers {
            if !snapshot.state.computers.contains_key(&c.id) {
                return Err(SimError::invalid("checkpoint missing computer"));
            }
        }
        for s in &snapshot.definition.services {
            if snapshot
                .state
                .services
                .get(&s.id)
                .is_none_or(|v| v.kind != s.kind)
            {
                return Err(SimError::invalid(
                    "checkpoint missing service or wrong kind",
                ));
            }
        }
        Ok(())
    }
    pub fn restore(&mut self, snapshot: &Snapshot) -> Result<()> {
        self.validate_snapshot(snapshot)?;
        self.generation = self
            .generation
            .checked_add(1)
            .ok_or_else(|| SimError::invalid("host generation exhausted"))?;
        self.live.clear();
        self.definition = snapshot.definition.clone();
        self.state = snapshot.state.clone();
        Ok(())
    }
    pub fn fork(&self, snapshot: &Snapshot) -> Result<Self> {
        self.validate_snapshot(snapshot)?;
        Ok(Self {
            definition: snapshot.definition.clone(),
            baseline: self.baseline.clone(),
            registry: self.registry.clone(),
            modules: self.modules.clone(),
            state: snapshot.state.clone(),
            initial: self.initial.clone(),
            live: BTreeMap::new(),
            generation: 0,
        })
    }
    pub fn reset(&mut self, seed: u64) -> Result<()> {
        let generation = self
            .generation
            .checked_add(1)
            .ok_or_else(|| SimError::invalid("host generation exhausted"))?;
        if seed == self.initial.determinism.seed() {
            self.definition = self.baseline.clone();
            self.state = self.initial.clone();
            self.live.clear();
            self.generation = generation;
        } else {
            let mut fresh = Self::new((*self.baseline).clone(), seed, self.registry.clone())?;
            fresh.generation = generation;
            *self = fresh;
        }
        Ok(())
    }
    pub fn state_hash(&self) -> Result<String> {
        let s = &self.state;
        canonical_hash(&(
            ENGINE_VERSION,
            &self.definition,
            &self.modules,
            &s.clock,
            &s.determinism,
            &s.computers,
            s.network.semantic_state(),
            &s.services,
            &s.scheduler,
            &s.responses,
            s.events.len(),
        ))
    }
    pub fn try_snapshot(&self) -> Result<Snapshot> {
        if !self.live.is_empty() {
            return Err(SimError::new(
                "external_effect_pending",
                "complete live host effects before portable snapshot or fork",
            ));
        }
        Ok(self.snapshot())
    }
    pub fn begin_external(
        &mut self,
        machine: &str,
        actor: &str,
        request: HttpRequest,
        resolved_addresses: Vec<String>,
    ) -> Result<ExternalEffect> {
        let source = self
            .definition
            .computers
            .iter()
            .find(|c| c.id == machine)
            .ok_or_else(|| SimError::not_found("external effect source machine"))?
            .node_id();
        let authorization = self.state.network.authorize_host(
            source,
            &request.url,
            resolved_addresses,
            self.tick(),
        )?;
        let id = Arc::make_mut(&mut self.state)
            .determinism
            .next_id("external")
            .map_err(determinism_error)?;
        let effect = ExternalEffect {
            id: id.clone(),
            generation: self.generation,
            authorization,
            request,
        };
        self.live.insert(
            id.clone(),
            LiveEffect {
                effect: effect.clone(),
                machine: machine.into(),
                actor: actor.into(),
            },
        );
        self.event(
            "external.submitted",
            Some(machine),
            Some(actor),
            json!({"id":id,"request":effect.request,"authorization":effect.authorization}),
        );
        Ok(effect)
    }
    pub fn complete_external(
        &mut self,
        effect: &ExternalEffect,
        result: Result<HttpResponse>,
    ) -> Result<()> {
        let live = self
            .live
            .get(&effect.id)
            .ok_or_else(|| SimError::invalid("stale or unknown external effect"))?;
        if effect.generation != self.generation || live.effect.generation != effect.generation {
            return Err(SimError::invalid("stale external effect generation"));
        }
        let result = if self.tick() > live.effect.authorization.deadline {
            Err(SimError::new("timeout", "external effect deadline elapsed"))
        } else if result
            .as_ref()
            .is_ok_and(|r| r.body.len() > live.effect.authorization.max_response_bytes)
        {
            Err(SimError::denied("host response exceeds byte budget"))
        } else if result
            .as_ref()
            .is_ok_and(|r| !(100..=599).contains(&r.status))
        {
            Err(SimError::invalid("host response has invalid HTTP status"))
        } else {
            result
        };
        let live = self.live.remove(&effect.id).unwrap();
        self.event(
            "external.completed",
            Some(&live.machine),
            Some(&live.actor),
            json!({"id":effect.id,"response":result}),
        );
        Arc::make_mut(&mut self.state)
            .responses
            .insert(effect.id.clone(), result);
        Ok(())
    }
    pub fn read_file(&self, machine: &str, path: &str) -> Result<Vec<u8>> {
        let computer = self.computer(machine)?;
        computer
            .vfs
            .read_as(&computer.resolve(path), &computer.user)
            .map_err(|e| computer_error(e.to_string()))
    }
    pub fn write_file(
        &mut self,
        machine: &str,
        actor: &str,
        path: &str,
        bytes: &[u8],
    ) -> Result<()> {
        let tick = self.tick();
        let state = Arc::make_mut(&mut self.state);
        let computer = Arc::make_mut(
            state
                .computers
                .get_mut(machine)
                .ok_or_else(|| SimError::not_found(format!("computer {machine}")))?,
        );
        computer
            .vfs
            .write_as(&computer.resolve(path), bytes, &computer.user, tick)
            .map_err(|e| computer_error(e.to_string()))?;
        self.event(
            "filesystem.write",
            Some(machine),
            Some(actor),
            json!({"path":path,"bytes":bytes}),
        );
        Ok(())
    }
    /// Create a folder and any missing parent, as the user who owns the machine.
    pub fn create_directory(&mut self, machine: &str, actor: &str, path: &str) -> Result<()> {
        let tick = self.tick();
        let state = Arc::make_mut(&mut self.state);
        let computer = Arc::make_mut(
            state
                .computers
                .get_mut(machine)
                .ok_or_else(|| SimError::not_found(format!("computer {machine}")))?,
        );
        let resolved = computer.resolve(path);
        let user = computer.user.clone();
        computer
            .vfs
            .mkdir_all_as(&resolved, &user, tick)
            .map_err(|e| computer_error(e.to_string()))?;
        self.event(
            "filesystem.mkdir",
            Some(machine),
            Some(actor),
            json!({ "path": path }),
        );
        Ok(())
    }
    /// Create an empty file, as the user who owns the machine. Refuses an existing
    /// path: a file manager's New must never overwrite what is already there.
    pub fn create_file(&mut self, machine: &str, actor: &str, path: &str) -> Result<()> {
        let tick = self.tick();
        let computer = self.computer_mut(machine)?;
        let resolved = computer.resolve(path);
        if computer.vfs.exists(&resolved) {
            return Err(computer_error(format!("already exists: {path}")));
        }
        let user = computer.user.clone();
        computer
            .vfs
            .write_as(&resolved, &[], &user, tick)
            .map_err(|e| computer_error(e.to_string()))?;
        self.event(
            "filesystem.write",
            Some(machine),
            Some(actor),
            json!({"path":path,"bytes":Vec::<u8>::new()}),
        );
        Ok(())
    }
    /// Copy a file or folder tree. The same access checks a read and a write make, so a
    /// file manager cannot copy what its user could not have read.
    pub fn copy_path(&mut self, machine: &str, actor: &str, from: &str, to: &str) -> Result<()> {
        let tick = self.tick();
        let computer = self.computer_mut(machine)?;
        let (source, target) = (computer.resolve(from), computer.resolve(to));
        let user = computer.user.clone();
        computer
            .vfs
            .copy_as(&source, &target, &user, tick)
            .map_err(|e| computer_error(e.to_string()))?;
        self.event(
            "filesystem.copy",
            Some(machine),
            Some(actor),
            json!({"from":from,"to":to}),
        );
        Ok(())
    }
    /// Move or rename, with the write checks on both parents that a shell `mv` makes.
    pub fn move_path(&mut self, machine: &str, actor: &str, from: &str, to: &str) -> Result<()> {
        let computer = self.computer_mut(machine)?;
        let (source, target) = (computer.resolve(from), computer.resolve(to));
        if computer.vfs.exists(&target) {
            return Err(computer_error(format!("already exists: {to}")));
        }
        let user = computer.user.clone();
        computer
            .vfs
            .rename_as(&source, &target, &user)
            .map_err(|e| computer_error(e.to_string()))?;
        self.event(
            "filesystem.move",
            Some(machine),
            Some(actor),
            json!({"from":from,"to":to}),
        );
        Ok(())
    }
    /// File `path` under `trash`. This is what a desktop's Delete does, and it is the
    /// same FreeDesktop trash the shell's `trash` command writes: the data moves into
    /// `~/.local/share/Trash/files` under a name that is free, and a
    /// `~/.local/share/Trash/info/NAME.trashinfo` record says where it came from and
    /// when. Nothing a user throws away is destroyed, and everything can be put back.
    pub fn trash_path(
        &mut self,
        machine: &str,
        actor: &str,
        path: &str,
        trash: &str,
    ) -> Result<String> {
        let tick = self.tick();
        let computer = self.computer_mut(machine)?;
        let source = computer.resolve(path);
        let root = computer.resolve(trash);
        if root == source || root.starts_with(&format!("{}/", source.trim_end_matches('/'))) {
            return Err(SimError::invalid("cannot move the trash into itself"));
        }
        let home = trash_home(computer, &root);
        let user = computer.user.clone();
        let name = cw_computer::trash::put(&mut computer.vfs, &user, &home, &source, tick)
            .map_err(computer_error)?;
        let target = format!("{}/{name}", cw_computer::trash::files_dir(&home));
        self.event(
            "filesystem.trash",
            Some(machine),
            Some(actor),
            json!({"path":path,"trash":target}),
        );
        Ok(target)
    }
    /// Put one trashed thing back where it came from, named by its original path, its
    /// name in the trash, or the path it now has under `files/`. The `.trashinfo`
    /// record is what makes this possible, so only what the trash really recorded can
    /// be restored — an ambiguous or missing query names the candidates rather than
    /// guessing which of them the user meant.
    pub fn restore_path(&mut self, machine: &str, actor: &str, query: &str) -> Result<String> {
        let tick = self.tick();
        let computer = self.computer_mut(machine)?;
        let home = home_folder(computer);
        // A file manager selects a row, so what it has in hand is the path under
        // `files/`; the trash records names, so reduce one to the other.
        let files = format!("{}/", cw_computer::trash::files_dir(&home));
        let wanted = match query.strip_prefix(&files) {
            Some(name) => name.trim_end_matches('/'),
            None => query.trim_end_matches('/'),
        };
        let mut found = cw_computer::trash::matching(&computer.vfs, &home, wanted);
        if found.is_empty() {
            return Err(SimError::not_found(format!(
                "nothing in the trash matches '{query}'"
            )));
        }
        if found.len() > 1 {
            let candidates: Vec<&str> = found.iter().map(|e| e.original.as_str()).collect();
            return Err(SimError::invalid(format!(
                "more than one trashed item matches '{query}': {}",
                candidates.join(", ")
            )));
        }
        let entry = found.remove(0);
        let from = entry.path.clone();
        let user = computer.user.clone();
        let original =
            cw_computer::trash::restore(&mut computer.vfs, &user, &home, &entry, false, tick)
                .map_err(computer_error)?;
        self.event(
            "filesystem.restore",
            Some(machine),
            Some(actor),
            json!({"path":original,"from":from}),
        );
        Ok(original)
    }
    /// Throw the whole trash away for good, and say how many things went. This is the
    /// one operation in the file manager that really destroys data, so it is its own
    /// entry point rather than a flag on a delete.
    pub fn empty_trash(&mut self, machine: &str, actor: &str) -> Result<usize> {
        let computer = self.computer_mut(machine)?;
        let home = home_folder(computer);
        let user = computer.user.clone();
        let emptied =
            cw_computer::trash::empty(&mut computer.vfs, &user, &home).map_err(computer_error)?;
        self.event(
            "filesystem.trash_empty",
            Some(machine),
            Some(actor),
            json!({"emptied":emptied}),
        );
        Ok(emptied)
    }
    /// The machine, ready to mutate. Same unwrapping every filesystem entry point does.
    fn computer_mut(&mut self, machine: &str) -> Result<&mut Computer> {
        let state = Arc::make_mut(&mut self.state);
        Ok(Arc::make_mut(state.computers.get_mut(machine).ok_or_else(
            || SimError::not_found(format!("computer {machine}")),
        )?))
    }
    /// A line typed at a terminal: standard input is that terminal, so a bare
    /// `python3` or `node` starts a console and a program may stop for a line
    /// that has not been typed yet (see `Computer::session_prompt`).
    pub fn execute_at_terminal(
        &mut self,
        machine: &str,
        actor: &str,
        command: &str,
    ) -> Result<CommandResult> {
        self.computer_mut(machine)?.tty = true;
        let result = self.execute(machine, actor, command);
        if let Ok(c) = self.computer_mut(machine) {
            c.tty = false;
        }
        result
    }

    /// Start the machine process an open application window runs as. An application on
    /// the screen is something the machine is running, so it is in the process table:
    /// `ps` lists it and `kill` really ends it. `holding` is what the window has open
    /// on top of the program's own footprint, in bytes.
    pub fn start_window_process(
        &mut self,
        machine: &str,
        command: &str,
        holding: u64,
    ) -> Result<u64> {
        let tick = self.tick();
        let computer = self.computer_mut(machine)?;
        let user = computer.user.clone();
        // A window is not on a terminal; it is on the display.
        let pid = computer.processes.spawn(1, &user, command, tick);
        computer
            .processes
            .hold(pid, holding)
            .map_err(computer_error)?;
        Ok(pid)
    }
    /// End a window's process, as closing the window does.
    pub fn end_window_process(&mut self, machine: &str, pid: u64) -> Result<()> {
        let tick = self.tick();
        let computer = self.computer_mut(machine)?;
        if computer.processes.get(pid).is_some() {
            let _ = computer.processes.exit(pid, 0, tick);
            let _ = computer.processes.wait(1, pid);
        }
        self.close_process_network(machine, pid);
        Ok(())
    }
    /// Whether a pid is still a live process on `machine`.
    pub fn process_alive(&self, machine: &str, pid: u64) -> bool {
        self.computer(machine).is_ok_and(|c| {
            c.processes.get(pid).is_some_and(|p| {
                !matches!(
                    p.state,
                    cw_computer::ProcessState::Zombie { .. }
                        | cw_computer::ProcessState::Exited { .. }
                )
            })
        })
    }
    /// Serve one Run and Debug request on `machine`: the machine hands it to the debug
    /// adapter for the program's runtime, or says why it has none.
    pub fn debug(
        &mut self,
        machine: &str,
        actor: &str,
        request: &cw_protocol::debug::Request,
    ) -> Result<cw_protocol::debug::Reply> {
        let tick = self.tick();
        self.event(
            "debug.request",
            Some(machine),
            Some(actor),
            serde_json::to_value(request).unwrap_or_default(),
        );
        self.computer_mut(machine)?
            .debug(tick, request)
            .map_err(SimError::invalid)
    }
    pub fn execute(&mut self, machine: &str, actor: &str, command: &str) -> Result<CommandResult> {
        self.computer(machine)?;
        self.event(
            "terminal.input",
            Some(machine),
            Some(actor),
            json!({"command":command}),
        );
        let mut computer = Arc::make_mut(&mut self.state)
            .computers
            .remove(machine)
            .unwrap();
        let tick = self.tick();
        let result = Arc::make_mut(&mut computer).execute(
            command,
            tick,
            &mut RuntimeShell {
                runtime: self,
                machine: machine.into(),
                actor: actor.into(),
            },
        );
        Arc::make_mut(&mut self.state)
            .computers
            .insert(machine.into(), computer);
        self.event("terminal.output", Some(machine), Some(actor), json!(result));
        Ok(result)
    }
    /// Run `command` as a shell session whose working directory is `cwd`, the way an
    /// application's integrated terminal is its own process: `cd` inside it moves the
    /// session, never the machine's shell. Returns the result and the session's working
    /// directory afterwards.
    pub fn execute_in(
        &mut self,
        machine: &str,
        actor: &str,
        cwd: &str,
        command: &str,
    ) -> Result<(CommandResult, String)> {
        let dir = {
            let computer = self.computer(machine)?;
            let dir = computer.resolve(cwd);
            computer
                .vfs
                .check_access(&dir, &computer.user, true, false, true)
                .map_err(|e| computer_error(e.to_string()))?;
            if !computer.vfs.stat(&dir).is_ok_and(|m| m.is_dir) {
                return Err(SimError::not_found("directory"));
            }
            dir
        };
        let saved = std::mem::replace(&mut self.computer_mut(machine)?.cwd, dir);
        let result = self.execute(machine, actor, command);
        let computer = self.computer_mut(machine)?;
        let after = std::mem::replace(&mut computer.cwd, saved);
        Ok((result?, after))
    }
    pub fn submit_http(
        &mut self,
        machine: &str,
        actor: &str,
        request: HttpRequest,
    ) -> Result<String> {
        if !self.definition.computers.iter().any(|c| c.id == machine) {
            return Err(SimError::not_found(format!("computer {machine}")));
        }
        self.queue_http(machine, actor, request, None)
    }
    fn queue_http(
        &mut self,
        source: &str,
        actor: &str,
        request: HttpRequest,
        reply: Option<ReplyTarget>,
    ) -> Result<String> {
        let tick = self.tick();
        let node = self
            .definition
            .computers
            .iter()
            .find(|c| c.id == source)
            .map(|c| c.node_id())
            .unwrap_or(source)
            .to_owned();
        let trace_start = self.state.network.traces().len();
        let prepared = Arc::make_mut(&mut Arc::make_mut(&mut self.state).network)
            .prepare_http(&node, &request, tick);
        self.capture_network_events(trace_start, Some(actor));
        let resolved = match prepared {
            Ok(v) => v,
            Err(e) => {
                self.event(
                    "http.rejected",
                    Some(source),
                    Some(actor),
                    json!({"request":request,"error":e}),
                );
                return Err(e.into());
            }
        };
        let state = Arc::make_mut(&mut self.state);
        let id = state
            .determinism
            .next_id("http")
            .map_err(determinism_error)?;
        let due = resolved.ready_at;
        state
            .scheduler
            .schedule(
                tick,
                due,
                1,
                Task::Http(PendingHttp {
                    id: id.clone(),
                    machine: source.into(),
                    actor: actor.into(),
                    request: request.clone(),
                    resolved,
                    reply,
                }),
            )
            .map_err(determinism_error)?;
        self.event(
            "http.submitted",
            Some(source),
            Some(actor),
            json!({"id":id,"request":request,"due":due}),
        );
        Ok(id)
    }
    fn complete_http(&mut self, pending: PendingHttp) -> Result<()> {
        let tick = self.tick();
        let mut effects = vec![];
        let result: Result<HttpResponse> = (|| {
            self.state.network.validate_delivery(&pending.resolved)?;
            let instance = self
                .state
                .services
                .get(&pending.resolved.service_id)
                .ok_or_else(|| SimError::not_found("destination service instance"))?;
            let implementation = self.registry.service(&instance.kind)?;
            let context = ServiceContext {
                actor: pending.actor.clone(),
                source: pending.machine.clone(),
                tick,
                seed: instance.seed,
                instance: pending.resolved.service_id.clone(),
            };
            let mut candidate = (*instance.state).clone();
            let transition =
                implementation.handle_with_effects(&mut candidate, &context, &pending.request)?;
            self.validate_effects(&transition.effects)?;
            Arc::make_mut(&mut self.state)
                .services
                .get_mut(&pending.resolved.service_id)
                .unwrap()
                .state = Arc::new(candidate);
            effects = transition.effects;
            Ok(transition.response)
        })();
        self.dispatch_effects(&pending.resolved.service_id, &pending.actor, effects)?;
        let trace_start = self.state.network.traces().len();
        let (due, result) = match result {
            Ok(response) => match Arc::make_mut(&mut Arc::make_mut(&mut self.state).network)
                .finish_http(&pending.resolved, &response, tick)
            {
                Ok(due) => (due, Ok(response)),
                Err(e) => (tick, Err(e.into())),
            },
            Err(e) => {
                Arc::make_mut(&mut Arc::make_mut(&mut self.state).network).fail_http(
                    &pending.machine,
                    &pending.request.url,
                    &e.to_string(),
                    tick,
                );
                (tick, Err(e))
            }
        };
        self.capture_network_events(trace_start, Some(&pending.actor));
        Arc::make_mut(&mut self.state)
            .scheduler
            .schedule(tick, due, 2, Task::Delivery { pending, result })
            .map_err(determinism_error)?;
        Ok(())
    }
    fn deliver_http(&mut self, pending: PendingHttp, result: Result<HttpResponse>) -> Result<()> {
        self.event(
            "http.completed",
            Some(&pending.machine),
            Some(&pending.actor),
            json!({"id":pending.id,"response":result}),
        );
        if let Some(reply) = pending.reply {
            let tick = self.tick();
            Arc::make_mut(&mut self.state)
                .scheduler
                .schedule(
                    tick,
                    tick,
                    3,
                    Task::Callback {
                        service: reply.service,
                        actor: pending.actor,
                        source: pending.resolved.destination,
                        token: reply.token,
                        result: ServiceEffectResult::Http { result },
                    },
                )
                .map_err(determinism_error)?;
        } else {
            Arc::make_mut(&mut self.state)
                .responses
                .insert(pending.id, result);
        }
        Ok(())
    }
    fn validate_effects(&self, effects: &[ServiceEffect]) -> Result<()> {
        if effects.len() > 1024 {
            return Err(SimError::new("budget", "too many service effects"));
        }
        for e in effects {
            if let ServiceEffect::Schedule { delay_us, .. } = e {
                self.tick()
                    .checked_add(*delay_us)
                    .ok_or_else(|| SimError::invalid("timer overflow"))?;
            }
        }
        Ok(())
    }
    fn dispatch_effects(
        &mut self,
        service: &str,
        actor: &str,
        effects: Vec<ServiceEffect>,
    ) -> Result<()> {
        let source = self
            .definition
            .services
            .iter()
            .find(|s| s.id == service)
            .ok_or_else(|| SimError::not_found("effect source service"))?
            .node
            .clone();
        for effect in effects {
            let tick = self.tick();
            match effect {
                ServiceEffect::Http {
                    request,
                    reply_token,
                } => {
                    if let Err(error) = self.queue_http(
                        &source,
                        actor,
                        request,
                        Some(ReplyTarget {
                            service: service.into(),
                            token: reply_token.clone(),
                        }),
                    ) {
                        Arc::make_mut(&mut self.state)
                            .scheduler
                            .schedule(
                                tick,
                                tick,
                                3,
                                Task::Callback {
                                    service: service.into(),
                                    actor: actor.into(),
                                    source: source.clone(),
                                    token: reply_token,
                                    result: ServiceEffectResult::Http { result: Err(error) },
                                },
                            )
                            .map_err(determinism_error)?;
                    }
                }
                ServiceEffect::Schedule {
                    delay_us,
                    token,
                    data,
                } => {
                    Arc::make_mut(&mut self.state)
                        .scheduler
                        .schedule(
                            tick,
                            tick + delay_us,
                            3,
                            Task::Callback {
                                service: service.into(),
                                actor: actor.into(),
                                source: source.clone(),
                                token,
                                result: ServiceEffectResult::Timer { data },
                            },
                        )
                        .map_err(determinism_error)?;
                }
                ServiceEffect::Emit { name, data } => self.event(
                    "service.emitted",
                    Some(&source),
                    Some(actor),
                    json!({"service":service,"name":name,"data":data}),
                ),
            }
        }
        Ok(())
    }
    fn callback(
        &mut self,
        service: &str,
        actor: &str,
        source: &str,
        token: &str,
        result: &ServiceEffectResult,
    ) -> Result<()> {
        let instance = self
            .state
            .services
            .get(service)
            .ok_or_else(|| SimError::not_found("callback service"))?;
        let context = ServiceContext {
            actor: actor.into(),
            source: source.into(),
            tick: self.tick(),
            seed: instance.seed,
            instance: service.into(),
        };
        let mut candidate = (*instance.state).clone();
        let transition = self
            .registry
            .service(&instance.kind)?
            .on_effect(&mut candidate, &context, token, result)
            .and_then(|effects| {
                self.validate_effects(&effects)?;
                Ok(effects)
            });
        match transition {
            Ok(effects) => {
                Arc::make_mut(&mut self.state)
                    .services
                    .get_mut(service)
                    .unwrap()
                    .state = Arc::new(candidate);
                self.dispatch_effects(service, actor, effects)?;
            }
            Err(error) => self.event(
                "service.callback_failed",
                Some(source),
                Some(actor),
                json!({"service":service,"token":token,"error":error}),
            ),
        }
        Ok(())
    }
    pub fn take_response(&mut self, id: &str) -> Result<Option<HttpResponse>> {
        if let Some(response) = Arc::make_mut(&mut self.state).responses.remove(id) {
            return response.map(Some);
        }
        if self.live.contains_key(id)
            || self.state.scheduler.iter().any(|e| match &e.value {
                Task::Http(p) | Task::Delivery { pending: p, .. } => p.id == id,
                _ => false,
            })
        {
            return Ok(None);
        }
        Err(SimError::not_found(format!("request {id}")))
    }
    /// Advance at most 100,000 continuations. Budget exhaustion leaves resumable work.
    pub fn advance(&mut self, ticks: u64) -> Result<()> {
        let through = self
            .tick()
            .checked_add(ticks)
            .ok_or_else(|| SimError::new("determinism", "logical clock overflow"))?;
        if self
            .state
            .scheduler
            .iter()
            .filter(|e| e.due <= through)
            .count()
            > MAX_EVENTS_PER_ADVANCE
        {
            return Err(SimError::new("budget", "scheduler event budget exhausted"));
        }
        let mut processed = 0;
        loop {
            let next = self
                .state
                .scheduler
                .next_due()
                .into_iter()
                .chain(
                    self.state
                        .computers
                        .values()
                        .filter_map(|c| c.processes.next_deadline()),
                )
                .min();
            let Some(due) = next.filter(|due| *due <= through) else {
                break;
            };
            if processed >= MAX_EVENTS_PER_ADVANCE {
                return Err(SimError::new(
                    "budget",
                    "scheduler event budget exhausted; continuations remain",
                ));
            }
            processed += 1;
            Arc::make_mut(&mut self.state)
                .clock
                .advance_to(due)
                .map_err(determinism_error)?;
            self.advance_computers(due);
            Arc::make_mut(&mut Arc::make_mut(&mut self.state).network).advance(due);
            if let Some(event) = Arc::make_mut(&mut self.state).scheduler.pop_due(due) {
                match event.value {
                    Task::Http(p) => self.complete_http(p)?,
                    Task::Delivery { pending, result } => self.deliver_http(pending, result)?,
                    Task::Callback {
                        service,
                        actor,
                        source,
                        token,
                        result,
                    } => self.callback(&service, &actor, &source, &token, &result)?,
                }
            }
        }
        Arc::make_mut(&mut self.state)
            .clock
            .advance_to(through)
            .map_err(determinism_error)?;
        Arc::make_mut(&mut Arc::make_mut(&mut self.state).network).advance(through);
        Ok(())
    }
    fn advance_computers(&mut self, tick: u64) {
        let mut exited = vec![];
        for computer in Arc::make_mut(&mut self.state).computers.values_mut() {
            // Avoid detaching a shared computer when no process is due.
            if !computer
                .processes
                .next_deadline()
                .is_some_and(|due| due <= tick)
            {
                continue;
            }
            let computer = Arc::make_mut(computer);
            for pid in computer.processes.advance(tick) {
                if computer.processes.get(pid).is_some_and(|p| {
                    matches!(
                        p.state,
                        cw_computer::ProcessState::Exited { .. }
                            | cw_computer::ProcessState::Zombie { .. }
                    )
                }) {
                    exited.push((computer.id.clone(), pid));
                }
            }
        }
        for (machine, pid) in exited {
            self.close_process_network(&machine, pid);
        }
    }
    /// The network node a computer sits on.
    fn node_of(&self, machine: &str) -> String {
        self.definition
            .computers
            .iter()
            .find(|c| c.id == machine)
            .map(|c| c.node_id())
            .unwrap_or(machine)
            .to_owned()
    }
    fn close_process_network(&mut self, machine: &str, pid: u64) {
        let node = self.node_of(machine);
        let tick = self.tick();
        let start = self.state.network.traces().len();
        Arc::make_mut(&mut Arc::make_mut(&mut self.state).network).close_process(&node, pid, tick);
        self.capture_network_events(start, None);
    }
    pub fn http(
        &mut self,
        machine: &str,
        actor: &str,
        request: HttpRequest,
    ) -> Result<HttpResponse> {
        let id = self.submit_http(machine, actor, request)?;
        loop {
            if let Some(response) = self.take_response(&id)? {
                return Ok(response);
            }
            let due = self
                .state
                .scheduler
                .next_due()
                .ok_or_else(|| SimError::invalid("missing HTTP continuation"))?;
            self.advance(due - self.tick())?;
        }
    }
}
struct RuntimeShell<'a> {
    runtime: &'a mut Runtime,
    machine: String,
    actor: String,
}
impl ShellHost for RuntimeShell<'_> {
    fn http(&mut self, request: HttpRequest) -> std::result::Result<HttpResponse, String> {
        self.runtime
            .http(&self.machine, &self.actor, request)
            .map_err(|e| e.to_string())
    }
    fn start_service(&mut self, name: &str, pid: u64) -> std::result::Result<String, String> {
        let node = self
            .runtime
            .definition
            .computers
            .iter()
            .find(|c| c.id == self.machine)
            .map(|c| c.node_id())
            .unwrap_or(&self.machine);
        let service = self
            .runtime
            .definition
            .services
            .iter()
            .find(|s| s.id == name && s.node == node)
            .ok_or_else(|| format!("service {name} is not placed on {}", self.machine))?
            .clone();
        Arc::make_mut(&mut Arc::make_mut(&mut self.runtime.state).network)
            .start_service_listener(&service, Some(pid))
            .map_err(|e| e.to_string())?;
        self.runtime.event(
            "service.started",
            Some(&self.machine),
            Some(&self.actor),
            json!({"service":name,"pid":pid}),
        );
        Ok(format!("{}:{}", service.node, service.port))
    }
    fn advance(&mut self, ticks: u64) -> std::result::Result<u64, String> {
        self.runtime.advance(ticks).map_err(|e| e.to_string())?;
        Ok(self.runtime.tick())
    }
    fn entropy(&mut self) -> u64 {
        Arc::make_mut(&mut self.runtime.state)
            .determinism
            .next_u64(&format!("computer/{}/entropy", self.machine))
    }
    fn cleanup_process(&mut self, pid: u64) {
        self.runtime.close_process_network(&self.machine, pid);
    }
    fn now_tick(&self) -> Option<u64> {
        Some(self.runtime.tick())
    }
    fn http_exchange(
        &mut self,
        request: HttpRequest,
    ) -> std::result::Result<HttpResponse, cw_computer::NetFailure> {
        self.runtime
            .http(&self.machine, &self.actor, request)
            .map_err(|e| cw_computer::NetFailure {
                code: e.code,
                message: e.message,
            })
    }
    fn resolve_name(
        &mut self,
        name: &str,
    ) -> std::result::Result<Vec<String>, cw_computer::NetFailure> {
        let node = self.runtime.node_of(&self.machine);
        let tick = self.runtime.tick();
        let start = self.runtime.state.network.traces().len();
        let r = Arc::make_mut(&mut Arc::make_mut(&mut self.runtime.state).network)
            .resolve(&node, name, tick);
        self.runtime
            .capture_network_events(start, Some(&self.actor));
        r.map_err(|e| {
            let e = SimError::from(e);
            cw_computer::NetFailure {
                code: e.code,
                message: e.message,
            }
        })
    }
    fn probe_tcp(
        &mut self,
        host: &str,
        port: u16,
    ) -> std::result::Result<String, cw_computer::NetFailure> {
        let node = self.runtime.node_of(&self.machine);
        let tick = self.runtime.tick();
        let start = self.runtime.state.network.traces().len();
        let r = Arc::make_mut(&mut Arc::make_mut(&mut self.runtime.state).network)
            .probe_tcp(&node, host, port, tick);
        self.runtime
            .capture_network_events(start, Some(&self.actor));
        r.map(|(address, _)| address).map_err(|e| {
            let e = SimError::from(e);
            cw_computer::NetFailure {
                code: e.code,
                message: e.message,
            }
        })
    }
}
