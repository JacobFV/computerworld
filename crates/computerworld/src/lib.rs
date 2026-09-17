//! Canonical Rust synthetic computer worlds.
//!
//! [`World`] is the trusted owner. Grant an agent a session rather than passing
//! owner inspection or portable snapshots. All language bindings use this facade.
use cw_environment::Environment;
pub use cw_environment::Snapshot;
use cw_kernel::Runtime;
pub use cw_protocol::*;
#[cfg(feature = "render")]
pub use cw_render::Frame;
pub use cw_scene::Scene;
pub use cw_sdk::{Application, Registry, Service};
use std::sync::Arc;

/// Persistent owner of simulation and interface state; renderer caches are disposable.
pub struct World {
    environment: Environment,
    #[cfg(feature = "render")]
    renderer: Option<cw_render::Renderer>,
    #[cfg(feature = "render")]
    previous_scene: Option<Scene>,
}
impl World {
    /// Compose the optional standard services. The world definition supplies all instances.
    pub fn new(definition: WorldDefinition, seed: u64) -> Result<Self> {
        Self::with_registry(definition, seed, cw_services::registry()?)
    }
    /// Construct an arbitrary world with owner-selected implementations.
    pub fn with_registry(
        definition: WorldDefinition,
        seed: u64,
        registry: Registry,
    ) -> Result<Self> {
        Ok(Self::from_environment(Environment::with_registry(
            Runtime::new(definition, seed, registry.clone())?,
            registry,
        )))
    }
    fn from_environment(environment: Environment) -> Self {
        Self {
            environment,
            #[cfg(feature = "render")]
            renderer: None,
            #[cfg(feature = "render")]
            previous_scene: None,
        }
    }
    pub fn from_json(definition: &str, seed: u64) -> Result<Self> {
        Self::new(WorldDefinition::from_json(definition)?, seed)
    }
    /// Add a synthetic computer with explicit identity and links. This grants no actor access.
    pub fn add_computer(
        &mut self,
        computer: ComputerDefinition,
        node: NetworkNode,
        links: Vec<NetworkLink>,
    ) -> Result<()> {
        self.environment.add_computer(computer, node, links)?;
        self.clear_renderer();
        Ok(())
    }
    /// Remove a device and revoke its actor grants. Hosted services must be migrated first.
    pub fn remove_computer(&mut self, id: &str) -> Result<()> {
        self.environment.remove_computer(id)?;
        self.clear_renderer();
        Ok(())
    }
    pub fn environment(&mut self, config: EnvironmentConfig) -> Result<String> {
        self.environment.environment(config)
    }
    /// Borrow a restricted native agent interface. The owner retains reset/inspection authority.
    pub fn actor(&mut self, session: &str) -> Result<ActorEnvironment<'_>> {
        self.validate_session(session)?;
        Ok(ActorEnvironment {
            world: self,
            session: session.to_owned(),
        })
    }
    pub fn validate_session(&self, session: &str) -> Result<()> {
        self.environment.session(session).map(|_| ())
    }
    pub fn register_observation_channel(
        &mut self,
        channel: Arc<dyn cw_environment::ObservationChannel>,
    ) -> Result<()> {
        self.environment.register_observation_channel(channel)
    }
    pub fn register_application<A: cw_sdk::Application + 'static>(&mut self, app: A) -> Result<()> {
        self.environment.register_application(app)
    }
    pub fn register_action_family(
        &mut self,
        family: Arc<dyn cw_environment::ActionFamily>,
    ) -> Result<()> {
        self.environment.register_action_family(family)
    }
    pub fn step(&mut self, session: &str, actions: Vec<ActionEnvelope>) -> Result<StepResult> {
        self.environment.step(session, actions)
    }
    pub fn observe(&self, session: &str) -> Result<Observation> {
        self.environment.observe(session)
    }
    pub fn scene(&self, session: &str, width: u32, height: u32) -> Result<Scene> {
        validate_viewport(width, height)?;
        self.environment.scene(session, width, height)
    }
    #[cfg(feature = "render")]
    pub fn render(&mut self, session: &str, width: u32, height: u32) -> Result<Frame> {
        let scene = self.scene(session, width, height)?;
        scene
            .validate()
            .map_err(|e| SimError::new("render", e.to_string()))?;
        let renderer = self.renderer.get_or_insert_with(cw_render::Renderer::new);
        let frame = if let Some(previous) = &self.previous_scene {
            renderer
                .render_incremental(&scene, &scene_damage(previous, &scene))
                .clone()
        } else {
            renderer
                .try_render(&scene)
                .map_err(|e| SimError::new("render", e.to_string()))?
        };
        self.previous_scene = Some(scene);
        Ok(frame)
    }
    pub fn snapshot(&self) -> Snapshot {
        self.environment.snapshot()
    }
    pub fn restore(&mut self, snapshot: &Snapshot) -> Result<()> {
        self.environment.restore(snapshot)?;
        self.clear_renderer();
        Ok(())
    }
    pub fn fork(&self, snapshot: &Snapshot) -> Result<Self> {
        Ok(Self::from_environment(self.environment.fork(snapshot)?))
    }
    pub fn reset(&mut self, seed: u64) -> Result<()> {
        self.environment.reset(seed)?;
        self.clear_renderer();
        Ok(())
    }
    pub fn export_snapshot(&self) -> Result<String> {
        self.environment.runtime.try_snapshot()?;
        self.environment.export_snapshot()
    }
    pub fn import_snapshot(&mut self, json: &str) -> Result<()> {
        self.environment.import_snapshot(json)?;
        self.clear_renderer();
        Ok(())
    }
    pub fn trajectory(&self) -> Vec<EventRecord> {
        self.environment.trajectory()
    }
    pub fn state_hash(&self) -> Result<String> {
        self.environment.state_hash()
    }
    pub fn definition(&self) -> &WorldDefinition {
        self.environment.runtime.definition()
    }
    /// Privileged owner view; never return this from an actor endpoint.
    pub fn inspect(&self) -> serde_json::Value {
        self.environment.inspect()
    }
    /// Privileged access for custom integrations and explicitly enabled host adapters.
    pub fn runtime(&self) -> &Runtime {
        &self.environment.runtime
    }
    pub fn runtime_mut(&mut self) -> &mut Runtime {
        &mut self.environment.runtime
    }
    pub fn interfaces(&self) -> &Environment {
        &self.environment
    }
    pub fn interfaces_mut(&mut self) -> &mut Environment {
        &mut self.environment
    }
    fn clear_renderer(&mut self) {
        #[cfg(feature = "render")]
        {
            self.renderer = None;
            self.previous_scene = None;
        }
    }
}
fn validate_viewport(width: u32, height: u32) -> Result<()> {
    if width == 0 || height == 0 || u64::from(width) * u64::from(height) > 16_777_216 {
        return Err(SimError::invalid(
            "viewport must contain 1..16777216 pixels",
        ));
    }
    Ok(())
}
/// Explicit optional reference blueprint; never an implicit kernel default.
pub fn reference_world() -> WorldDefinition {
    serde_json::from_str(include_str!("../../../worlds/company-2026/world.json"))
        .expect("packaged reference world is valid JSON")
}

