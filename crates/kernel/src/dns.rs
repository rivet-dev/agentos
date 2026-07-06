use hickory_proto::rr::domain::Name;
use hickory_proto::rr::rdata::{A, AAAA};
use hickory_proto::rr::{RData, Record, RecordType};
#[cfg(not(target_arch = "wasm32"))]
use hickory_resolver::config::{NameServerConfig, ResolverConfig};
#[cfg(not(target_arch = "wasm32"))]
use hickory_resolver::net::runtime::TokioRuntimeProvider;
#[cfg(not(target_arch = "wasm32"))]
use hickory_resolver::TokioResolver;
use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fmt;
use std::net::{IpAddr, SocketAddr};
#[cfg(not(target_arch = "wasm32"))]
use std::net::{Ipv4Addr, Ipv6Addr};
use std::sync::Arc;
#[cfg(not(target_arch = "wasm32"))]
use std::sync::{mpsc, Mutex};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DnsConfig {
    pub name_servers: Vec<SocketAddr>,
    pub overrides: BTreeMap<String, Vec<IpAddr>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DnsLookupPolicy {
    CheckPermissions,
    SkipPermissions,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DnsLookupRequest {
    hostname: String,
    name_servers: Vec<SocketAddr>,
}

impl DnsLookupRequest {
    pub fn new(hostname: impl Into<String>, name_servers: Vec<SocketAddr>) -> Self {
        Self {
            hostname: hostname.into(),
            name_servers,
        }
    }

    pub fn hostname(&self) -> &str {
        &self.hostname
    }

    pub fn name_servers(&self) -> &[SocketAddr] {
        &self.name_servers
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DnsRecordLookupRequest {
    hostname: String,
    name_servers: Vec<SocketAddr>,
    record_type: RecordType,
}

impl DnsRecordLookupRequest {
    pub fn new(
        hostname: impl Into<String>,
        name_servers: Vec<SocketAddr>,
        record_type: RecordType,
    ) -> Self {
        Self {
            hostname: hostname.into(),
            name_servers,
            record_type,
        }
    }

    pub fn hostname(&self) -> &str {
        &self.hostname
    }

    pub fn name_servers(&self) -> &[SocketAddr] {
        &self.name_servers
    }

    pub const fn record_type(&self) -> RecordType {
        self.record_type
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DnsResolutionSource {
    Literal,
    Override,
    Resolver,
}

impl DnsResolutionSource {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Literal => "literal",
            Self::Override => "override",
            Self::Resolver => "resolver",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DnsResolution {
    hostname: String,
    source: DnsResolutionSource,
    addresses: Vec<IpAddr>,
}

impl DnsResolution {
    pub fn new(
        hostname: impl Into<String>,
        source: DnsResolutionSource,
        addresses: Vec<IpAddr>,
    ) -> Self {
        Self {
            hostname: hostname.into(),
            source,
            addresses,
        }
    }

    pub fn hostname(&self) -> &str {
        &self.hostname
    }

    pub const fn source(&self) -> DnsResolutionSource {
        self.source
    }

    pub fn addresses(&self) -> &[IpAddr] {
        &self.addresses
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DnsRecordResolution {
    hostname: String,
    source: DnsResolutionSource,
    records: Vec<Record>,
}

impl DnsRecordResolution {
    pub fn new(
        hostname: impl Into<String>,
        source: DnsResolutionSource,
        records: Vec<Record>,
    ) -> Self {
        Self {
            hostname: hostname.into(),
            source,
            records,
        }
    }

    pub fn hostname(&self) -> &str {
        &self.hostname
    }

    pub const fn source(&self) -> DnsResolutionSource {
        self.source
    }

    pub fn records(&self) -> &[Record] {
        &self.records
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DnsResolverErrorKind {
    InvalidInput,
    LookupFailed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DnsResolverError {
    kind: DnsResolverErrorKind,
    message: String,
}

impl DnsResolverError {
    pub fn invalid_input(message: impl Into<String>) -> Self {
        Self {
            kind: DnsResolverErrorKind::InvalidInput,
            message: message.into(),
        }
    }

    pub fn lookup_failed(message: impl Into<String>) -> Self {
        Self {
            kind: DnsResolverErrorKind::LookupFailed,
            message: message.into(),
        }
    }

    pub const fn kind(&self) -> DnsResolverErrorKind {
        self.kind
    }
}

impl fmt::Display for DnsResolverError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl Error for DnsResolverError {}

pub trait DnsResolver {
    fn lookup_ip(&self, request: &DnsLookupRequest) -> Result<Vec<IpAddr>, DnsResolverError>;
    fn lookup_records(
        &self,
        request: &DnsRecordLookupRequest,
    ) -> Result<Vec<Record>, DnsResolverError>;
}

pub type SharedDnsResolver = Arc<dyn DnsResolver + Send + Sync>;

#[cfg(not(target_arch = "wasm32"))]
pub struct HickoryDnsResolver {
    worker: Mutex<mpsc::Sender<DnsWorkerRequest>>,
}

/// On wasm the kernel has no tokio runtime or host DNS stack, so the resolver is
/// a unit type whose `DnsResolver` impl reports that name resolution is
/// unavailable; guests must supply DNS overrides or literal addresses.
#[cfg(target_arch = "wasm32")]
pub struct HickoryDnsResolver;

#[cfg(not(target_arch = "wasm32"))]
enum DnsWorkerRequest {
    LookupIp {
        hostname: String,
        name_servers: Vec<SocketAddr>,
        response: mpsc::Sender<Result<Vec<IpAddr>, DnsResolverError>>,
    },
    LookupRecords {
        hostname: String,
        name_servers: Vec<SocketAddr>,
        record_type: RecordType,
        response: mpsc::Sender<Result<Vec<Record>, DnsResolverError>>,
    },
}

#[cfg(not(target_arch = "wasm32"))]
impl Default for HickoryDnsResolver {
    fn default() -> Self {
        let (sender, receiver) = mpsc::channel();
        std::thread::Builder::new()
            .name("secure-exec-dns-resolver".to_owned())
            .spawn(move || run_dns_worker(receiver))
            .expect("failed to spawn DNS resolver worker");
        Self {
            worker: Mutex::new(sender),
        }
    }
}

#[cfg(target_arch = "wasm32")]
impl Default for HickoryDnsResolver {
    fn default() -> Self {
        Self
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl HickoryDnsResolver {
    fn send_lookup_ip(
        &self,
        hostname: String,
        name_servers: Vec<SocketAddr>,
    ) -> Result<Vec<IpAddr>, DnsResolverError> {
        let (response, receiver) = mpsc::channel();
        self.worker
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .send(DnsWorkerRequest::LookupIp {
                hostname,
                name_servers,
                response,
            })
            .map_err(|_| DnsResolverError::lookup_failed("dns resolver worker stopped"))?;
        receiver
            .recv()
            .map_err(|_| DnsResolverError::lookup_failed("dns resolver worker stopped"))?
    }

    fn send_lookup_records(
        &self,
        hostname: String,
        name_servers: Vec<SocketAddr>,
        record_type: RecordType,
    ) -> Result<Vec<Record>, DnsResolverError> {
        let (response, receiver) = mpsc::channel();
        self.worker
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .send(DnsWorkerRequest::LookupRecords {
                hostname,
                name_servers,
                record_type,
                response,
            })
            .map_err(|_| DnsResolverError::lookup_failed("dns resolver worker stopped"))?;
        receiver
            .recv()
            .map_err(|_| DnsResolverError::lookup_failed("dns resolver worker stopped"))?
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl DnsResolver for HickoryDnsResolver {
    fn lookup_ip(&self, request: &DnsLookupRequest) -> Result<Vec<IpAddr>, DnsResolverError> {
        self.send_lookup_ip(
            request.hostname().to_owned(),
            request.name_servers().to_vec(),
        )
    }

    fn lookup_records(
        &self,
        request: &DnsRecordLookupRequest,
    ) -> Result<Vec<Record>, DnsResolverError> {
        self.send_lookup_records(
            request.hostname().to_owned(),
            request.name_servers().to_vec(),
            request.record_type(),
        )
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn run_dns_worker(receiver: mpsc::Receiver<DnsWorkerRequest>) {
    let runtime = match tokio::runtime::Runtime::new() {
        Ok(runtime) => runtime,
        Err(error) => {
            for request in receiver {
                request.respond_with_error(DnsResolverError::lookup_failed(format!(
                    "failed to create DNS runtime: {error}"
                )));
            }
            return;
        }
    };
    let mut resolvers = BTreeMap::new();
    for request in receiver {
        match request {
            DnsWorkerRequest::LookupIp {
                hostname,
                name_servers,
                response,
            } => {
                let result = worker_lookup_ip(&runtime, &mut resolvers, hostname, name_servers);
                let _ = response.send(result);
            }
            DnsWorkerRequest::LookupRecords {
                hostname,
                name_servers,
                record_type,
                response,
            } => {
                let result = worker_lookup_records(
                    &runtime,
                    &mut resolvers,
                    hostname,
                    name_servers,
                    record_type,
                );
                let _ = response.send(result);
            }
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl DnsWorkerRequest {
    fn respond_with_error(self, error: DnsResolverError) {
        match self {
            Self::LookupIp { response, .. } => {
                let _ = response.send(Err(error));
            }
            Self::LookupRecords { response, .. } => {
                let _ = response.send(Err(error));
            }
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn worker_resolver_for(
    resolvers: &mut BTreeMap<Vec<SocketAddr>, TokioResolver>,
    name_servers: &[SocketAddr],
) -> Result<TokioResolver, DnsResolverError> {
    let key = name_servers.to_vec();
    if let Some(resolver) = resolvers.get(&key).cloned() {
        return Ok(resolver);
    }

    let resolver_config = resolver_config_from_name_servers(name_servers);
    let builder = if let Some(config) = resolver_config {
        TokioResolver::builder_with_config(config, TokioRuntimeProvider::default())
    } else {
        TokioResolver::builder_tokio().map_err(|error| {
            DnsResolverError::lookup_failed(format!(
                "failed to initialize DNS resolver from system configuration: {error}"
            ))
        })?
    };
    let resolver = builder.build().map_err(|error| {
        DnsResolverError::lookup_failed(format!("failed to build DNS resolver: {error}"))
    })?;
    let resolver = resolvers.entry(key).or_insert_with(|| resolver).clone();
    Ok(resolver)
}

#[cfg(not(target_arch = "wasm32"))]
fn worker_lookup_ip(
    runtime: &tokio::runtime::Runtime,
    resolvers: &mut BTreeMap<Vec<SocketAddr>, TokioResolver>,
    hostname: String,
    name_servers: Vec<SocketAddr>,
) -> Result<Vec<IpAddr>, DnsResolverError> {
    let resolver = worker_resolver_for(resolvers, &name_servers)?;
    runtime.block_on(async move {
        let lookup = resolver.lookup_ip(&hostname).await.map_err(|error| {
            DnsResolverError::lookup_failed(format!(
                "failed to resolve DNS address {hostname}: {error}"
            ))
        })?;

        let mut addresses = Vec::new();
        let mut seen = BTreeSet::new();
        for ip in lookup.iter() {
            if seen.insert(ip) {
                addresses.push(ip);
            }
        }

        if addresses.is_empty() {
            return Err(DnsResolverError::lookup_failed(format!(
                "failed to resolve DNS address {hostname}"
            )));
        }

        Ok(addresses)
    })
}

#[cfg(not(target_arch = "wasm32"))]
fn worker_lookup_records(
    runtime: &tokio::runtime::Runtime,
    resolvers: &mut BTreeMap<Vec<SocketAddr>, TokioResolver>,
    hostname: String,
    name_servers: Vec<SocketAddr>,
    record_type: RecordType,
) -> Result<Vec<Record>, DnsResolverError> {
    let resolver = worker_resolver_for(resolvers, &name_servers)?;
    runtime.block_on(async move {
        let lookup = resolver
            .lookup(&hostname, record_type)
            .await
            .map_err(|error| {
                DnsResolverError::lookup_failed(format!(
                    "failed to resolve DNS {record_type} record {hostname}: {error}"
                ))
            })?;
        let records = lookup.answers().to_vec();
        if records.is_empty() {
            return Err(DnsResolverError::lookup_failed(format!(
                "failed to resolve DNS {record_type} record {hostname}"
            )));
        }
        Ok(records)
    })
}

#[cfg(target_arch = "wasm32")]
impl DnsResolver for HickoryDnsResolver {
    fn lookup_ip(&self, request: &DnsLookupRequest) -> Result<Vec<IpAddr>, DnsResolverError> {
        Err(DnsResolverError::lookup_failed(format!(
            "browser sidecar DNS resolver is unavailable for {}; configure DNS overrides or pass a literal address",
            request.hostname()
        )))
    }

    fn lookup_records(
        &self,
        request: &DnsRecordLookupRequest,
    ) -> Result<Vec<Record>, DnsResolverError> {
        Err(DnsResolverError::lookup_failed(format!(
            "browser sidecar DNS record resolver is unavailable for {}; configure DNS overrides or pass a literal address",
            request.hostname()
        )))
    }
}

pub fn normalize_dns_hostname(hostname: &str) -> Result<String, DnsResolverError> {
    let normalized = hostname.trim().trim_end_matches('.').to_ascii_lowercase();
    if normalized.is_empty() {
        return Err(DnsResolverError::invalid_input(
            "DNS hostname must not be empty",
        ));
    }
    Ok(normalized)
}

pub fn format_dns_resource(hostname: &str) -> Result<String, DnsResolverError> {
    Ok(format!("dns://{}", canonical_dns_subject(hostname)?))
}

pub fn resolve_dns(
    config: &DnsConfig,
    resolver: &dyn DnsResolver,
    hostname: &str,
) -> Result<DnsResolution, DnsResolverError> {
    let trimmed = hostname.trim();
    if let Ok(ip_addr) = trimmed.parse::<IpAddr>() {
        return Ok(DnsResolution::new(
            ip_addr.to_string(),
            DnsResolutionSource::Literal,
            vec![ip_addr],
        ));
    }

    let normalized_hostname = normalize_dns_hostname(trimmed)?;
    if let Some(addresses) = config.overrides.get(&normalized_hostname) {
        return Ok(DnsResolution::new(
            normalized_hostname,
            DnsResolutionSource::Override,
            addresses.clone(),
        ));
    }

    let request = DnsLookupRequest::new(normalized_hostname.clone(), config.name_servers.clone());
    let addresses = resolver.lookup_ip(&request)?;
    if addresses.is_empty() {
        return Err(DnsResolverError::lookup_failed(format!(
            "failed to resolve DNS address {normalized_hostname}"
        )));
    }

    Ok(DnsResolution::new(
        normalized_hostname,
        DnsResolutionSource::Resolver,
        dedupe_addresses(addresses),
    ))
}

pub fn resolve_dns_records(
    config: &DnsConfig,
    resolver: &dyn DnsResolver,
    hostname: &str,
    record_type: RecordType,
) -> Result<DnsRecordResolution, DnsResolverError> {
    let trimmed = hostname.trim();
    let normalized_hostname = normalize_dns_hostname(trimmed)?;
    let owner_name = normalized_hostname.parse::<Name>().map_err(|error| {
        DnsResolverError::invalid_input(format!("invalid DNS hostname: {error}"))
    })?;

    if let Some(records) = records_from_literal(trimmed, owner_name.clone(), record_type) {
        return Ok(DnsRecordResolution::new(
            normalized_hostname,
            DnsResolutionSource::Literal,
            records,
        ));
    }

    if let Some(addresses) = config.overrides.get(&normalized_hostname) {
        let records = records_from_addresses(owner_name.clone(), addresses, record_type);
        if !records.is_empty() {
            return Ok(DnsRecordResolution::new(
                normalized_hostname,
                DnsResolutionSource::Override,
                records,
            ));
        }
    }

    let request = DnsRecordLookupRequest::new(
        normalized_hostname.clone(),
        config.name_servers.clone(),
        record_type,
    );
    let records = resolver.lookup_records(&request)?;
    if records.is_empty() {
        return Err(DnsResolverError::lookup_failed(format!(
            "failed to resolve DNS {record_type} record {normalized_hostname}"
        )));
    }

    Ok(DnsRecordResolution::new(
        normalized_hostname,
        DnsResolutionSource::Resolver,
        records,
    ))
}

fn canonical_dns_subject(hostname: &str) -> Result<String, DnsResolverError> {
    let trimmed = hostname.trim();
    if let Ok(ip_addr) = trimmed.parse::<IpAddr>() {
        return Ok(ip_addr.to_string());
    }

    normalize_dns_hostname(trimmed)
}

#[cfg(not(target_arch = "wasm32"))]
fn resolver_config_from_name_servers(name_servers: &[SocketAddr]) -> Option<ResolverConfig> {
    if name_servers.is_empty() {
        return None;
    }

    let name_servers = name_servers
        .iter()
        .map(|server| {
            let mut config = NameServerConfig::udp_and_tcp(server.ip());
            for connection in &mut config.connections {
                connection.port = server.port();
                connection.bind_addr = Some(SocketAddr::new(
                    if server.is_ipv6() {
                        IpAddr::V6(Ipv6Addr::UNSPECIFIED)
                    } else {
                        IpAddr::V4(Ipv4Addr::UNSPECIFIED)
                    },
                    0,
                ));
            }
            config
        })
        .collect();

    Some(ResolverConfig::from_parts(None, vec![], name_servers))
}

fn dedupe_addresses(addresses: Vec<IpAddr>) -> Vec<IpAddr> {
    let mut deduped = Vec::with_capacity(addresses.len());
    let mut seen = BTreeSet::new();
    for address in addresses {
        if seen.insert(address) {
            deduped.push(address);
        }
    }
    deduped
}

fn records_from_literal(
    hostname: &str,
    owner_name: Name,
    record_type: RecordType,
) -> Option<Vec<Record>> {
    let ip_addr = hostname.parse::<IpAddr>().ok()?;
    let records = records_from_addresses(owner_name, &[ip_addr], record_type);
    if records.is_empty() {
        return None;
    }
    Some(records)
}

fn records_from_addresses(
    owner_name: Name,
    addresses: &[IpAddr],
    record_type: RecordType,
) -> Vec<Record> {
    addresses
        .iter()
        .filter_map(|ip| match (record_type, ip) {
            (RecordType::A, IpAddr::V4(ipv4)) | (RecordType::ANY, IpAddr::V4(ipv4)) => Some(
                Record::from_rdata(owner_name.clone(), 60, RData::A(A::from(*ipv4))),
            ),
            (RecordType::AAAA, IpAddr::V6(ipv6)) | (RecordType::ANY, IpAddr::V6(ipv6)) => Some(
                Record::from_rdata(owner_name.clone(), 60, RData::AAAA(AAAA::from(*ipv6))),
            ),
            _ => None,
        })
        .collect()
}
