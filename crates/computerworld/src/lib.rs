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
#[cfg(feature = "render")]
use std::collections::BTreeMap;
use std::sync::Arc;

/// Rasterizes a scene for the Screenshot effect. A fresh renderer per capture keeps the
/// incremental cache the interactive path relies on untouched.
#[cfg(feature = "render")]
struct PngCapture;
#[cfg(feature = "render")]
impl cw_environment::Raster for PngCapture {
    fn png(&self, scene: &Scene) -> std::result::Result<Vec<u8>, String> {
        let mut renderer = cw_render::Renderer::new();
        let frame = renderer.try_render(scene).map_err(|e| e.to_string())?;
        let mut out = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut out, frame.width, frame.height);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            encoder
                .write_header()
                .and_then(|mut w| w.write_image_data(&frame.rgba))
                .map_err(|e| e.to_string())?;
        }
        Ok(out)
    }
    fn encode(&self, width: u32, height: u32, rgba: &[u8]) -> std::result::Result<Vec<u8>, String> {
        if rgba.len() != width as usize * height as usize * 4 || width == 0 || height == 0 {
            return Err("pixel buffer does not match the image size".into());
        }
        let mut out = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut out, width, height);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            encoder
                .write_header()
                .and_then(|mut w| w.write_image_data(rgba))
                .map_err(|e| e.to_string())?;
        }
        Ok(out)
    }
    fn pixels(&self, scene: &Scene) -> std::result::Result<(u32, u32, Vec<u8>), String> {
        let frame = cw_render::Renderer::new()
            .try_render(scene)
            .map_err(|e| e.to_string())?;
        Ok((frame.width, frame.height, frame.rgba))
    }
    fn decode(&self, bytes: &[u8]) -> std::result::Result<(u32, u32, Vec<u8>), String> {
        // JPEG files start with an SOI marker; everything else is tried as PNG.
        if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
            return decode_jpeg(bytes);
        }
        let decoder = png::Decoder::new(bytes);
        let mut reader = decoder.read_info().map_err(|e| e.to_string())?;
        // Bound the allocation: a malformed header must not ask for gigabytes.
        let size = reader.output_buffer_size();
        if size > 64 << 20 {
            return Err("image is too large to decode".into());
        }
        let mut buffer = vec![0; size];
        let info = reader.next_frame(&mut buffer).map_err(|e| e.to_string())?;
        buffer.truncate(info.buffer_size());
        // The scene draws RGBA; widen anything narrower rather than refusing it.
        let rgba = match info.color_type {
            png::ColorType::Rgba => buffer,
            png::ColorType::Rgb => buffer
                .as_chunks::<3>()
                .0
                .iter()
                .flat_map(|p| [p[0], p[1], p[2], 255])
                .collect(),
            png::ColorType::Grayscale => buffer.iter().flat_map(|g| [*g, *g, *g, 255]).collect(),
            png::ColorType::GrayscaleAlpha => buffer
                .as_chunks::<2>()
                .0
                .iter()
                .flat_map(|p| [p[0], p[0], p[0], p[1]])
                .collect(),
            other => return Err(format!("unsupported image format {other:?}")),
        };
        Ok((info.width, info.height, rgba))
    }
}
/// Baseline and progressive JPEG to RGBA, with the integer-only decoder build.
#[cfg(feature = "render")]
fn decode_jpeg(bytes: &[u8]) -> std::result::Result<(u32, u32, Vec<u8>), String> {
    let mut decoder = jpeg_decoder::Decoder::new(bytes);
    decoder.read_info().map_err(|e| e.to_string())?;
    let info = decoder.info().ok_or("JPEG has no header")?;
    // Bound the allocation before decoding, as for PNG.
    if u64::from(info.width) * u64::from(info.height) * 4 > 64 << 20 {
        return Err("image is too large to decode".into());
    }
    let data = decoder.decode().map_err(|e| e.to_string())?;
    let rgba: Vec<u8> = match info.pixel_format {
        jpeg_decoder::PixelFormat::RGB24 => data
            .as_chunks::<3>()
            .0
            .iter()
            .flat_map(|p| [p[0], p[1], p[2], 255])
            .collect(),
        jpeg_decoder::PixelFormat::L8 => data.iter().flat_map(|g| [*g, *g, *g, 255]).collect(),
        other => return Err(format!("unsupported JPEG pixel format {other:?}")),
    };
    Ok((u32::from(info.width), u32::from(info.height), rgba))
}
/// Persistent owner of simulation and interface state; renderer caches are disposable.
pub struct World {
    environment: Environment,
    #[cfg(feature = "render")]
    renderer: Option<cw_render::Renderer>,
    /// One per session: every machine on screen keeps the frame it was last left at, so a
    /// world showing seven of them repaints each incrementally instead of diffing one
    /// machine's screen against another's. The caches stay in the single renderer above.
    #[cfg(feature = "render")]
    surfaces: BTreeMap<String, cw_render::Surface>,
    #[cfg(feature = "render")]
    previous_scene: BTreeMap<String, Scene>,
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
        #[allow(unused_mut)]
        let mut environment = environment;
        // A build with the rasterizer can take a real screenshot; one without refuses
        // rather than writing a file that is not a picture of the screen.
        #[cfg(feature = "render")]
        environment.register_raster(std::sync::Arc::new(PngCapture));
        Self {
            environment,
            #[cfg(feature = "render")]
            renderer: None,
            #[cfg(feature = "render")]
            surfaces: BTreeMap::new(),
            #[cfg(feature = "render")]
            previous_scene: BTreeMap::new(),
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
        self.render_with(session, width, height, Frame::clone)
    }
    /// The same frame, handed to `take` where it lies in the renderer rather than copied
    /// out of it first. A caller that is going to copy the pixels somewhere of its own —
    /// a browser's `ImageData`, say — copies them once this way instead of two or three
    /// times, which at four megabytes a screen is most of what a repaint costs.
    #[cfg(feature = "render")]
    pub fn render_with<T>(
        &mut self,
        session: &str,
        width: u32,
        height: u32,
        take: impl FnOnce(&Frame) -> T,
    ) -> Result<T> {
        let next = self.scene(session, width, height)?;
        next.validate()
            .map_err(|e| SimError::new("render", e.to_string()))?;
        // Against this session's own last scene, never another session's: the damage is
        // what spares the renderer the screen it has already drawn.
        let damage = self
            .previous_scene
            .get(session)
            .map(|previous| scene_damage(previous, &next));
        self.previous_scene.insert(session.to_owned(), next);
        let scene = &self.previous_scene[session];
        let renderer = self.renderer.get_or_insert_with(cw_render::Renderer::new);
        let mut surface = self.surfaces.remove(session).unwrap_or_default();
        renderer.swap(&mut surface);
        let taken = take(match &damage {
            Some(damage) => renderer.render_incremental(scene, damage),
            None => renderer.render_ref(scene),
        });
        renderer.swap(&mut surface);
        self.surfaces.insert(session.to_owned(), surface);
        Ok(taken)
    }
    /// What the renderer has drawn since the world was built. `painted_pixels` is the one
    /// that says whether a repaint was incremental: a screen redrawn only because another
    /// screen in the same world was touched should cost a few rectangles, not a megapixel.
    #[cfg(feature = "render")]
    pub fn render_stats(&self) -> Option<&cw_render::RenderStats> {
        self.renderer.as_ref().map(|renderer| &renderer.stats)
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
            self.surfaces.clear();
            self.previous_scene.clear();
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
///
/// Parsed once and cloned: the world now carries a whole simulated internet, and benchmarks and
/// tests call this per iteration, so re-parsing the embedded JSON every time is pure waste.
pub fn reference_world() -> WorldDefinition {
    static PARSED: std::sync::OnceLock<WorldDefinition> = std::sync::OnceLock::new();
    PARSED
        .get_or_init(|| {
            serde_json::from_str(include_str!("../../../worlds/company-2026/world.json"))
                .expect("packaged reference world is valid JSON")
        })
        .clone()
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
