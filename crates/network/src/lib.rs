//! Pure deterministic networking. No function in this crate performs host IO.
//! HTTP delivery is split into address/policy resolution and kernel-owned dispatch.
use cw_protocol::{HttpRequest, WorldDefinition};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::net::IpAddr;
use thiserror::Error;
use url::Url;

#[derive(Clone, Debug, Serialize, Deserialize, Error, PartialEq, Eq)]
pub enum NetworkError {
    #[error("unknown node: {0}")]
    UnknownNode(String),
    #[error("DNS resolution failed: {0}")]
    Dns(String),
    #[error("unreachable: {0}")]
    Unreachable(String),
    #[error("connection refused: {0}")]
    Refused(String),
    #[error("network policy denied: {0}")]
    Denied(String),
    #[error("invalid network configuration: {0}")]
    Invalid(String),
    #[error("transport closed or unknown: {0}")]
    Closed(u64),
    #[error("transport buffer full")]
    WouldBlock,
    #[error("simulated packet loss")]
    PacketLoss,
}
pub type Result<T> = std::result::Result<T, NetworkError>;

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Zone {
    #[default]
    Local,
    Internet,
    Host,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Node {
    pub id: String,
    pub addresses: Vec<String>,
    #[serde(default)]
    pub zone: Zone,
    #[serde(default)]
    pub resolver: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Link {
    pub a: String,
    pub b: String,
    #[serde(default = "yes")]
    pub bidirectional: bool,
    #[serde(default)]
    pub latency_us: u64,
    #[serde(default)]
    pub loss_per_million: u32,
    #[serde(default = "yes")]
    pub enabled: bool,
}
fn yes() -> bool {
    true
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Route {
    pub source: String,
    pub destination: String,
    /// Next hop node; explicit routes restrict routing when supplied for a source.
    pub via: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DnsRecord {
    pub name: String,
    #[serde(default)]
    pub addresses: Vec<String>,
    #[serde(default)]
    pub cname: Option<String>,
    #[serde(default = "default_ttl")]
    pub ttl_us: u64,
    #[serde(default)]
    pub resolver: Option<String>,
}
fn default_ttl() -> u64 {
    60_000_000
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Listener {
    pub node: String,
    pub address: String,
    pub port: u16,
    #[serde(default)]
    pub protocol: Transport,
    #[serde(default)]
    pub process: Option<u64>,
    #[serde(default)]
    pub service: Option<String>,
}
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Transport {
    #[default]
    Tcp,
    Udp,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NetworkConfig {
    #[serde(default)]
    pub nodes: Vec<Node>,
    #[serde(default)]
    pub links: Vec<Link>,
    #[serde(default)]
    pub routes: Vec<Route>,
    #[serde(default)]
    pub dns: Vec<DnsRecord>,
    /// Explicitly opt into a complete synthetic LAN when no links are supplied.
    #[serde(default)]
    pub implicit_lan: bool,
    #[serde(default)]
    pub gateway: GatewayPolicy,
    #[serde(default)]
    pub listeners: Vec<Listener>,
    #[serde(default = "yes")]
    pub allow_local: bool,
    #[serde(default = "yes")]
    pub allow_internet: bool,
    #[serde(default)]
    pub denied_pairs: BTreeSet<(String, String)>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GatewayPolicy {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub sources: BTreeSet<String>,
    #[serde(default)]
    pub hosts: BTreeSet<String>,
    #[serde(default)]
    pub ports: BTreeSet<u16>,
    #[serde(default)]
    pub schemes: BTreeSet<String>,
    /// Every resolved address must match an allowed CIDR. Empty means deny all.
    #[serde(default)]
    pub allowed_cidrs: Vec<String>,
    #[serde(default)]
    pub denied_cidrs: Vec<String>,
    #[serde(default = "default_budget")]
    pub max_response_bytes: usize,
    #[serde(default = "default_deadline")]
    pub timeout_us: u64,
}
impl Default for NetworkConfig {
    fn default() -> Self {
        Self {
            nodes: vec![],
            links: vec![],
            routes: vec![],
            dns: vec![],
            implicit_lan: false,
            gateway: GatewayPolicy::default(),
            listeners: vec![],
            allow_local: true,
            allow_internet: true,
            denied_pairs: BTreeSet::new(),
        }
    }
}
impl Default for GatewayPolicy {
    fn default() -> Self {
        Self {
            enabled: false,
            sources: BTreeSet::new(),
            hosts: BTreeSet::new(),
            ports: BTreeSet::new(),
            schemes: BTreeSet::new(),
            allowed_cidrs: vec![],
            denied_cidrs: vec![],
            max_response_bytes: default_budget(),
            timeout_us: default_deadline(),
        }
    }
}
fn default_budget() -> usize {
    1_048_576
}
fn default_deadline() -> u64 {
    30_000_000
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct NetworkTrace {
    pub sequence: u64,
    pub tick: u64,
    pub source: String,
    pub destination: String,
    pub kind: String,
    pub detail: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
struct CacheEntry {
    addresses: Vec<String>,
    expires: u64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ResolvedRequest {
    pub source: String,
    pub destination: String,
    pub address: String,
    pub port: u16,
    pub service_id: String,
    pub ready_at: u64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HostAuthorization {
    pub source: String,
    pub url: String,
    pub addresses: Vec<String>,
    pub port: u16,
    pub deadline: u64,
    pub timeout_us: u64,
    pub max_response_bytes: usize,
}
/// An optional adapter must pin one authorized address and disable automatic
/// redirects. Each redirected URL is separately authorized by the owner.
pub trait HostAdapter {
    fn execute(
        &mut self,
        authorization: &HostAuthorization,
        request: &HttpRequest,
    ) -> Result<cw_protocol::HttpResponse>;
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Connection {
    pub id: u64,
    pub source: String,
    pub destination: String,
    pub source_process: Option<u64>,
    pub destination_process: Option<u64>,
    pub port: u16,
    pub closed: bool,
    pub ready_at: u64,
    incoming: VecDeque<u8>,
    outgoing: VecDeque<u8>,
    pub capacity: usize,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Datagram {
    pub source: String,
    pub destination: String,
    pub port: u16,
    pub body: Vec<u8>,
    pub ready_at: u64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
struct PendingStream {
    id: u64,
    from_server: bool,
    bytes: Vec<u8>,
    ready_at: u64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Network {
    pub config: NetworkConfig,
    cache: BTreeMap<String, CacheEntry>,
    pub connections: BTreeMap<u64, Connection>,
    datagrams: VecDeque<Datagram>,
    traces: Vec<NetworkTrace>,
    next_id: u64,
    rng: u64,
    current_time: u64,
    dns_ready_at: u64,
    pending_streams: VecDeque<PendingStream>,
}
/// Serializable semantic view excludes diagnostic packet history.
#[derive(Serialize)]
pub struct NetworkState<'a> {
    config: &'a NetworkConfig,
    cache: &'a BTreeMap<String, CacheEntry>,
    connections: &'a BTreeMap<u64, Connection>,
    datagrams: &'a VecDeque<Datagram>,
    next_id: u64,
    rng: u64,
    current_time: u64,
    pending_streams: &'a VecDeque<PendingStream>,
}
impl From<NetworkError> for cw_protocol::SimError {
    fn from(error: NetworkError) -> Self {
        let code = match &error {
            NetworkError::Denied(_) => "network_denied",
            NetworkError::Dns(_) => "dns",
            NetworkError::Unreachable(_) => "unreachable",
            NetworkError::Refused(_) => "connection_refused",
            NetworkError::PacketLoss => "packet_loss",
            NetworkError::WouldBlock => "would_block",
            _ => "network",
        };
        Self::new(code, error.to_string())
    }
}
impl Network {
    pub fn semantic_state(&self) -> NetworkState<'_> {
        NetworkState {
            config: &self.config,
            cache: &self.cache,
            connections: &self.connections,
            datagrams: &self.datagrams,
            next_id: self.next_id,
            rng: self.rng,
            current_time: self.current_time,
            pending_streams: &self.pending_streams,
        }
    }
    pub fn new(world: &WorldDefinition) -> Result<Self> {
        Self::with_seed(world, 0)
    }
    pub fn with_seed(world: &WorldDefinition, seed: u64) -> Result<Self> {
        let mut nodes: Vec<Node> = world
            .network
            .nodes
            .iter()
            .map(|n| Node {
                id: n.id.clone(),
                addresses: vec![n.address.clone()],
                zone: match n.zone {
                    cw_protocol::NetworkZone::Local => Zone::Local,
                    cw_protocol::NetworkZone::Internet => Zone::Internet,
                    cw_protocol::NetworkZone::Host => Zone::Host,
                },
                resolver: None,
            })
            .collect();
        for computer in &world.computers {
            if !nodes.iter().any(|n| n.id == computer.node_id()) {
                nodes.push(Node {
                    id: computer.node_id().into(),
                    addresses: vec![computer.address.clone()],
                    zone: Zone::Local,
                    resolver: None,
                });
            }
        }
        let mut dns: Vec<DnsRecord> = world
            .network
            .dns
            .iter()
            .map(|d| DnsRecord {
                name: d.name.clone(),
                addresses: if d.address.parse::<IpAddr>().is_ok() {
                    vec![d.address.clone()]
                } else {
                    vec![]
                },
                cname: if d.address.parse::<IpAddr>().is_ok() {
                    None
                } else {
                    Some(d.address.clone())
                },
                ttl_us: d.ttl_us,
                resolver: d.resolver.clone(),
            })
            .collect();
        let mut listeners = vec![];
        for service in &world.services {
            let node = nodes
                .iter()
                .find(|n| n.id == service.node)
                .ok_or_else(|| NetworkError::UnknownNode(service.node.clone()))?;
            let address = node
                .addresses
                .first()
                .ok_or_else(|| NetworkError::Invalid("service node without address".into()))?
                .clone();
            listeners.push(Listener {
                node: service.node.clone(),
                address: address.clone(),
                port: service.port,
                protocol: Transport::Tcp,
                process: None,
                service: Some(service.id.clone()),
            });
            for domain in &service.domains {
                if !dns.iter().any(|d| d.name.eq_ignore_ascii_case(domain)) {
                    dns.push(DnsRecord {
                        name: domain.clone(),
                        addresses: vec![address.clone()],
                        cname: None,
                        ttl_us: default_ttl(),
                        resolver: None,
                    });
                }
            }
        }
        let gateway = &world.network.gateway;
        Self::from_config(
            NetworkConfig {
                nodes,
                links: world
                    .network
                    .links
                    .iter()
                    .map(|l| Link {
                        a: l.from.clone(),
                        b: l.to.clone(),
                        bidirectional: l.bidirectional,
                        latency_us: l.latency_us,
                        loss_per_million: l.loss_per_million,
                        enabled: true,
                    })
                    .collect(),
                routes: world
                    .network
                    .routes
                    .iter()
                    .map(|r| Route {
                        source: r.from.clone(),
                        destination: r.to.clone(),
                        via: r.via.clone().unwrap_or_else(|| r.to.clone()),
                    })
                    .collect(),
                dns,
                implicit_lan: world.network.implicit_lan,
                gateway: GatewayPolicy {
                    enabled: gateway.allow_host,
                    hosts: gateway
                        .host_allowlist
                        .iter()
                        .map(|h| h.to_ascii_lowercase())
                        .collect(),
                    sources: gateway.sources.iter().cloned().collect(),
                    ports: gateway.ports.iter().copied().collect(),
                    schemes: gateway.schemes.iter().cloned().collect(),
                    allowed_cidrs: gateway.allowed_cidrs.clone(),
                    denied_cidrs: gateway.denied_cidrs.clone(),
                    max_response_bytes: gateway.max_response_bytes,
                    timeout_us: gateway.timeout_us,
                },
                listeners,
                allow_local: gateway.allow_local,
                allow_internet: gateway.allow_internet,
                denied_pairs: gateway.denied_pairs.iter().cloned().collect(),
            },
            seed,
        )
    }
    pub fn advance(&mut self, now: u64) {
        self.current_time = self.current_time.max(now);
        self.cache.retain(|_, entry| entry.expires > now);
        let mut pending = VecDeque::new();
        while let Some(chunk) = self.pending_streams.pop_front() {
            if chunk.ready_at <= now {
                if let Some(connection) = self.connections.get_mut(&chunk.id) {
                    let queue = if chunk.from_server {
                        &mut connection.incoming
                    } else {
                        &mut connection.outgoing
                    };
                    queue.extend(chunk.bytes);
                }
            } else {
                pending.push_back(chunk);
            }
        }
        self.pending_streams = pending;
    }
    pub fn finish_http(
        &mut self,
        resolved: &ResolvedRequest,
        response: &cw_protocol::HttpResponse,
        now: u64,
    ) -> Result<u64> {
        self.record_response(resolved, response.status, now)
    }
    pub fn fail_http(&mut self, source: &str, url: &str, error: &str, now: u64) {
        self.trace(now, source, url, "http_error", error);
    }

    pub fn from_config(config: NetworkConfig, seed: u64) -> Result<Self> {
        let mut names = BTreeSet::new();
        let mut addresses = BTreeSet::new();
        for node in &config.nodes {
            if !names.insert(node.id.clone()) {
                return Err(NetworkError::Invalid(format!("duplicate node {}", node.id)));
            }
            if node.zone == Zone::Host {
                return Err(NetworkError::Invalid(
                    "host nodes are adapters, not synthetic nodes".into(),
                ));
            }
            for address in &node.addresses {
                let ip: IpAddr = address
                    .parse()
                    .map_err(|_| NetworkError::Invalid(format!("invalid address {address}")))?;
                if ip.is_loopback() || ip.is_unspecified() || !addresses.insert(ip) {
                    return Err(NetworkError::Invalid(format!(
                        "reserved or duplicate address {address}"
                    )));
                }
            }
        }
        for link in &config.links {
            if !names.contains(&link.a)
                || !names.contains(&link.b)
                || link.loss_per_million > 1_000_000
            {
                return Err(NetworkError::Invalid("invalid link".into()));
            }
        }
        for route in &config.routes {
            if !names.contains(&route.source) || !names.contains(&route.via) {
                return Err(NetworkError::Invalid(
                    "route references unknown node".into(),
                ));
            }
        }
        for cidr in config
            .gateway
            .allowed_cidrs
            .iter()
            .chain(config.gateway.denied_cidrs.iter())
        {
            parse_cidr(cidr)?;
        }
        let listeners = config.listeners.clone();
        let mut net = Self {
            config,
            cache: BTreeMap::new(),
            connections: BTreeMap::new(),
            datagrams: VecDeque::new(),
            traces: Vec::new(),
            next_id: 1,
            rng: seed,
            current_time: 0,
            dns_ready_at: 0,
            pending_streams: VecDeque::new(),
        };
        net.config.listeners.clear();
        for listener in listeners {
            net.listen(listener)?;
        }
        Ok(net)
    }
    pub fn traces(&self) -> &[NetworkTrace] {
        &self.traces
    }
    pub fn drain_traces(&mut self) -> Vec<NetworkTrace> {
        std::mem::take(&mut self.traces)
    }
    fn trace(
        &mut self,
        tick: u64,
        source: &str,
        destination: &str,
        kind: &str,
        detail: impl Into<String>,
    ) {
        let sequence = self.next_id;
        self.next_id += 1;
        self.traces.push(NetworkTrace {
            sequence,
            tick,
            source: source.into(),
            destination: destination.into(),
            kind: kind.into(),
            detail: detail.into(),
        });
    }
    fn random(&mut self) -> u64 {
        self.rng = self.rng.wrapping_add(0x9e3779b97f4a7c15);
        let mut z = self.rng;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d049bb133111eb);
        z ^ (z >> 31)
    }
    fn node(&self, id: &str) -> Result<&Node> {
        self.config
            .nodes
            .iter()
            .find(|n| n.id == id)
            .ok_or_else(|| NetworkError::UnknownNode(id.into()))
    }
    fn destination(&self, source: &str, address: &str) -> Result<String> {
        let ip: IpAddr = address
            .parse()
            .map_err(|_| NetworkError::Invalid(address.into()))?;
        if ip.is_loopback() {
            self.node(source)?;
            return Ok(source.into());
        }
        self.config
            .nodes
            .iter()
            .find(|n| {
                n.addresses
                    .iter()
                    .any(|a| a.parse::<IpAddr>().ok() == Some(ip))
            })
            .map(|n| n.id.clone())
            .ok_or_else(|| NetworkError::Unreachable(address.into()))
    }
    /// Deterministic breadth-first path in declared link order. Explicit source
    /// routes select the first hop; no host routing table is ever consulted.
    fn path(&self, source: &str, destination: &str, address: &str) -> Result<Vec<usize>> {
        self.node(source)?;
        let target = self.node(destination)?;
        if self
            .config
            .denied_pairs
            .contains(&(source.into(), destination.into()))
            || (target.zone == Zone::Local && !self.config.allow_local)
            || (target.zone == Zone::Internet && !self.config.allow_internet)
        {
            return Err(NetworkError::Denied(format!("{source} -> {destination}")));
        }
        if source == destination {
            return Ok(Vec::new());
        }
        let rules: Vec<_> = self
            .config
            .routes
            .iter()
            .filter(|r| r.source == source)
            .collect();
        let route = rules
            .iter()
            .filter(|r| {
                r.destination == destination
                    || cidr_contains(&r.destination, address).unwrap_or(false)
            })
            .max_by_key(|r| {
                r.destination
                    .split('/')
                    .nth(1)
                    .and_then(|p| p.parse::<u8>().ok())
                    .unwrap_or(255)
            });
        if !rules.is_empty() && route.is_none() {
            return Err(NetworkError::Unreachable(format!(
                "no route from {source} to {destination}"
            )));
        }
        if self.config.links.is_empty() && self.config.implicit_lan {
            return Ok(Vec::new());
        }
        let mut queue = VecDeque::from([(source.to_string(), Vec::new())]);
        let mut seen = BTreeSet::from([source.to_string()]);
        while let Some((current, path)) = queue.pop_front() {
            for (index, link) in self
                .config
                .links
                .iter()
                .enumerate()
                .filter(|(_, l)| l.enabled)
            {
                let next = if link.a == current {
                    &link.b
                } else if link.bidirectional && link.b == current {
                    &link.a
                } else {
                    continue;
                };
                if current == source && route.is_some_and(|r| r.via != *next) {
                    continue;
                }
                if !seen.insert(next.clone()) {
                    continue;
                }
                let mut next_path = path.clone();
                next_path.push(index);
                if next == destination {
                    return Ok(next_path);
                }
                queue.push_back((next.clone(), next_path));
            }
        }
        Err(NetworkError::Unreachable(format!(
            "{source} -> {destination}"
        )))
    }
    fn deliver(&mut self, source: &str, destination: &str, address: &str, now: u64) -> Result<u64> {
        let path = self.path(source, destination, address)?;
        let mut at = now;
        for index in path {
            let link = self.config.links[index].clone();
            at = at.saturating_add(link.latency_us);
            if link.loss_per_million > 0
                && self.random() % 1_000_000 < u64::from(link.loss_per_million)
            {
                self.trace(at, source, destination, "drop", "configured link loss");
                return Err(NetworkError::PacketLoss);
            }
        }
        Ok(at)
    }
    pub fn resolve(&mut self, source: &str, name: &str, now: u64) -> Result<Vec<String>> {
        let result = self.resolve_inner(source, name, now);
        if let Err(error) = &result {
            self.trace(now, source, name, "dns_error", error.to_string());
        }
        result
    }
    fn resolve_inner(&mut self, source: &str, name: &str, now: u64) -> Result<Vec<String>> {
        self.dns_ready_at = now;
        self.node(source)?;
        let normalized = name.trim_end_matches('.').to_ascii_lowercase();
        if let Ok(ip) = normalized.parse::<IpAddr>() {
            return Ok(vec![ip.to_string()]);
        }
        if normalized == "localhost" {
            return Ok(vec!["127.0.0.1".into()]);
        }
        let key = format!("{source}\0{normalized}");
        if let Some(entry) = self.cache.get(&key).filter(|e| e.expires > now) {
            let addresses = entry.addresses.clone();
            self.trace(now, source, &normalized, "dns_cache", addresses.join(","));
            return Ok(addresses);
        }
        let resolver = self.node(source)?.resolver.clone();
        if let Some(resolver) = &resolver {
            let address = self
                .node(resolver)?
                .addresses
                .first()
                .cloned()
                .ok_or_else(|| NetworkError::Dns("resolver has no address".into()))?;
            let at = self.deliver(source, resolver, &address, self.dns_ready_at)?;
            let source_address = self
                .node(source)?
                .addresses
                .first()
                .cloned()
                .ok_or_else(|| NetworkError::Dns("source has no address".into()))?;
            self.dns_ready_at = self.deliver(resolver, source, &source_address, at)?;
        }
        let mut current = normalized.clone();
        let mut seen = BTreeSet::new();
        let mut ttl = u64::MAX;
        for _ in 0..32 {
            if !seen.insert(current.clone()) {
                return Err(NetworkError::Dns("CNAME cycle".into()));
            }
            let record = self
                .config
                .dns
                .iter()
                .find(|r| {
                    r.name.trim_end_matches('.').eq_ignore_ascii_case(&current)
                        && (resolver.is_none() || r.resolver.is_none() || r.resolver == resolver)
                })
                .cloned()
                .ok_or_else(|| NetworkError::Dns(current.clone()))?;
            if let Some(record_resolver) = &record.resolver {
                if resolver.as_ref() != Some(record_resolver) {
                    let address = self
                        .node(record_resolver)?
                        .addresses
                        .first()
                        .cloned()
                        .ok_or_else(|| NetworkError::Dns("resolver has no address".into()))?;
                    let at = self.deliver(source, record_resolver, &address, self.dns_ready_at)?;
                    let source_address = self
                        .node(source)?
                        .addresses
                        .first()
                        .cloned()
                        .ok_or_else(|| NetworkError::Dns("source has no address".into()))?;
                    self.dns_ready_at =
                        self.deliver(record_resolver, source, &source_address, at)?;
                }
            }
            ttl = ttl.min(record.ttl_us);
            if let Some(alias) = record.cname {
                current = alias.trim_end_matches('.').to_ascii_lowercase();
                continue;
            }
            if record.addresses.is_empty()
                || record
                    .addresses
                    .iter()
                    .any(|a| a.parse::<IpAddr>().is_err())
            {
                return Err(NetworkError::Dns(format!("invalid record {current}")));
            }
            self.cache.insert(
                key,
                CacheEntry {
                    addresses: record.addresses.clone(),
                    expires: self.dns_ready_at.saturating_add(ttl),
                },
            );
            self.trace(
                self.dns_ready_at,
                source,
                &normalized,
                "dns",
                record.addresses.join(","),
            );
            return Ok(record.addresses);
        }
        Err(NetworkError::Dns("CNAME depth exceeded".into()))
    }
    pub fn listen(&mut self, listener: Listener) -> Result<()> {
        self.node(&listener.node)?;
        let wildcard = listener.address == "0.0.0.0" || listener.address == "::";
        if !wildcard && self.destination(&listener.node, &listener.address)? != listener.node {
            return Err(NetworkError::Denied(
                "listener address not owned by node".into(),
            ));
        }
        if self.config.listeners.iter().any(|l| {
            l.node == listener.node
                && l.port == listener.port
                && l.protocol == listener.protocol
                && (l.address == listener.address
                    || wildcard
                    || l.address == "0.0.0.0"
                    || l.address == "::")
        }) {
            return Err(NetworkError::Refused("address already in use".into()));
        }
        self.config.listeners.push(listener);
        Ok(())
    }
    /// Associate a placed HTTP service with its computer process. A process exit
    /// then revokes the listener through the same lifecycle as stream listeners.
    pub fn own_service_listener(&mut self, service_id: &str, node: &str, pid: u64) -> Result<()> {
        let listener = self
            .config
            .listeners
            .iter_mut()
            .find(|listener| {
                listener.node == node && listener.service.as_deref() == Some(service_id)
            })
            .ok_or_else(|| {
                NetworkError::Refused(format!("service listener {service_id} on {node}"))
            })?;
        listener.process = Some(pid);
        Ok(())
    }
    pub fn start_service_listener(
        &mut self,
        service: &cw_protocol::ServiceDefinition,
        pid: Option<u64>,
    ) -> Result<()> {
        if let Some(listener) = self.config.listeners.iter_mut().find(|listener| {
            listener.node == service.node
                && listener.service.as_deref() == Some(service.id.as_str())
        }) {
            listener.process = pid;
            return Ok(());
        }
        let address = self
            .node(&service.node)?
            .addresses
            .first()
            .cloned()
            .ok_or_else(|| NetworkError::Invalid("service node has no address".into()))?;
        self.listen(Listener {
            node: service.node.clone(),
            address,
            port: service.port,
            protocol: Transport::Tcp,
            process: pid,
            service: Some(service.id.clone()),
        })
    }
    pub fn close_process(&mut self, node: &str, pid: u64, now: u64) {
        self.config
            .listeners
            .retain(|l| !(l.node == node && l.process == Some(pid)));
        for connection in self.connections.values_mut() {
            if (connection.source == node && connection.source_process == Some(pid))
                || (connection.destination == node && connection.destination_process == Some(pid))
            {
                connection.closed = true;
            }
        }
        self.trace(now, node, node, "process_cleanup", pid.to_string());
    }
    fn listener(
        &self,
        node: &str,
        address: &str,
        port: u16,
        protocol: Transport,
    ) -> Result<&Listener> {
        self.config
            .listeners
            .iter()
            .find(|l| {
                l.node == node
                    && l.port == port
                    && l.protocol == protocol
                    && (l.address == address || l.address == "0.0.0.0" || l.address == "::")
            })
            .ok_or_else(|| NetworkError::Refused(format!("{address}:{port}")))
    }
    /// Revalidate the bound service endpoint when a queued request arrives.
    /// Listener revocation or replacement cannot redirect an in-flight request.
    pub fn validate_delivery(&self, resolved: &ResolvedRequest) -> Result<()> {
        let listener = self.listener(
            &resolved.destination,
            &resolved.address,
            resolved.port,
            Transport::Tcp,
        )?;
        if listener.service.as_deref() != Some(resolved.service_id.as_str()) {
            return Err(NetworkError::Refused(format!(
                "service {} no longer owns destination listener",
                resolved.service_id
            )));
        }
        Ok(())
    }
    pub fn prepare_http(
        &mut self,
        source: &str,
        request: &HttpRequest,
        now: u64,
    ) -> Result<ResolvedRequest> {
        let url = Url::parse(&request.url).map_err(|e| NetworkError::Invalid(e.to_string()))?;
        if !matches!(url.scheme(), "http" | "https") {
            return Err(NetworkError::Denied("unsupported scheme".into()));
        }
        if !url.username().is_empty() || url.password().is_some() {
            return Err(NetworkError::Denied("URL credentials unsupported".into()));
        }
        let host = url
            .host_str()
            .ok_or_else(|| NetworkError::Invalid("missing hostname".into()))?
            .trim_matches(['[', ']']);
        let port = url
            .port_or_known_default()
            .ok_or_else(|| NetworkError::Invalid("missing port".into()))?;
        let addresses = self.resolve(source, host, now)?;
        let address = addresses
            .first()
            .ok_or_else(|| NetworkError::Dns(host.into()))?
            .clone();
        let destination = self.destination(source, &address).map_err(|_| {
            NetworkError::Denied(format!("host address {address} requires explicit adapter"))
        })?;
        let ready_at = self.deliver(source, &destination, &address, self.dns_ready_at.max(now))?;
        let service_id = self
            .listener(&destination, &address, port, Transport::Tcp)?
            .service
            .clone()
            .ok_or_else(|| NetworkError::Refused("listener is not HTTP service".into()))?;
        self.trace(
            now,
            source,
            &destination,
            "http_request",
            format!("{} {}", request.method, request.url),
        );
        Ok(ResolvedRequest {
            source: source.into(),
            destination,
            address,
            port,
            service_id,
            ready_at,
        })
    }
    pub fn record_response(
        &mut self,
        resolved: &ResolvedRequest,
        status: u16,
        now: u64,
    ) -> Result<u64> {
        let source_address = self
            .node(&resolved.source)?
            .addresses
            .first()
            .cloned()
            .unwrap_or_else(|| "127.0.0.1".into());
        let ready = self.deliver(
            &resolved.destination,
            &resolved.source,
            &source_address,
            now,
        )?;
        self.trace(
            ready,
            &resolved.destination,
            &resolved.source,
            "http_response",
            status.to_string(),
        );
        Ok(ready)
    }
    pub fn connect(
        &mut self,
        source: &str,
        host: &str,
        port: u16,
        process: Option<u64>,
        now: u64,
    ) -> Result<u64> {
        self.advance(now);
        let addresses = self.resolve(source, host, now)?;
        let address = addresses
            .first()
            .ok_or_else(|| NetworkError::Dns(host.into()))?;
        let destination = self.destination(source, address)?;
        let at = self.deliver(source, &destination, address, self.dns_ready_at.max(now))?;
        let destination_process = self
            .listener(&destination, address, port, Transport::Tcp)?
            .process;
        let id = self.next_id;
        self.next_id += 1;
        self.connections.insert(
            id,
            Connection {
                id,
                source: source.into(),
                destination: destination.clone(),
                source_process: process,
                destination_process,
                port,
                closed: false,
                ready_at: at,
                incoming: VecDeque::new(),
                outgoing: VecDeque::new(),
                capacity: 65536,
            },
        );
        self.trace(at, source, &destination, "connect", id.to_string());
        Ok(id)
    }
    /// Writes bytes to the peer's receive queue; `from_server` selects direction.
    pub fn write(&mut self, id: u64, from_server: bool, bytes: &[u8]) -> Result<usize> {
        self.write_at(id, from_server, bytes, self.current_time)
    }
    pub fn write_at(
        &mut self,
        id: u64,
        from_server: bool,
        bytes: &[u8],
        now: u64,
    ) -> Result<usize> {
        self.advance(now);
        let connection = self
            .connections
            .get(&id)
            .filter(|c| !c.closed)
            .ok_or(NetworkError::Closed(id))?;
        let queue = if from_server {
            &connection.incoming
        } else {
            &connection.outgoing
        };
        let queued: usize = self
            .pending_streams
            .iter()
            .filter(|p| p.id == id && p.from_server == from_server)
            .map(|p| p.bytes.len())
            .sum();
        let n = bytes.len().min(
            connection
                .capacity
                .saturating_sub(queue.len().saturating_add(queued)),
        );
        if n == 0 && !bytes.is_empty() {
            return Err(NetworkError::WouldBlock);
        }
        let (source, destination) = if from_server {
            (connection.destination.clone(), connection.source.clone())
        } else {
            (connection.source.clone(), connection.destination.clone())
        };
        let address = self
            .node(&destination)?
            .addresses
            .first()
            .cloned()
            .ok_or_else(|| NetworkError::Unreachable(destination.clone()))?;
        let start = now.max(connection.ready_at);
        let ready_at = self.deliver(&source, &destination, &address, start)?;
        self.pending_streams.push_back(PendingStream {
            id,
            from_server,
            bytes: bytes[..n].to_vec(),
            ready_at,
        });
        self.trace(
            now,
            &source,
            &destination,
            "stream_write",
            format!("{id}: {n} bytes"),
        );
        self.advance(now);
        Ok(n)
    }
    pub fn read(&mut self, id: u64, on_server: bool, max: usize) -> Result<Vec<u8>> {
        let connection = self
            .connections
            .get_mut(&id)
            .ok_or(NetworkError::Closed(id))?;
        let queue = if on_server {
            &mut connection.outgoing
        } else {
            &mut connection.incoming
        };
        if queue.is_empty() && !connection.closed {
            return Err(NetworkError::WouldBlock);
        }
        Ok(queue.drain(..max.min(queue.len())).collect())
    }
    pub fn close(&mut self, id: u64) -> Result<()> {
        self.connections
            .get_mut(&id)
            .ok_or(NetworkError::Closed(id))?
            .closed = true;
        Ok(())
    }
    pub fn send_datagram(
        &mut self,
        source: &str,
        host: &str,
        port: u16,
        body: Vec<u8>,
        now: u64,
    ) -> Result<()> {
        if body.len() > 65507 {
            return Err(NetworkError::Invalid("datagram exceeds 65507 bytes".into()));
        }
        let addresses = self.resolve(source, host, now)?;
        let address = addresses
            .first()
            .ok_or_else(|| NetworkError::Dns(host.into()))?;
        let destination = self.destination(source, address)?;
        self.listener(&destination, address, port, Transport::Udp)?;
        let ready_at = self.deliver(source, &destination, address, self.dns_ready_at.max(now))?;
        self.trace(
            now,
            source,
            &destination,
            "datagram",
            format!("{} bytes", body.len()),
        );
        self.datagrams.push_back(Datagram {
            source: source.into(),
            destination,
            port,
            body,
            ready_at,
        });
        Ok(())
    }
    pub fn receive_datagram(&mut self, node: &str, port: u16, now: u64) -> Option<Datagram> {
        let index = self
            .datagrams
            .iter()
            .position(|d| d.destination == node && d.port == port && d.ready_at <= now)?;
        self.datagrams.remove(index)
    }
    /// Caller supplies results from an explicit resolver adapter. Validate every
    /// address, including mixed public/private responses, before any connection.
    pub fn authorize_host(
        &self,
        source: &str,
        url: &str,
        addresses: Vec<String>,
        now: u64,
    ) -> Result<HostAuthorization> {
        self.node(source)?;
        let policy = &self.config.gateway;
        let parsed = Url::parse(url).map_err(|e| NetworkError::Invalid(e.to_string()))?;
        let host = parsed
            .host_str()
            .ok_or_else(|| NetworkError::Invalid("missing hostname".into()))?
            .trim_matches(['[', ']'])
            .to_ascii_lowercase();
        let port = parsed
            .port_or_known_default()
            .ok_or_else(|| NetworkError::Denied("unknown scheme".into()))?;
        if !policy.enabled
            || !policy.sources.contains(source)
            || !policy.hosts.contains(&host)
            || !policy.ports.contains(&port)
            || !policy.schemes.contains(parsed.scheme())
            || !parsed.username().is_empty()
            || parsed.password().is_some()
        {
            return Err(NetworkError::Denied(
                "host/source/scheme/port policy".into(),
            ));
        }
        if addresses.is_empty() {
            return Err(NetworkError::Denied("empty resolution".into()));
        }
        for address in &addresses {
            let ip: IpAddr = address
                .parse()
                .map_err(|_| NetworkError::Denied("invalid resolved address".into()))?;
            if let Some(literal) = parsed
                .host_str()
                .and_then(|s| s.trim_matches(['[', ']']).parse::<IpAddr>().ok())
            {
                if literal != ip {
                    return Err(NetworkError::Denied("literal IP mismatch".into()));
                }
            }
            if ip.is_unspecified()
                || ip.is_multicast()
                || policy
                    .denied_cidrs
                    .iter()
                    .any(|c| cidr_contains(c, address).unwrap_or(true))
                || !policy
                    .allowed_cidrs
                    .iter()
                    .any(|c| cidr_contains(c, address).unwrap_or(false))
            {
                return Err(NetworkError::Denied(format!("address {address}")));
            }
        }
        Ok(HostAuthorization {
            source: source.into(),
            url: url.into(),
            addresses,
            port,
            deadline: now.saturating_add(policy.timeout_us),
            timeout_us: policy.timeout_us,
            max_response_bytes: policy.max_response_bytes,
        })
    }
}
fn parse_cidr(cidr: &str) -> Result<(IpAddr, u8)> {
    let (address, prefix) = cidr
        .split_once('/')
        .ok_or_else(|| NetworkError::Invalid(format!("CIDR expected: {cidr}")))?;
    let address: IpAddr = address
        .parse()
        .map_err(|_| NetworkError::Invalid(cidr.into()))?;
    let prefix: u8 = prefix
        .parse()
        .map_err(|_| NetworkError::Invalid(cidr.into()))?;
    if prefix > if address.is_ipv4() { 32 } else { 128 } {
        return Err(NetworkError::Invalid(cidr.into()));
    }
    Ok((address, prefix))
}
pub fn cidr_contains(cidr: &str, address: &str) -> Result<bool> {
    let (network, prefix) = parse_cidr(cidr)?;
    let address: IpAddr = address
        .parse()
        .map_err(|_| NetworkError::Invalid(address.into()))?;
    Ok(match (network, address) {
        (IpAddr::V4(a), IpAddr::V4(b)) => {
            let mask = if prefix == 0 {
                0
            } else {
                u32::MAX << (32 - prefix)
            };
            u32::from(a) & mask == u32::from(b) & mask
        }
        (IpAddr::V6(a), IpAddr::V6(b)) => {
            if let Some(v4) = b.to_ipv4_mapped() {
                if let Some(net) = a.to_ipv4_mapped() {
                    if prefix >= 96 {
                        return cidr_contains(&format!("{net}/{}", prefix - 96), &v4.to_string());
                    }
                }
            }
            let mask = if prefix == 0 {
                0
            } else {
                u128::MAX << (128 - prefix)
            };
            u128::from(a) & mask == u128::from(b) & mask
        }
        (IpAddr::V4(a), IpAddr::V6(b)) => match b.to_ipv4_mapped() {
            Some(b) => cidr_contains(&format!("{a}/{prefix}"), &b.to_string())?,
            None => false,
        },
        _ => false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn config() -> NetworkConfig {
        NetworkConfig {
            nodes: vec![
                Node {
                    id: "a".into(),
                    addresses: vec!["10.0.0.1".into()],
                    zone: Zone::Local,
                    resolver: None,
                },
                Node {
                    id: "b".into(),
                    addresses: vec!["10.0.0.2".into()],
                    zone: Zone::Local,
                    resolver: None,
                },
            ],
            dns: vec![DnsRecord {
                name: "site.test".into(),
                addresses: vec!["10.0.0.2".into()],
                cname: None,
                ttl_us: 100,
                resolver: None,
            }],
            listeners: vec![Listener {
                node: "b".into(),
                address: "0.0.0.0".into(),
                port: 80,
                protocol: Transport::Tcp,
                process: Some(9),
                service: Some("site".into()),
            }],
            implicit_lan: true,
            ..NetworkConfig::default()
        }
    }
    #[test]
    fn http_resolves_real_listener() {
        let mut net = Network::from_config(config(), 1).unwrap();
        let resolved = net
            .prepare_http("a", &HttpRequest::get("http://site.test/hello"), 0)
            .unwrap();
        assert_eq!(resolved.service_id, "site");
        assert_eq!(resolved.destination, "b");
        assert_eq!(net.traces()[0].kind, "dns");
        assert_eq!(
            net.finish_http(&resolved, &cw_protocol::HttpResponse::text(200, "yes"), 0)
                .unwrap(),
            0
        );
    }
    #[test]
    fn unknown_dns_never_escapes() {
        let mut net = Network::from_config(config(), 1).unwrap();
        assert!(matches!(
            net.prepare_http("a", &HttpRequest::get("http://google.com/"), 0),
            Err(NetworkError::Dns(_))
        ));
        assert!(matches!(
            net.prepare_http("a", &HttpRequest::get("http://8.8.8.8/"), 0),
            Err(NetworkError::Denied(_))
        ));
    }
    #[test]
    fn loopback_is_source_local() {
        let mut net = Network::from_config(config(), 1).unwrap();
        assert!(net
            .prepare_http("a", &HttpRequest::get("http://localhost/"), 0)
            .is_err());
        assert_eq!(
            net.prepare_http("b", &HttpRequest::get("http://localhost/"), 0)
                .unwrap()
                .destination,
            "b"
        );
    }
    #[test]
    fn latency_and_directed_links() {
        let mut c = config();
        c.links.push(Link {
            a: "a".into(),
            b: "b".into(),
            bidirectional: false,
            latency_us: 15,
            loss_per_million: 0,
            enabled: true,
        });
        let mut net = Network::from_config(c, 1).unwrap();
        let r = net
            .prepare_http("a", &HttpRequest::get("http://site.test"), 10)
            .unwrap();
        assert_eq!(r.ready_at, 25);
        assert!(net
            .finish_http(&r, &cw_protocol::HttpResponse::text(200, ""), 25)
            .is_err());
    }
    #[test]
    fn disconnected_resolver_cannot_answer() {
        let mut c = config();
        c.dns[0].resolver = Some("b".into());
        c.implicit_lan = false;
        let mut net = Network::from_config(c, 1).unwrap();
        assert!(matches!(
            net.resolve("a", "site.test", 0),
            Err(NetworkError::Unreachable(_))
        ));
    }
    #[test]
    fn cache_ttl_and_cname_cycle() {
        let mut net = Network::from_config(config(), 1).unwrap();
        assert_eq!(net.resolve("a", "site.test", 0).unwrap(), vec!["10.0.0.2"]);
        net.config.dns[0].addresses = vec!["10.0.0.3".into()];
        assert_eq!(net.resolve("a", "site.test", 99).unwrap(), vec!["10.0.0.2"]);
        assert_eq!(
            net.resolve("a", "site.test", 100).unwrap(),
            vec!["10.0.0.3"]
        );
        net.config.dns[0].cname = Some("site.test".into());
        assert!(net.resolve("b", "site.test", 100).is_err());
    }
    #[test]
    fn stream_buffers_and_owner_cleanup() {
        let mut net = Network::from_config(config(), 1).unwrap();
        let id = net.connect("a", "site.test", 80, Some(3), 0).unwrap();
        assert_eq!(net.write(id, false, b"hello").unwrap(), 5);
        assert_eq!(net.read(id, true, 2).unwrap(), b"he");
        assert_eq!(net.read(id, true, 100).unwrap(), b"llo");
        assert!(matches!(
            net.read(id, true, 1),
            Err(NetworkError::WouldBlock)
        ));
        net.close_process("b", 9, 10);
        assert_eq!(net.read(id, false, 5).unwrap(), b"");
        assert!(net.write(id, false, b"x").is_err());
        assert!(net.connect("a", "site.test", 80, None, 11).is_err());
    }
    #[test]
    fn datagram_timing_and_snapshot() {
        let mut c = config();
        c.listeners[0].protocol = Transport::Udp;
        c.links.push(Link {
            a: "a".into(),
            b: "b".into(),
            bidirectional: true,
            latency_us: 15,
            loss_per_million: 0,
            enabled: true,
        });
        let mut net = Network::from_config(c, 1).unwrap();
        net.send_datagram("a", "site.test", 80, b"hello".to_vec(), 10)
            .unwrap();
        let mut restored: Network =
            serde_json::from_str(&serde_json::to_string(&net).unwrap()).unwrap();
        assert!(restored.receive_datagram("b", 80, 24).is_none());
        assert_eq!(
            restored.receive_datagram("b", 80, 25).unwrap().body,
            b"hello"
        );
    }
    #[test]
    fn local_and_internet_policy() {
        let mut c = config();
        c.nodes[1].zone = Zone::Internet;
        c.allow_internet = false;
        let mut net = Network::from_config(c, 1).unwrap();
        assert!(matches!(
            net.prepare_http("a", &HttpRequest::get("http://site.test"), 0),
            Err(NetworkError::Denied(_))
        ));
    }
    fn gateway() -> Network {
        let mut c = config();
        c.gateway = GatewayPolicy {
            enabled: true,
            sources: BTreeSet::from(["a".into()]),
            hosts: BTreeSet::from(["example.com".into()]),
            ports: BTreeSet::from([443]),
            schemes: BTreeSet::from(["https".into()]),
            allowed_cidrs: vec!["0.0.0.0/0".into()],
            denied_cidrs: vec!["10.0.0.0/8".into(), "127.0.0.0/8".into()],
            ..GatewayPolicy::default()
        };
        Network::from_config(c, 0).unwrap()
    }
    #[test]
    fn gateway_checks_every_address_and_redirect() {
        let net = gateway();
        assert!(net
            .authorize_host("a", "https://example.com", vec!["93.184.215.14".into()], 0)
            .is_ok());
        assert!(net
            .authorize_host(
                "a",
                "https://example.com",
                vec!["93.184.215.14".into(), "10.1.2.3".into()],
                0
            )
            .is_err());
        assert!(net
            .authorize_host(
                "a",
                "https://example.com",
                vec!["::ffff:127.0.0.1".into()],
                0
            )
            .is_err());
        assert!(net
            .authorize_host(
                "a",
                "https://example.com.evil.test",
                vec!["93.184.215.14".into()],
                0
            )
            .is_err());
        assert!(net
            .authorize_host("a", "http://example.com", vec!["93.184.215.14".into()], 0)
            .is_err());
        assert!(net
            .authorize_host("b", "https://example.com", vec!["93.184.215.14".into()], 0)
            .is_err());
    }
    #[test]
    fn deterministic_packet_loss() {
        let mut c = config();
        c.links.push(Link {
            a: "a".into(),
            b: "b".into(),
            bidirectional: true,
            latency_us: 1,
            loss_per_million: 400000,
            enabled: true,
        });
        let mut a = Network::from_config(c.clone(), 2).unwrap();
        let mut b = Network::from_config(c, 2).unwrap();
        for i in 0..100 {
            assert_eq!(
                a.prepare_http("a", &HttpRequest::get("http://site.test"), i)
                    .is_ok(),
                b.prepare_http("a", &HttpRequest::get("http://site.test"), i)
                    .is_ok()
            );
        }
        assert_eq!(
            serde_json::to_string(&a).unwrap(),
            serde_json::to_string(&b).unwrap()
        );
    }
    #[test]
    fn stream_delivery_is_timed_and_checkpointed() {
        let mut c = config();
        c.links.push(Link {
            a: "a".into(),
            b: "b".into(),
            bidirectional: true,
            latency_us: 5,
            loss_per_million: 0,
            enabled: true,
        });
        let mut net = Network::from_config(c, 0).unwrap();
        let id = net.connect("a", "site.test", 80, None, 0).unwrap();
        net.write_at(id, false, b"hi", 5).unwrap();
        assert!(matches!(
            net.read(id, true, 5),
            Err(NetworkError::WouldBlock)
        ));
        let mut restored: Network =
            serde_json::from_str(&serde_json::to_string(&net).unwrap()).unwrap();
        restored.advance(10);
        assert_eq!(restored.read(id, true, 5).unwrap(), b"hi");
    }
    #[test]
    fn dns_roundtrip_precedes_http() {
        let mut c = config();
        c.dns[0].resolver = Some("b".into());
        c.links.push(Link {
            a: "a".into(),
            b: "b".into(),
            bidirectional: true,
            latency_us: 5,
            loss_per_million: 0,
            enabled: true,
        });
        let mut net = Network::from_config(c, 0).unwrap();
        let request = HttpRequest::get("http://site.test");
        assert_eq!(net.prepare_http("a", &request, 0).unwrap().ready_at, 15);
        assert_eq!(net.prepare_http("a", &request, 15).unwrap().ready_at, 20);
    }
    #[test]
    fn no_links_requires_explicit_lan() {
        let mut c = config();
        c.implicit_lan = false;
        let mut net = Network::from_config(c, 0).unwrap();
        assert!(matches!(
            net.prepare_http("a", &HttpRequest::get("http://site.test"), 0),
            Err(NetworkError::Unreachable(_))
        ));
    }
    #[test]
    fn literal_host_cannot_spoof_adapter_resolution() {
        let mut net = gateway();
        net.config.gateway.hosts.insert("127.0.0.1".into());
        assert!(net
            .authorize_host("a", "https://127.0.0.1", vec!["93.184.215.14".into()], 0)
            .is_err());
        net.config.gateway.hosts.insert("93.184.215.14".into());
        assert!(net
            .authorize_host(
                "a",
                "https://93.184.215.14",
                vec!["93.184.215.14".into()],
                0
            )
            .is_ok());
        assert!(net
            .authorize_host(
                "a",
                "https://user:pass@example.com",
                vec!["93.184.215.14".into()],
                0
            )
            .is_err());
    }
    #[test]
    fn queued_delivery_rechecks_listener_lifecycle() {
        let mut net = Network::from_config(config(), 0).unwrap();
        let resolved = net
            .prepare_http("a", &HttpRequest::get("http://site.test"), 0)
            .unwrap();
        net.validate_delivery(&resolved).unwrap();
        net.close_process("b", 9, 1);
        assert!(net.validate_delivery(&resolved).is_err());
        net.listen(Listener {
            node: "b".into(),
            address: "0.0.0.0".into(),
            port: 80,
            protocol: Transport::Tcp,
            process: Some(10),
            service: Some("replacement".into()),
        })
        .unwrap();
        assert!(net.validate_delivery(&resolved).is_err());
    }
    proptest::proptest! {
        #[test]fn denied_private_addresses_never_pass(b in proptest::num::u8::ANY,c in proptest::num::u8::ANY,d in proptest::num::u8::ANY){let net=gateway();let address=format!("10.{b}.{c}.{d}");proptest::prop_assert!(net.authorize_host("a","https://example.com",vec![address],0).is_err());}
        #[test]fn cidr_exact_matches_only_self(a in proptest::num::u32::ANY,b in proptest::num::u32::ANY){let aa=std::net::Ipv4Addr::from(a);let bb=std::net::Ipv4Addr::from(b);proptest::prop_assert_eq!(cidr_contains(&format!("{aa}/32"),&bb.to_string()).unwrap(),a==b);}
    }
}
