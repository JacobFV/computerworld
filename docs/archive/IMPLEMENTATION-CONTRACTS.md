# Implementation coordination (approved phase 3)

All agents share files. Own your assigned crates; root alone owns root Cargo.toml,
facade crate `crates/computerworld`, README, and integration wiring. Do not revert
others' edits. Send contract changes to owner and affected agents. No host IO,
wall clock, global RNG, threads, sockets or subprocesses in pure crates.

Workspace crate names `cw-protocol`, `cw-determinism`, `cw-computer`, `cw-network`,
`cw-scene`, `cw-render`, `cw-sdk`, `cw-browser`, `cw-applications`, `cw-kernel`,
`cw-environment`, `cw-trajectory`, `cw-evaluation`, `cw-services`, `computerworld`,
`cw-wasm`, `cw-python`; directories without cw prefix. Dependencies via relative
paths; serde/serde_json/sha2/thiserror/url/proptest via workspace dependencies.
Root has wildcard members, crate owners create own Cargo manifests.

## Contract owners

Protocol + sdk: protocol agent. Determinism/kernel: kernel agent. Computer:
computer agent. Network: network agent. Scene/render: render agent. Apps/browser:
apps agent. Environment/trajectory/evaluation: environment agent. Services divided
among service agents. Bindings depend on facade below. Owners quickly publish
exact signatures in their source, alert consumers, then evolve compatibly.

## Shared direction

Protocol owns WorldDefinition and shared HttpRequest/HttpResponse/SimError,
EventRecord/ActionEnvelope/EnvironmentConfig and native Page schema. All fields
serializable, BTreeMap stable maps, u64 logical microseconds. IDs initially String
newtypes only if owner publishes early. HttpRequest includes method/url/headers
and Vec<u8> body; HttpResponse status/headers/Vec<u8> body, helpers json/page/text.
Network determines destination before invoking service. Service context includes
actor, source machine, tick, seed/instance; cannot access global world.

WorldDefinition fields schema_version:u32,id:String, profiles:Vec<OsProfile>,
computers:Vec<ComputerDefinition>, network:NetworkDefinition,
services:Vec<ServiceDefinition>, metadata:serde_json::Value.
OS profile id/name/family/home/path case/dialect config; machine id/profile/address/
user/initial_files BTreeMap<String,String>/installed apps/packages, services own
id/kind/node/domains/port/initial_state Value. Protocol owner refines ASAP.

SDK pure extensible Service trait: kind/version, initialize(initial Value, context)
-> Result<Value>, handle(state:&mut Value,context:&ServiceContext,request:&HttpRequest)
-> Result<HttpResponse>. No serde encode/decode on every request; Value is mutable
in-memory service state. Kernel shares each instance root with Arc COW; arbitrary
service kinds registered externally. Page versioned title/elements are response data
not live service views. Form/link event schema has method/url/fields. App trait
similarly state/event/page + declared effects. This deliberately keeps extension
contract small; typed built-in records/accessors may sit within it.

Computer and network agents coordinate directly with kernel on hot-path APIs.
Kernel is generic Runtime: new(definition,seed,registry), execute(machine,actor,cmd),
http(machine,actor,request), advance(ticks), snapshot/restore/fork, events/state_hash,
read-only inspectors. Resumable network request queue APIs are necessary in addition
to convenience synchronous http. Registry code never serialized, instance state is.

Environment agent owns ActorSession/EnvironmentConfig validation, action families,
observations/browser/application state and full snapshot aggregate above kernel.
Connect browser with kernel HTTP, never directly with service state. Need owner
inspector vs actor interface. Coordinate with apps and kernel directly.

## Facade signature for bindings (root owns)

pub struct World; pub struct Snapshot;
World::new(definition: WorldDefinition, seed:u64)->Result<Self> (standard registry)
World::from_json(definition:&str, seed:u64)->Result<Self>
World::environment(&mut self, config:EnvironmentConfig)->Result<String> // actor session id
World::step(&mut self, session:&str, actions:Vec<ActionEnvelope>)->Result<StepResult>
World::observe(&self, session:&str)->Result<Observation>
World::render(&mut self, session:&str,width:u32,height:u32)->Result<Frame>
World::snapshot(&self)->Snapshot; restore(&mut self,&Snapshot)->Result<()>;
World::fork(&self,&Snapshot)->Result<World>; reset(&mut self,seed:u64)->Result<()>;
World::export_snapshot(&self)->Result<String>; import_snapshot(&mut self,json:&str)->Result<()>;
World::trajectory(&self)->Vec<EventRecord>; state_hash(&self)->Result<String>;
World::definition(&self)->&WorldDefinition; World::inspect()->serde_json::Value (owner).
Frame {width:u32,height:u32,rgba:Vec<u8>} from render, may have extra fields.
Snapshot clone cheap, full export expensive. World must never hardcode reference world.
Bindings parse/convert objects only; owner World and restricted Env must be distinct.

Reference world is JSON at worlds/company-2026/world.json. Facade may offer
`reference_world()->WorldDefinition` explicit helper; not kernel default.

## First integration gate

Two computers, source DNS→route→service HTTP, mutation visible after second fetch,
browser native page→scene→hit test, actor action/observation, event trajectory,
complete deterministic snapshot/replay. Then richer behavior in parallel.