#[cfg(feature = "render")]
fn scene_damage(old: &Scene, new: &Scene) -> cw_scene::Damage {
    use cw_scene::{Damage, Rect};
    if old.width != new.width
        || old.height != new.height
        || old.background != new.background
        || !old
            .nodes
            .iter()
            .map(|n| n.id)
            .eq(new.nodes.iter().map(|n| n.id))
    {
        return Damage {
            rects: vec![Rect::new(0, 0, new.width, new.height)],
        };
    }
    let mut rects = Vec::new();
    for (before, after) in old.nodes.iter().zip(&new.nodes) {
        if before != after {
            rects.push(before.painted_bounds());
            rects.push(after.painted_bounds());
        }
    }
    Damage { rects }
}

/// Restricted native interface. No privileged topology, snapshots, evaluator or runtime methods.
pub struct ActorEnvironment<'a> {
    world: &'a mut World,
    session: String,
}
impl ActorEnvironment<'_> {
    pub fn id(&self) -> &str {
        &self.session
    }
    pub fn observe(&self) -> Result<Observation> {
        self.world.observe(&self.session)
    }
    pub fn step(&mut self, actions: Vec<ActionEnvelope>) -> Result<StepResult> {
        self.world.step(&self.session, actions)
    }
    pub fn scene(&self, width: u32, height: u32) -> Result<Scene> {
        self.world.scene(&self.session, width, height)
    }
    #[cfg(feature = "render")]
    pub fn render(&mut self, width: u32, height: u32) -> Result<Frame> {
        self.world.render(&self.session, width, height)
    }
}
