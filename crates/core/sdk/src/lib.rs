//! Pure service and application extension points; executable registry code is never serialized.
use cw_protocol::{HttpRequest, HttpResponse, Page, Result, ServiceDefinition, SimError};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{collections::BTreeMap, sync::Arc};
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServiceContext {
    pub actor: String,
    pub source: String,
    pub tick: u64,
    pub seed: u64,
    pub instance: String,
}
/// Implementations may mutate only the supplied instance state. Native plugins are trusted code.
pub trait Service: Send + Sync {
    fn kind(&self) -> &str;
    fn version(&self) -> u32 {
        1
    }
    fn initialize(&self, initial: Value, _context: &ServiceContext) -> Result<Value> {
        Ok(initial)
    }
    /// What the kernel calls at boot, with every service the world declares, itself included.
    /// A service whose default state depends on its neighbours — a search engine indexing the
    /// sites beside it — reads them here; every other service is [`Service::initialize`].
    fn initialize_in(
        &self,
        initial: Value,
        context: &ServiceContext,
        _world: &[ServiceDefinition],
    ) -> Result<Value> {
        self.initialize(initial, context)
    }
    fn handle(
        &self,
        state: &mut Value,
        context: &ServiceContext,
        request: &HttpRequest,
    ) -> Result<HttpResponse>;
    /// Kernel schedules these effects after committing the response transition.
    fn handle_with_effects(
        &self,
        state: &mut Value,
        context: &ServiceContext,
        request: &HttpRequest,
    ) -> Result<ServiceTransition> {
        Ok(ServiceTransition {
            response: self.handle(state, context, request)?,
            effects: vec![],
        })
    }
    fn on_effect(
        &self,
        _state: &mut Value,
        _context: &ServiceContext,
        _token: &str,
        _result: &ServiceEffectResult,
    ) -> Result<Vec<ServiceEffect>> {
        Ok(vec![])
    }
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ServiceTransition {
    pub response: HttpResponse,
    pub effects: Vec<ServiceEffect>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ServiceEffect {
    Http {
        request: HttpRequest,
        reply_token: String,
    },
    Schedule {
        delay_us: u64,
        token: String,
        data: Value,
    },
    Emit {
        name: String,
        data: Value,
    },
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ServiceEffectResult {
    Http { result: Result<HttpResponse> },
    Timer { data: Value },
}
#[derive(Clone, Default)]
pub struct Registry {
    services: BTreeMap<String, Arc<dyn Service>>,
    applications: BTreeMap<String, Arc<dyn Application>>,
    web_applications: BTreeMap<String, Arc<WebApplication>>,
}
impl std::fmt::Debug for Registry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Registry")
            .field("services", &self.services.keys())
            .field("applications", &self.applications.keys())
            .field("web_applications", &self.web_applications.keys())
            .finish()
    }
}
impl Registry {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn register<S: Service + 'static>(&mut self, service: S) -> Result<()> {
        self.register_service(Arc::new(service))
    }
    pub fn register_service(&mut self, service: Arc<dyn Service>) -> Result<()> {
        let kind = service.kind().to_owned();
        if kind.is_empty() || self.services.contains_key(&kind) {
            return Err(SimError::invalid(format!(
                "empty or duplicate service kind {kind}"
            )));
        }
        self.services.insert(kind, service);
        Ok(())
    }
    pub fn service(&self, kind: &str) -> Result<&Arc<dyn Service>> {
        self.services
            .get(kind)
            .ok_or_else(|| SimError::new("unknown_service", format!("unregistered service {kind}")))
    }
    pub fn service_kinds(&self) -> impl Iterator<Item = &str> {
        self.services.keys().map(String::as_str)
    }
    pub fn register_application<A: Application + 'static>(&mut self, app: A) -> Result<()> {
        let kind = app.kind().to_owned();
        if kind.is_empty()
            || self.applications.contains_key(&kind)
            || self.web_applications.contains_key(&kind)
        {
            return Err(SimError::invalid(format!(
                "empty or duplicate application kind {kind}"
            )));
        }
        self.applications.insert(kind, Arc::new(app));
        Ok(())
    }
    pub fn application(&self, kind: &str) -> Result<&Arc<dyn Application>> {
        self.applications.get(kind).ok_or_else(|| {
            SimError::new(
                "unknown_application",
                format!("unregistered application {kind}"),
            )
        })
    }
    /// Register an application written as a web app: it opens in a desktop window
    /// whose content is its document. See [`WebApplication`].
    pub fn register_web_application(&mut self, app: WebApplication) -> Result<()> {
        let kind = app.kind.clone();
        if kind.is_empty()
            || self.applications.contains_key(&kind)
            || self.web_applications.contains_key(&kind)
        {
            return Err(SimError::invalid(format!(
                "empty or duplicate application kind {kind}"
            )));
        }
        self.web_applications.insert(kind, Arc::new(app));
        Ok(())
    }
    pub fn web_application(&self, kind: &str) -> Result<&Arc<WebApplication>> {
        self.web_applications.get(kind).ok_or_else(|| {
            SimError::new(
                "unknown_application",
                format!("unregistered web application {kind}"),
            )
        })
    }
    pub fn module_versions(&self) -> BTreeMap<String, u32> {
        self.services
            .iter()
            .map(|(k, v)| (format!("service:{k}"), v.version()))
            .chain(
                self.applications
                    .iter()
                    .map(|(k, v)| (format!("app:{k}"), v.version())),
            )
            .chain(
                self.web_applications
                    .iter()
                    .map(|(k, v)| (format!("web:{k}"), v.version)),
            )
            .collect()
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppContext {
    pub actor: String,
    pub machine: String,
    pub tick: u64,
    pub seed: u64,
    pub instance: String,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AppEvent {
    pub kind: String,
    #[serde(default)]
    pub target: Option<String>,
    #[serde(default)]
    pub data: Value,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AppEffect {
    Http { request: HttpRequest },
    ReadFile { path: String },
    WriteFile { path: String, bytes: Vec<u8> },
    Launch { application: String },
    Emit { name: String, data: Value },
}
/// An application written as a web app. Its window's frame is the platform's own;
/// its content is a document the host lays out and paints with the web engine, driven
/// by the application's script, which reaches the machine only through the `cw`
/// global (files, services, launching, named data, declared state) and so through
/// the same mediation as any native application's effects.
///
/// Across a snapshot the application is its declared state (`cw.state.set`): a
/// restored window boots the same code with that state, so the document must be a
/// function of it. The code itself is never serialised; `kind` and `version` name it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WebApplication {
    pub kind: String,
    #[serde(default = "first_version")]
    pub version: u32,
    /// Window title by platform (`macos`, `windows`, `ubuntu`, `ios`, `android`);
    /// `*` is every other platform. Empty uses the kind.
    #[serde(default)]
    pub titles: BTreeMap<String, String>,
    pub source: WebSource,
}
fn first_version() -> u32 {
    1
}
/// What a web application is made of.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "format", rename_all = "snake_case")]
pub enum WebSource {
    /// JavaScript that renders into `#root`, typically compiled from TSX ahead of
    /// time, with its stylesheet. With `react`, React 18's production build is loaded
    /// first and the script finds it as the globals `React` and `ReactDOM`.
    Script {
        script: String,
        #[serde(default)]
        style: String,
        #[serde(default)]
        react: bool,
    },
}
pub trait Application: Send + Sync {
    fn kind(&self) -> &str;
    fn version(&self) -> u32 {
        1
    }
    fn initialize(&self, initial: Value, _context: &AppContext) -> Result<Value> {
        Ok(initial)
    }
    fn event(
        &self,
        state: &mut Value,
        context: &AppContext,
        event: &AppEvent,
    ) -> Result<Vec<AppEffect>>;
    fn page(&self, state: &Value, context: &AppContext) -> Result<Page>;
}
#[cfg(test)]
mod tests {
    use super::*;
    struct Echo;
    impl Service for Echo {
        fn kind(&self) -> &str {
            "echo"
        }
        fn handle(
            &self,
            _: &mut Value,
            _: &ServiceContext,
            r: &HttpRequest,
        ) -> Result<HttpResponse> {
            Ok(HttpResponse::text(200, r.url.clone()))
        }
    }
    #[test]
    fn duplicate_rejected_without_replacement() {
        let mut r = Registry::new();
        r.register(Echo).unwrap();
        assert!(r.register(Echo).is_err());
        assert!(r.service("echo").is_ok());
        assert!(r.service("other").is_err())
    }
}
