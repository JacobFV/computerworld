//! Versioned, serializable contracts. This crate performs no host I/O.
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
pub const SCHEMA_VERSION: u32 = 1;
pub const PAGE_MEDIA_TYPE: &str = "application/vnd.computerworld.page+json";
pub type Result<T> = std::result::Result<T, SimError>;
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, thiserror::Error)]
#[error("{code}: {message}")]
pub struct SimError {
    pub code: String,
    pub message: String,
}
impl SimError {
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }
    pub fn invalid(message: impl Into<String>) -> Self {
        Self::new("invalid", message)
    }
    pub fn denied(message: impl Into<String>) -> Self {
        Self::new("denied", message)
    }
    pub fn not_found(message: impl Into<String>) -> Self {
        Self::new("not_found", message)
    }
}
impl From<serde_json::Error> for SimError {
    fn from(e: serde_json::Error) -> Self {
        Self::new("serialization", e.to_string())
    }
}
fn yes() -> bool {
    true
}
fn port() -> u16 {
    80
}
fn version() -> u32 {
    SCHEMA_VERSION
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorldDefinition {
    #[serde(default = "version")]
    pub schema_version: u32,
    pub id: String,
    #[serde(default)]
    pub profiles: Vec<OsProfile>,
    #[serde(default)]
    pub computers: Vec<ComputerDefinition>,
    #[serde(default)]
    pub network: NetworkDefinition,
    #[serde(default)]
    pub services: Vec<ServiceDefinition>,
    #[serde(default)]
    pub metadata: Value,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OsProfile {
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub family: String,
    #[serde(default)]
    pub home: String,
    #[serde(default = "yes")]
    pub case_sensitive: bool,
    #[serde(default)]
    pub shell: String,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ComputerDefinition {
    pub id: String,
    pub profile: String,
    pub address: String,
    pub user: String,
    #[serde(default)]
    pub node: String,
    #[serde(default)]
    pub initial_files: BTreeMap<String, String>,
    #[serde(default)]
    pub installed_apps: Vec<String>,
    #[serde(default)]
    pub packages: Vec<String>,
}
impl ComputerDefinition {
    pub fn node_id(&self) -> &str {
        if self.node.is_empty() {
            &self.id
        } else {
            &self.node
        }
    }
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ServiceDefinition {
    pub id: String,
    pub kind: String,
    pub node: String,
    #[serde(default)]
    pub domains: Vec<String>,
    #[serde(default = "port")]
    pub port: u16,
    #[serde(default)]
    pub initial_state: Value,
}
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct NetworkDefinition {
    #[serde(default)]
    pub implicit_lan: bool,
    #[serde(default)]
    pub nodes: Vec<NetworkNode>,
    #[serde(default)]
    pub links: Vec<NetworkLink>,
    #[serde(default)]
    pub dns: Vec<DnsRecord>,
    #[serde(default)]
    pub routes: Vec<Route>,
    #[serde(default)]
    pub gateway: GatewayPolicy,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NetworkNode {
    pub id: String,
    pub address: String,
    #[serde(default)]
    pub zone: NetworkZone,
}
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NetworkZone {
    #[default]
    Local,
    Internet,
    Host,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NetworkLink {
    pub from: String,
    pub to: String,
    #[serde(default = "yes")]
    pub bidirectional: bool,
    #[serde(default)]
    pub latency_us: u64,
    #[serde(default)]
    pub loss_per_million: u32,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DnsRecord {
    pub name: String,
    pub address: String,
    #[serde(default)]
    pub ttl_us: u64,
    #[serde(default)]
    pub resolver: Option<String>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Route {
    pub from: String,
    pub to: String,
    #[serde(default)]
    pub via: Option<String>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GatewayPolicy {
    #[serde(default)]
    pub sources: Vec<String>,
    #[serde(default)]
    pub ports: Vec<u16>,
    #[serde(default)]
    pub schemes: Vec<String>,
    #[serde(default)]
    pub allowed_cidrs: Vec<String>,
    #[serde(default)]
    pub denied_cidrs: Vec<String>,
    #[serde(default = "default_max_response_bytes")]
    pub max_response_bytes: usize,
    #[serde(default = "default_timeout_us")]
    pub timeout_us: u64,
    #[serde(default = "yes")]
    pub allow_local: bool,
    #[serde(default = "yes")]
    pub allow_internet: bool,
    #[serde(default)]
    pub allow_host: bool,
    #[serde(default)]
    pub host_allowlist: Vec<String>,
    #[serde(default)]
    pub denied_pairs: Vec<(String, String)>,
}
fn default_max_response_bytes() -> usize {
    1_048_576
}
fn default_timeout_us() -> u64 {
    30_000_000
}
impl Default for GatewayPolicy {
    fn default() -> Self {
        Self {
            sources: vec![],
            ports: vec![],
            schemes: vec![],
            allowed_cidrs: vec![],
            denied_cidrs: vec![],
            max_response_bytes: default_max_response_bytes(),
            timeout_us: default_timeout_us(),
            allow_local: true,
            allow_internet: true,
            allow_host: false,
            host_allowlist: vec![],
            denied_pairs: vec![],
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HttpRequest {
    pub method: String,
    pub url: String,
    #[serde(default)]
    pub headers: BTreeMap<String, String>,
    #[serde(default)]
    pub body: Vec<u8>,
}
impl HttpRequest {
    pub fn get(url: impl Into<String>) -> Self {
        Self {
            method: "GET".into(),
            url: url.into(),
            headers: BTreeMap::new(),
            body: vec![],
        }
    }
    pub fn json(
        method: impl Into<String>,
        url: impl Into<String>,
        value: &impl Serialize,
    ) -> Result<Self> {
        Ok(Self {
            method: method.into(),
            url: url.into(),
            headers: BTreeMap::from([("content-type".into(), "application/json".into())]),
            body: serde_json::to_vec(value)?,
        })
    }
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HttpResponse {
    pub status: u16,
    #[serde(default)]
    pub headers: BTreeMap<String, String>,
    #[serde(default)]
    pub body: Vec<u8>,
}
impl HttpResponse {
    pub fn text(status: u16, text: impl Into<String>) -> Self {
        Self {
            status,
            headers: BTreeMap::from([("content-type".into(), "text/plain; charset=utf-8".into())]),
            body: text.into().into_bytes(),
        }
    }
    pub fn json(status: u16, value: &impl Serialize) -> Result<Self> {
        Ok(Self {
            status,
            headers: BTreeMap::from([("content-type".into(), "application/json".into())]),
            body: serde_json::to_vec(value)?,
        })
    }
    pub fn page(page: &Page) -> Result<Self> {
        Ok(Self {
            status: 200,
            headers: BTreeMap::from([("content-type".into(), PAGE_MEDIA_TYPE.into())]),
            body: serde_json::to_vec(page)?,
        })
    }
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Page {
    #[serde(default = "version")]
    pub version: u32,
    pub title: String,
    #[serde(default)]
    pub elements: Vec<PageElement>,
}
impl PageElement {
    pub fn id(&self) -> &str {
        match self {
            Self::Heading { id, .. }
            | Self::Text { id, .. }
            | Self::Link { id, .. }
            | Self::Button { id, .. }
            | Self::Input { id, .. }
            | Self::Form { id, .. }
            | Self::Group { id, .. }
            | Self::Image { id, .. } => id,
        }
    }
}
impl Page {
    /// Reject ambiguous interaction targets and unsupported native-page contracts.
    pub fn validate(&self) -> Result<()> {
        if self.version != SCHEMA_VERSION {
            return Err(SimError::invalid("unsupported page version"));
        }
        fn visit(elements: &[PageElement], ids: &mut BTreeSet<String>, depth: usize) -> Result<()> {
            if depth > 64 {
                return Err(SimError::invalid("page nesting exceeds 64 levels"));
            }
            for element in elements {
                if element.id().is_empty() || !ids.insert(element.id().to_owned()) {
                    return Err(SimError::invalid("empty or duplicate page element id"));
                }
                if ids.len() > 100_000 {
                    return Err(SimError::invalid("page exceeds element budget"));
                }
                match element {
                    PageElement::Heading { level, .. } if !(1..=6).contains(level) => {
                        return Err(SimError::invalid("heading level must be 1 through 6"))
                    }
                    PageElement::Group { children, .. } | PageElement::Form { children, .. } => {
                        visit(children, ids, depth + 1)?
                    }
                    _ => (),
                }
            }
            Ok(())
        }
        visit(&self.elements, &mut BTreeSet::new(), 0)
    }
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            version: SCHEMA_VERSION,
            title: title.into(),
            elements: vec![],
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PageElement {
    Heading {
        id: String,
        text: String,
        level: u8,
    },
    Text {
        id: String,
        text: String,
    },
    Link {
        id: String,
        text: String,
        url: String,
    },
    Button {
        id: String,
        text: String,
        action: PageAction,
    },
    Input {
        id: String,
        label: String,
        value: String,
        #[serde(default)]
        placeholder: String,
    },
    Form {
        id: String,
        action: PageAction,
        children: Vec<PageElement>,
    },
    Group {
        id: String,
        children: Vec<PageElement>,
    },
    Image {
        id: String,
        source: String,
        alt: String,
        width: u32,
        height: u32,
    },
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PageAction {
    pub method: String,
    pub url: String,
    #[serde(default)]
    pub fields: BTreeMap<String, String>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ActionEnvelope {
    pub family: String,
    pub op: String,
    pub machine: String,
    #[serde(default)]
    pub payload: Value,
}
impl ActionEnvelope {
    pub fn new(
        family: impl Into<String>,
        op: impl Into<String>,
        machine: impl Into<String>,
        payload: Value,
    ) -> Self {
        Self {
            family: family.into(),
            op: op.into(),
            machine: machine.into(),
            payload,
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnvironmentConfig {
    pub actor: String,
    pub machines: Vec<String>,
    pub actions: Vec<String>,
    pub observations: Vec<String>,
    #[serde(default = "default_budget")]
    pub action_budget: u32,
}
fn default_budget() -> u32 {
    1024
}
impl EnvironmentConfig {
    pub fn terminal(actor: impl Into<String>, machine: impl Into<String>) -> Self {
        Self {
            actor: actor.into(),
            machines: vec![machine.into()],
            actions: vec!["terminal.v1".into()],
            observations: vec!["terminal.v1".into()],
            action_budget: default_budget(),
        }
    }
    pub fn desktop(actor: impl Into<String>, machine: impl Into<String>) -> Self {
        Self {
            actor: actor.into(),
            machines: vec![machine.into()],
            actions: vec![
                "terminal.v1",
                "filesystem.v1",
                "browser.v1",
                "pointer.v1",
                "keyboard.v1",
                "application.v1",
                "http.v1",
            ]
            .into_iter()
            .map(str::to_owned)
            .collect(),
            observations: vec!["terminal.v1".into(), "semantic.v1".into()],
            action_budget: default_budget(),
        }
    }
}
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Observation {
    pub tick: u64,
    #[serde(default)]
    pub channels: BTreeMap<String, Value>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ActionOutcome {
    pub index: usize,
    pub success: bool,
    #[serde(default)]
    pub value: Value,
    #[serde(default)]
    pub error: Option<SimError>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StepResult {
    pub observation: Observation,
    pub outcomes: Vec<ActionOutcome>,
    pub tick: u64,
    #[serde(default)]
    pub pending: usize,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EventRecord {
    pub sequence: u64,
    pub tick: u64,
    pub kind: String,
    #[serde(default)]
    pub machine: Option<String>,
    #[serde(default)]
    pub actor: Option<String>,
    #[serde(default)]
    pub data: Value,
}
impl WorldDefinition {
    pub fn from_json(json: &str) -> Result<Self> {
        let value: Self = serde_json::from_str(json)?;
        value.validate()?;
        Ok(value)
    }
    pub fn profile(&self, id: &str) -> Result<&OsProfile> {
        self.profiles
            .iter()
            .find(|p| p.id == id)
            .ok_or_else(|| SimError::not_found(format!("profile {id}")))
    }
    pub fn validate(&self) -> Result<()> {
        if self.schema_version != SCHEMA_VERSION {
            return Err(SimError::invalid("unsupported world schema version"));
        }
        if self.id.trim().is_empty() {
            return Err(SimError::invalid("world id is empty"));
        }
        fn unique<'a>(
            items: impl Iterator<Item = &'a str>,
            kind: &str,
        ) -> Result<BTreeSet<String>> {
            let mut out = BTreeSet::new();
            for id in items {
                if id.trim().is_empty() || !out.insert(id.to_owned()) {
                    return Err(SimError::invalid(format!(
                        "empty or duplicate {kind} id: {id}"
                    )));
                }
            }
            Ok(out)
        }
        let profiles = unique(self.profiles.iter().map(|x| x.id.as_str()), "profile")?;
        unique(self.computers.iter().map(|x| x.id.as_str()), "computer")?;
        unique(self.services.iter().map(|x| x.id.as_str()), "service")?;
        let mut nodes = unique(self.network.nodes.iter().map(|x| x.id.as_str()), "node")?;
        let mut addresses: BTreeMap<String, String> = BTreeMap::new();
        for n in &self.network.nodes {
            if n.address.parse::<std::net::IpAddr>().is_err() {
                return Err(SimError::invalid(format!(
                    "invalid node address {}",
                    n.address
                )));
            }
            if addresses.insert(n.address.clone(), n.id.clone()).is_some() {
                return Err(SimError::invalid("duplicate network address"));
            }
        }
        let mut computer_nodes = BTreeSet::new();
        for c in &self.computers {
            if !computer_nodes.insert(c.node_id()) {
                return Err(SimError::invalid("multiple computers own one node"));
            }
            if !profiles.contains(&c.profile) {
                return Err(SimError::invalid(format!("unknown profile {}", c.profile)));
            }
            if c.user.is_empty() {
                return Err(SimError::invalid("computer user is empty"));
            }
            if c.address.parse::<std::net::IpAddr>().is_err() {
                return Err(SimError::invalid("invalid computer address"));
            }
            if let Some(n) = addresses.get(&c.address) {
                if n != c.node_id() {
                    return Err(SimError::invalid(
                        "computer address belongs to another node",
                    ));
                }
            } else {
                addresses.insert(c.address.clone(), c.node_id().to_string());
            }
            nodes.insert(c.node_id().to_owned());
            if let Some(n) = self.network.nodes.iter().find(|n| n.id == c.node_id()) {
                if n.address != c.address {
                    return Err(SimError::invalid("computer/node address mismatch"));
                }
            }
        }
        let mut listeners = BTreeSet::new();
        let mut domains = BTreeMap::new();
        for s in &self.services {
            if !nodes.contains(&s.node) {
                return Err(SimError::invalid(format!(
                    "unknown service node {}",
                    s.node
                )));
            }
            if s.kind.is_empty() || s.port == 0 {
                return Err(SimError::invalid("invalid service kind or port"));
            }
            if !listeners.insert((&s.node, s.port)) {
                return Err(SimError::invalid("duplicate service listener"));
            }
            for d in &s.domains {
                let d = d.trim_end_matches('.').to_ascii_lowercase();
                if d.is_empty() {
                    return Err(SimError::invalid("empty service domain"));
                }
                if let Some(old) = domains.insert(d, s.node.clone()) {
                    if old != s.node {
                        return Err(SimError::invalid("domain owned by different service nodes"));
                    }
                }
            }
        }
        for l in &self.network.links {
            if !nodes.contains(&l.from) || !nodes.contains(&l.to) || l.loss_per_million > 1_000_000
            {
                return Err(SimError::invalid("invalid network link"));
            }
        }
        for r in &self.network.routes {
            if !nodes.contains(&r.from)
                || !nodes.contains(&r.to)
                || r.via.as_ref().is_some_and(|v| !nodes.contains(v))
            {
                return Err(SimError::invalid("invalid route"));
            }
        }
        let mut dns = BTreeSet::new();
        for r in &self.network.dns {
            if r.name.is_empty()
                || !dns.insert(r.name.trim_end_matches('.').to_ascii_lowercase())
                || !valid_dns_target(&r.address)
                || r.resolver.as_ref().is_some_and(|v| !nodes.contains(v))
            {
                return Err(SimError::invalid("invalid DNS record"));
            }
        }
        for (name, node) in domains {
            if let Some(record) = self
                .network
                .dns
                .iter()
                .find(|r| r.name.trim_end_matches('.').eq_ignore_ascii_case(&name))
            {
                if let Some(owner) = addresses.get(&record.address) {
                    if owner != &node {
                        return Err(SimError::invalid(
                            "service domain DNS points to a different node",
                        ));
                    }
                }
            }
        }
        for (a, b) in &self.network.gateway.denied_pairs {
            if !nodes.contains(a) || !nodes.contains(b) {
                return Err(SimError::invalid("unknown gateway policy node"));
            }
        }
        Ok(())
    }
}
/// DNS aliases use the same record representation as address records.
/// An address literal is an A/AAAA answer; a DNS name is a CNAME target.
pub fn valid_dns_target(value: &str) -> bool {
    value.parse::<std::net::IpAddr>().is_ok() || {
        let value = value.strip_suffix('.').unwrap_or(value);
        !value.is_empty()
            && value.len() <= 253
            && value.split('.').all(|part| {
                !part.is_empty()
                    && part.len() <= 63
                    && !part.starts_with('-')
                    && !part.ends_with('-')
                    && part.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn response_page_roundtrip() {
        let p = Page::new("hello");
        let r = HttpResponse::page(&p).unwrap();
        assert_eq!(r.header("Content-Type"), Some(PAGE_MEDIA_TYPE));
        assert_eq!(serde_json::from_slice::<Page>(&r.body).unwrap(), p)
    }
    #[test]
    fn default_gateway_cannot_escape() {
        assert!(!GatewayPolicy::default().allow_host)
    }
    #[test]
    fn unknown_schema_rejected() {
        let w = WorldDefinition {
            schema_version: 88,
            id: "w".into(),
            profiles: vec![],
            computers: vec![],
            network: NetworkDefinition::default(),
            services: vec![],
            metadata: Value::Null,
        };
        assert!(w.validate().is_err())
    }
}
