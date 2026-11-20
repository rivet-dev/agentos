//! Guest ES module graphs evaluated into a V8 startup snapshot.
//!
//! A trusted client can name guest Node entrypoints (`jsRuntime.
//! snapshotEntrypoints`). For such an entry, the sidecar discovers the static
//! import graph of the entry, excluding the entry itself, and evaluates that
//! graph once in a pristine snapshot helper process. A guest `node <entry>`
//! process then boots from that snapshot: its imports are already evaluated
//! and only the entry module compiles and runs per process.
//!
//! Isolation and validity:
//! - Every non-builtin module must come from a read-only host mount (the
//!   session reader serves only those), and must be an ES module.
//! - The snapshot is keyed by the SHA-256 of the encoded graph (paths, sources,
//!   links), so equal content shares one snapshot and any change gets its own.
//! - Each process revalidates the graph against its VM before it uses it: the
//!   entry and every module source must hash the same and every import edge
//!   must resolve to the same path. When the host file identities that a full
//!   validation depended on are unchanged, a stat of each replaces it (see
//!   `validation_fingerprint`).
//! - Each isolate deserializes its own heap copy; no mutable V8 state crosses
//!   isolates.

use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};

use sha2::{Digest, Sha256};

use crate::execution::{GuestModuleReader, ResolvedModuleFormat};

/// Prefix of the `userland_code` value that names a registered module graph.
/// The rest is the hex graph digest. A NUL first byte keeps it apart from any
/// script bundle.
pub const MODULE_GRAPH_TOKEN_PREFIX: &str = "\0agentos-module-snapshot:v1:";
/// First bytes of an encoded graph in a snapshot helper request. `0xff` is not
/// valid UTF-8, so a script bundle can never start with it.
pub(crate) const MODULE_GRAPH_MAGIC: &[u8; 8] = b"\xffAOSMG01";

/// Graph bounds. Sources are also bounded by the userland snapshot code limit.
pub const MAX_MODULE_SNAPSHOT_GRAPH_MODULES: usize = 1024;
pub const MAX_MODULE_SNAPSHOT_GRAPH_SOURCE_BYTES: usize = 32 * 1024 * 1024;
/// Registered graphs per process. The oldest graph is dropped first; a
/// snapshot that is already built stays in the snapshot cache.
const MAX_REGISTERED_MODULE_GRAPHS: usize = 8;

pub const MODULE_SNAPSHOT_GRAPH_LIMIT_ERROR_CODE: &str = "ERR_AGENTOS_MODULE_SNAPSHOT_LIMIT";
pub const MODULE_SNAPSHOT_GRAPH_ERROR_CODE: &str = "ERR_AGENTOS_MODULE_SNAPSHOT_GRAPH";

// The bridge-policy installer reserves the first per-context snapshot slot.
const MODULE_PATHS_DATA_INDEX: usize =
    crate::guest_bridge_policy::SNAPSHOT_INSTALLER_DATA_INDEX + 1;

pub type ModuleGraphDigest = [u8; 32];

#[derive(Debug)]
pub struct SnapshotModule {
    /// Resolved guest path (`node:fs` for a builtin).
    pub path: String,
    /// Source exactly as the session compiles it.
    pub source: String,
    /// SHA-256 of the file as read, before shebang and prelude handling.
    pub raw_digest: ModuleGraphDigest,
    /// Static imports in request order: specifier and module index.
    pub requests: Vec<(String, usize)>,
}

#[derive(Debug)]
pub struct ModuleSnapshotGraph {
    pub entry: String,
    pub entry_digest: ModuleGraphDigest,
    /// The entry's static imports in request order. Snapshot creation
    /// evaluates them in this order, as the entry would.
    pub entry_requests: Vec<(String, usize)>,
    pub modules: Vec<SnapshotModule>,
    digest: ModuleGraphDigest,
    encoded_len: usize,
}

impl ModuleSnapshotGraph {
    fn new(
        entry: String,
        entry_digest: ModuleGraphDigest,
        entry_requests: Vec<(String, usize)>,
        modules: Vec<SnapshotModule>,
    ) -> Self {
        let mut graph = Self {
            entry,
            entry_digest,
            entry_requests,
            modules,
            digest: [0; 32],
            encoded_len: 0,
        };
        let encoded = graph.encode();
        graph.digest = Sha256::digest(&encoded).into();
        graph.encoded_len = encoded.len();
        graph
    }

    pub fn digest(&self) -> ModuleGraphDigest {
        self.digest
    }

    /// The `userland_code` value that selects this graph's snapshot.
    pub fn token(&self) -> String {
        let mut token = String::from(MODULE_GRAPH_TOKEN_PREFIX);
        for byte in self.digest {
            token.push_str(&format!("{byte:02x}"));
        }
        token
    }

    pub(crate) fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.encoded_len.max(64));
        out.extend_from_slice(MODULE_GRAPH_MAGIC);
        put_str(&mut out, &self.entry);
        out.extend_from_slice(&self.entry_digest);
        put_requests(&mut out, &self.entry_requests);
        put_u64(&mut out, self.modules.len() as u64);
        for module in &self.modules {
            put_str(&mut out, &module.path);
            put_str(&mut out, &module.source);
            out.extend_from_slice(&module.raw_digest);
            put_requests(&mut out, &module.requests);
        }
        out
    }

    pub(crate) fn decode(bytes: &[u8]) -> Result<Self, String> {
        let mut input = bytes
            .strip_prefix(MODULE_GRAPH_MAGIC)
            .ok_or_else(|| String::from("module graph payload missing magic"))?;
        let entry = take_str(&mut input)?;
        let entry_digest: ModuleGraphDigest = take_bytes(&mut input, 32)?
            .try_into()
            .map_err(|_| String::from("module graph entry digest truncated"))?;
        let entry_requests = take_requests(&mut input)?;
        let count = take_len(&mut input, MAX_MODULE_SNAPSHOT_GRAPH_MODULES)?;
        let mut modules = Vec::with_capacity(count);
        for _ in 0..count {
            let path = take_str(&mut input)?;
            let source = take_str(&mut input)?;
            let raw_digest: ModuleGraphDigest = take_bytes(&mut input, 32)?
                .try_into()
                .map_err(|_| String::from("module graph digest truncated"))?;
            modules.push(SnapshotModule {
                path,
                source,
                raw_digest,
                requests: take_requests(&mut input)?,
            });
        }
        if !input.is_empty() {
            return Err(format!(
                "module graph payload has {} trailing bytes",
                input.len()
            ));
        }
        let graph = Self::new(entry, entry_digest, entry_requests, modules);
        graph.check_links()?;
        Ok(graph)
    }

    fn check_links(&self) -> Result<(), String> {
        let count = self.modules.len();
        let in_range = |requests: &[(String, usize)]| requests.iter().all(|(_, i)| *i < count);
        if !in_range(&self.entry_requests)
            || !self.modules.iter().all(|module| in_range(&module.requests))
        {
            return Err(String::from("module graph link index out of range"));
        }
        Ok(())
    }
}

fn put_u64(out: &mut Vec<u8>, value: u64) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn put_str(out: &mut Vec<u8>, value: &str) {
    put_u64(out, value.len() as u64);
    out.extend_from_slice(value.as_bytes());
}

fn put_requests(out: &mut Vec<u8>, requests: &[(String, usize)]) {
    put_u64(out, requests.len() as u64);
    for (specifier, index) in requests {
        put_str(out, specifier);
        put_u64(out, *index as u64);
    }
}

fn take_bytes<'a>(input: &mut &'a [u8], len: usize) -> Result<&'a [u8], String> {
    if input.len() < len {
        return Err(String::from("module graph payload truncated"));
    }
    let (head, rest) = input.split_at(len);
    *input = rest;
    Ok(head)
}

fn take_u64(input: &mut &[u8]) -> Result<u64, String> {
    let bytes = take_bytes(input, 8)?;
    Ok(u64::from_le_bytes(bytes.try_into().expect("8 bytes")))
}

fn take_len(input: &mut &[u8], max: usize) -> Result<usize, String> {
    let len = usize::try_from(take_u64(input)?)
        .map_err(|_| String::from("module graph length overflows usize"))?;
    if len > max {
        return Err(format!("module graph length {len} exceeds {max}"));
    }
    Ok(len)
}

fn take_str(input: &mut &[u8]) -> Result<String, String> {
    let len = take_len(input, MAX_MODULE_SNAPSHOT_GRAPH_SOURCE_BYTES)?;
    String::from_utf8(take_bytes(input, len)?.to_vec())
        .map_err(|error| format!("module graph string is not UTF-8: {error}"))
}

fn take_requests(input: &mut &[u8]) -> Result<Vec<(String, usize)>, String> {
    let count = take_len(input, MAX_MODULE_SNAPSHOT_GRAPH_MODULES * 64)?;
    let mut requests = Vec::with_capacity(count);
    for _ in 0..count {
        let specifier = take_str(input)?;
        let index = usize::try_from(take_u64(input)?)
            .map_err(|_| String::from("module graph index overflows usize"))?;
        requests.push((specifier, index));
    }
    Ok(requests)
}

// --- Process-wide registry ---

#[derive(Default)]
struct Registry {
    graphs: VecDeque<Arc<ModuleSnapshotGraph>>,
}

fn registry() -> &'static Mutex<Registry> {
    static REGISTRY: Mutex<Registry> = Mutex::new(Registry {
        graphs: VecDeque::new(),
    });
    &REGISTRY
}

/// Register a discovered graph and return the token that selects its snapshot.
pub fn register_module_graph(graph: ModuleSnapshotGraph) -> Arc<ModuleSnapshotGraph> {
    let mut registry = registry().lock().expect("module graph registry poisoned");
    if let Some(existing) = registry
        .graphs
        .iter()
        .find(|existing| existing.digest == graph.digest)
    {
        return Arc::clone(existing);
    }
    // Entries for the same entry and entry source are superseded.
    registry.graphs.retain(|existing| {
        existing.entry != graph.entry || existing.entry_digest != graph.entry_digest
    });
    while registry.graphs.len() >= MAX_REGISTERED_MODULE_GRAPHS {
        registry.graphs.pop_front();
    }
    let graph = Arc::new(graph);
    registry.graphs.push_back(Arc::clone(&graph));
    graph
}

/// The registered graph for an entry path and entry source digest, if any.
pub fn module_graph_candidate(
    entry: &str,
    entry_digest: &ModuleGraphDigest,
) -> Option<Arc<ModuleSnapshotGraph>> {
    let registry = registry().lock().expect("module graph registry poisoned");
    registry
        .graphs
        .iter()
        .rev()
        .find(|graph| graph.entry == entry && &graph.entry_digest == entry_digest)
        .cloned()
}

pub fn is_module_graph_token(userland_code: &str) -> bool {
    userland_code.starts_with(MODULE_GRAPH_TOKEN_PREFIX)
}

pub(crate) fn module_graph_for_token(token: &str) -> Option<Arc<ModuleSnapshotGraph>> {
    let registry = registry().lock().expect("module graph registry poisoned");
    registry
        .graphs
        .iter()
        .find(|graph| graph.token() == token)
        .cloned()
}

pub fn source_digest(source: &str) -> ModuleGraphDigest {
    Sha256::digest(source.as_bytes()).into()
}

// --- Discovery and validation (sidecar side) ---

fn graph_error(message: impl std::fmt::Display) -> String {
    format!("{MODULE_SNAPSHOT_GRAPH_ERROR_CODE}: {message}")
}

/// The raw source of a graph module, after the format and mount checks that
/// discovery and validation share.
fn snapshot_module_raw_source(
    reader: &mut dyn GuestModuleReader,
    path: &str,
) -> Result<String, String> {
    if reader.module_format(path) != Some(ResolvedModuleFormat::Module) {
        return Err(graph_error(format!(
            "{path} is not an ES module with a fixed format (.mjs or a node: builtin)"
        )));
    }
    reader.read_module_source(path).ok_or_else(|| {
        graph_error(format!(
            "{path} is not readable from a read-only host mount"
        ))
    })
}

fn snapshot_module_source(
    reader: &mut dyn GuestModuleReader,
    path: &str,
) -> Result<(String, ModuleGraphDigest), String> {
    let raw = snapshot_module_raw_source(reader, path)?;
    let source = crate::execution::esm_module_source_for_snapshot(
        &raw,
        path,
        Some(ResolvedModuleFormat::Module),
    )
    .ok_or_else(|| graph_error(format!("{path} does not compile as an ES module")))?;
    Ok((source, source_digest(&raw)))
}

/// Whether `specifier` from `referrer` resolves to `target` by path algebra
/// alone: a builtin, or an explicit relative file path. Such an edge cannot
/// change while the target file exists with the same content.
fn edge_is_fixed(specifier: &str, referrer: &str, target: &str) -> bool {
    if target.starts_with("node:") {
        return target == specifier || target.strip_prefix("node:") == Some(specifier);
    }
    if !(specifier.starts_with("./") || specifier.starts_with("../")) {
        return false;
    }
    let Some(directory) = referrer.rsplit_once('/').map(|(directory, _)| directory) else {
        return false;
    };
    let mut parts: Vec<&str> = directory
        .split('/')
        .filter(|part| !part.is_empty())
        .collect();
    for part in specifier.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            part => parts.push(part),
        }
    }
    let joined = format!("/{}", parts.join("/"));
    joined == target
}

/// Discover the static import graph of `entry` (excluding the entry).
///
/// Compiles sources in a throwaway isolate to read their import requests; no
/// module code runs here.
pub fn discover_module_graph(
    entry: &str,
    entry_source: &str,
    reader: &mut dyn GuestModuleReader,
) -> Result<ModuleSnapshotGraph, String> {
    with_parse_scope(|scope| {
        let entry_specifiers = module_specifiers(scope, entry, entry_source)?;
        let mut modules: Vec<SnapshotModule> = Vec::new();
        let mut index_by_path: HashMap<String, usize> = HashMap::new();
        let mut pending: VecDeque<usize> = VecDeque::new();
        let mut total_bytes = 0usize;
        let mut link = |specifier: &str,
                        referrer: &str,
                        modules: &mut Vec<SnapshotModule>,
                        pending: &mut VecDeque<usize>|
         -> Result<usize, String> {
            let path = reader.resolve_module(specifier, referrer).ok_or_else(|| {
                graph_error(format!("cannot resolve {specifier:?} from {referrer}"))
            })?;
            if path == entry {
                return Err(graph_error(format!(
                    "{referrer} imports the entry {entry}; a snapshot entry must not be in its own graph"
                )));
            }
            if let Some(index) = index_by_path.get(&path) {
                return Ok(*index);
            }
            if modules.len() >= MAX_MODULE_SNAPSHOT_GRAPH_MODULES {
                return Err(format!(
                    "{MODULE_SNAPSHOT_GRAPH_LIMIT_ERROR_CODE}: graph exceeds {MAX_MODULE_SNAPSHOT_GRAPH_MODULES} modules"
                ));
            }
            let (source, raw_digest) = snapshot_module_source(reader, &path)?;
            total_bytes = total_bytes.saturating_add(source.len());
            if total_bytes > MAX_MODULE_SNAPSHOT_GRAPH_SOURCE_BYTES {
                return Err(format!(
                    "{MODULE_SNAPSHOT_GRAPH_LIMIT_ERROR_CODE}: graph sources exceed {MAX_MODULE_SNAPSHOT_GRAPH_SOURCE_BYTES} bytes"
                ));
            }
            let index = modules.len();
            index_by_path.insert(path.clone(), index);
            modules.push(SnapshotModule {
                path,
                source,
                raw_digest,
                requests: Vec::new(),
            });
            pending.push_back(index);
            Ok(index)
        };

        let mut entry_requests = Vec::with_capacity(entry_specifiers.len());
        for specifier in entry_specifiers {
            let index = link(&specifier, entry, &mut modules, &mut pending)?;
            entry_requests.push((specifier, index));
        }
        while let Some(index) = pending.pop_front() {
            let path = modules[index].path.clone();
            let specifiers = module_specifiers(scope, &path, &modules[index].source)?;
            let mut requests = Vec::with_capacity(specifiers.len());
            for specifier in specifiers {
                let target = link(&specifier, &path, &mut modules, &mut pending)?;
                requests.push((specifier, target));
            }
            modules[index].requests = requests;
        }
        if entry_requests.is_empty() {
            return Err(graph_error(format!("{entry} has no static imports")));
        }
        Ok(ModuleSnapshotGraph::new(
            entry.to_owned(),
            source_digest(entry_source),
            entry_requests,
            modules,
        ))
    })
}

/// Check that `graph` still describes what this VM would load for `entry`:
/// the same entry source, the same module sources, and the same links.
pub fn validate_module_graph(
    graph: &ModuleSnapshotGraph,
    entry_source: &str,
    reader: &mut dyn GuestModuleReader,
) -> Result<(), String> {
    if source_digest(entry_source) != graph.entry_digest {
        return Err(graph_error(format!("{} changed", graph.entry)));
    }
    // Stat what the last full validation depended on; skip it if none changed.
    let fingerprint = validation_fingerprint(graph, reader);
    if fingerprint
        .as_ref()
        .is_some_and(|fingerprint| validated_fingerprint_matches(graph, fingerprint))
    {
        return Ok(());
    }
    let resolves_to = |reader: &mut dyn GuestModuleReader,
                       specifier: &str,
                       referrer: &str,
                       index: usize|
     -> Result<(), String> {
        let path = reader.resolve_module(specifier, referrer);
        if path.as_deref() != Some(graph.modules[index].path.as_str()) {
            return Err(graph_error(format!(
                "{specifier:?} from {referrer} no longer resolves to {}",
                graph.modules[index].path
            )));
        }
        Ok(())
    };
    for (specifier, index) in &graph.entry_requests {
        resolves_to(reader, specifier, &graph.entry, *index)?;
    }
    let mut verified = Vec::new();
    for module in &graph.modules {
        // Builtin wrappers are generated by this binary.
        if !module.path.starts_with("node:") {
            let identity = reader.source_identity(&module.path);
            let known = identity
                .as_ref()
                .is_some_and(|identity| verified_source_known(graph, identity));
            if !known {
                let raw = snapshot_module_raw_source(reader, &module.path)?;
                if source_digest(&raw) != module.raw_digest {
                    return Err(graph_error(format!("{} changed", module.path)));
                }
                verified.extend(identity);
            }
        }
        for (specifier, index) in &module.requests {
            resolves_to(reader, specifier, &module.path, *index)?;
        }
    }
    remember_verified_sources(graph, verified);
    if let Some(fingerprint) = fingerprint {
        remember_validated_fingerprint(graph, fingerprint);
    }
    Ok(())
}

/// Validated graphs remembered with their fingerprints.
const MAX_VALIDATED_GRAPH_FINGERPRINTS: usize = 64;

/// A digest of host file identities that a full validation of `graph`
/// depends on, or `None` when the graph's inputs are not all covered.
///
/// Covered inputs: every module file, and for the entry and every module the
/// chain of ancestor directories that lie in host mounts, each with its
/// `node_modules` and `package.json` entries. A directory's identity changes
/// when an entry is added, removed, or renamed in it, so a new candidate that
/// resolution would prefer (a closer `node_modules`, a package, an extension or
/// index file) changes the digest, and so does an edited `package.json`. An
/// edge that resolves outside its referrer's host-mounted chain could be
/// shadowed by a path no identity covers, so such a graph is never cached.
fn validation_fingerprint(
    graph: &ModuleSnapshotGraph,
    reader: &mut dyn GuestModuleReader,
) -> Option<ModuleGraphDigest> {
    let mut hasher = Sha256::new();
    let mut directories = std::collections::BTreeSet::new();
    // The highest host-mounted ancestor of each referrer.
    let mut chain_tops = HashMap::new();
    let referrers = std::iter::once(graph.entry.as_str()).chain(
        graph
            .modules
            .iter()
            .map(|module| module.path.as_str())
            .filter(|path| !path.starts_with("node:")),
    );
    for referrer in referrers {
        let mut top = None;
        let mut directory = referrer;
        while let Some((parent, _)) = directory.rsplit_once('/') {
            let parent = if parent.is_empty() { "/" } else { parent };
            if !directories.contains(parent) {
                if reader.source_identity(parent).is_none() {
                    break;
                }
                directories.insert(parent.to_owned());
            }
            top = Some(parent.to_owned());
            if parent == "/" {
                break;
            }
            directory = parent;
        }
        chain_tops.insert(referrer.to_owned(), top?);
    }
    let edges = std::iter::once((graph.entry.as_str(), &graph.entry_requests)).chain(
        graph
            .modules
            .iter()
            .map(|module| (module.path.as_str(), &module.requests)),
    );
    for (referrer, requests) in edges {
        for (specifier, index) in requests {
            let target = graph.modules[*index].path.as_str();
            if edge_is_fixed(specifier, referrer, target) {
                continue;
            }
            let top = chain_tops.get(referrer)?;
            let covered = top == "/"
                || target
                    .strip_prefix(top.as_str())
                    .is_some_and(|rest| rest.starts_with('/'));
            if !covered {
                return None;
            }
        }
    }
    for module in &graph.modules {
        if module.path.starts_with("node:") {
            continue;
        }
        hasher.update(module.path.as_bytes());
        hasher.update(reader.source_identity(&module.path)?.as_bytes());
    }
    for directory in &directories {
        hasher.update(directory.as_bytes());
        for name in ["", "node_modules", "package.json"] {
            let path = match (name, directory.as_str()) {
                ("", _) => directory.clone(),
                (name, "/") => format!("/{name}"),
                (name, directory) => format!("{directory}/{name}"),
            };
            hasher.update([0]);
            hasher.update(reader.source_identity(&path).unwrap_or_default().as_bytes());
        }
    }
    Some(hasher.finalize().into())
}

fn validated_fingerprints() -> &'static Mutex<HashMap<ModuleGraphDigest, ModuleGraphDigest>> {
    static VALIDATED: std::sync::OnceLock<Mutex<HashMap<ModuleGraphDigest, ModuleGraphDigest>>> =
        std::sync::OnceLock::new();
    VALIDATED.get_or_init(|| Mutex::new(HashMap::new()))
}

fn validated_fingerprint_matches(
    graph: &ModuleSnapshotGraph,
    fingerprint: &ModuleGraphDigest,
) -> bool {
    validated_fingerprints()
        .lock()
        .expect("validated module graph fingerprints poisoned")
        .get(&graph.digest)
        == Some(fingerprint)
}

fn remember_validated_fingerprint(graph: &ModuleSnapshotGraph, fingerprint: ModuleGraphDigest) {
    let mut validated = validated_fingerprints()
        .lock()
        .expect("validated module graph fingerprints poisoned");
    if validated.len() >= MAX_VALIDATED_GRAPH_FINGERPRINTS && !validated.contains_key(&graph.digest)
    {
        validated.clear();
    }
    validated.insert(graph.digest, fingerprint);
}

/// Host file identities whose content already matched a graph, so a later
/// validation can stat a file instead of reading and hashing it.
const MAX_VERIFIED_SOURCE_IDENTITIES: usize = 16 * 1024;

fn verified_sources(
) -> &'static Mutex<HashMap<ModuleGraphDigest, std::collections::HashSet<String>>> {
    static VERIFIED: std::sync::OnceLock<
        Mutex<HashMap<ModuleGraphDigest, std::collections::HashSet<String>>>,
    > = std::sync::OnceLock::new();
    VERIFIED.get_or_init(|| Mutex::new(HashMap::new()))
}

fn verified_source_known(graph: &ModuleSnapshotGraph, identity: &str) -> bool {
    verified_sources()
        .lock()
        .expect("verified module sources poisoned")
        .get(&graph.digest)
        .is_some_and(|identities| identities.contains(identity))
}

fn remember_verified_sources(graph: &ModuleSnapshotGraph, identities: Vec<String>) {
    if identities.is_empty() {
        return;
    }
    let mut verified = verified_sources()
        .lock()
        .expect("verified module sources poisoned");
    let total: usize = verified.values().map(|set| set.len()).sum();
    if total + identities.len() > MAX_VERIFIED_SOURCE_IDENTITIES {
        verified.clear();
    }
    verified.entry(graph.digest).or_default().extend(identities);
}

fn with_parse_scope<T>(
    f: impl FnOnce(&mut v8::HandleScope) -> Result<T, String>,
) -> Result<T, String> {
    crate::isolate::init_v8_platform();
    crate::isolate::prepare_current_thread();
    let mut isolate = Some(crate::isolate::create_isolate(None));
    let result = {
        let isolate = isolate.as_mut().expect("parse isolate");
        let scope = &mut v8::HandleScope::new(isolate);
        let context = v8::Context::new(scope, Default::default());
        let scope = &mut v8::ContextScope::new(scope, context);
        f(scope)
    };
    drop(isolate.take());
    result
}

/// Compile one graph module. Only the snapshot creator (`for_snapshot`) compiles
/// from a shared source: that source must be in the creator's external
/// reference table, and the helper process that holds it exits after the
/// build. Discovery runs in the sidecar, so it compiles from a heap copy that
/// its parse isolate frees, and lazily: it only reads import requests.
fn compile_snapshot_module<'s>(
    scope: &mut v8::HandleScope<'s>,
    path: &str,
    source: &str,
    for_snapshot: bool,
) -> Result<v8::Local<'s, v8::Module>, String> {
    let tc = &mut v8::TryCatch::new(scope);
    let v8_source = if for_snapshot {
        crate::shared_source::snapshot_source_string(tc, source)
            .map_err(|error| graph_error(format!("{path}: {error}")))?
    } else {
        v8::String::new(tc, source)
            .ok_or_else(|| graph_error(format!("{path} is too large for V8")))?
    };
    let resource = v8::String::new(tc, path)
        .ok_or_else(|| graph_error(format!("{path} is too large for V8")))?;
    let origin = v8::ScriptOrigin::new(
        tc,
        resource.into(),
        0,
        0,
        false,
        -1,
        None,
        false,
        false,
        true,
        None,
    );
    let mut compiled = v8::script_compiler::Source::new(v8_source, Some(&origin));
    // Eager: the snapshot keeps every function's bytecode, so a booted entrypoint runs its
    // first request without lazy compiles. The cost is a larger blob, paid at isolate boot.
    let options = if for_snapshot {
        v8::script_compiler::CompileOptions::EagerCompile
    } else {
        v8::script_compiler::CompileOptions::NoCompileOptions
    };
    match v8::script_compiler::compile_module2(
        tc,
        &mut compiled,
        options,
        v8::script_compiler::NoCacheReason::NoReason,
    ) {
        Some(module) => Ok(v8::Local::new(tc, module)),
        None => {
            let message = tc
                .exception()
                .map(|exception| exception.to_rust_string_lossy(tc))
                .unwrap_or_else(|| String::from("compile failed"));
            Err(graph_error(format!("{path}: {message}")))
        }
    }
}

fn module_specifiers(
    scope: &mut v8::HandleScope,
    path: &str,
    source: &str,
) -> Result<Vec<String>, String> {
    // Each module's source and record become garbage once its requests are read.
    let scope = &mut v8::HandleScope::new(scope);
    let module = compile_snapshot_module(scope, path, source, false)?;
    let requests = module.get_module_requests();
    let mut specifiers = Vec::with_capacity(requests.length());
    for index in 0..requests.length() {
        let request: v8::Local<v8::ModuleRequest> =
            requests.get(scope, index).expect("module request").cast();
        if request.get_import_attributes().length() > 0 {
            return Err(graph_error(format!(
                "{path} uses import attributes, which module snapshots do not support"
            )));
        }
        specifiers.push(request.get_specifier().to_rust_string_lossy(scope));
    }
    Ok(specifiers)
}

// --- Snapshot creation (helper process) ---

struct BuildState {
    modules: Vec<v8::Global<v8::Module>>,
    requests: Vec<Vec<(String, usize)>>,
    paths: Vec<String>,
}

thread_local! {
    static BUILD_STATE: RefCell<Option<BuildState>> = const { RefCell::new(None) };
}

fn build_resolve_callback<'a>(
    context: v8::Local<'a, v8::Context>,
    specifier: v8::Local<'a, v8::String>,
    _import_attributes: v8::Local<'a, v8::FixedArray>,
    referrer: v8::Local<'a, v8::Module>,
) -> Option<v8::Local<'a, v8::Module>> {
    let scope = &mut unsafe { v8::CallbackScope::new(context) };
    let specifier = specifier.to_rust_string_lossy(scope);
    let target = BUILD_STATE.with(|cell| {
        let state = cell.borrow();
        let state = state.as_ref()?;
        let referrer_index = state
            .modules
            .iter()
            .position(|module| v8::Local::new(scope, module) == referrer)?;
        let (_, target) = state.requests[referrer_index]
            .iter()
            .find(|(candidate, _)| *candidate == specifier)?;
        Some(state.modules[*target].clone())
    });
    match target {
        Some(module) => Some(v8::Local::new(scope, module)),
        None => {
            let message = v8::String::new(
                scope,
                &format!("module snapshot graph has no link for {specifier:?}"),
            )
            .unwrap();
            let exception = v8::Exception::error(scope, message);
            scope.throw_exception(exception);
            None
        }
    }
}

extern "C" fn build_import_meta_callback(
    context: v8::Local<v8::Context>,
    module: v8::Local<v8::Module>,
    meta: v8::Local<v8::Object>,
) {
    let scope = &mut unsafe { v8::CallbackScope::new(context) };
    let path = BUILD_STATE.with(|cell| {
        let state = cell.borrow();
        let state = state.as_ref()?;
        let index = state
            .modules
            .iter()
            .position(|candidate| v8::Local::new(scope, candidate) == module)?;
        Some(state.paths[index].clone())
    });
    if let Some(path) = path {
        crate::execution::set_import_meta_fields(scope, meta, &path);
    }
}

fn build_dynamic_import_callback<'a>(
    scope: &mut v8::HandleScope<'a>,
    _host_defined_options: v8::Local<'a, v8::Data>,
    _resource_name: v8::Local<'a, v8::Value>,
    specifier: v8::Local<'a, v8::String>,
    _import_attributes: v8::Local<'a, v8::FixedArray>,
) -> Option<v8::Local<'a, v8::Promise>> {
    let specifier = specifier.to_rust_string_lossy(scope);
    let message = v8::String::new(
        scope,
        &format!("import({specifier:?}) cannot run while a module snapshot is built"),
    )?;
    let error = v8::Exception::error(scope, message);
    let resolver = v8::PromiseResolver::new(scope)?;
    resolver.reject(scope, error);
    Some(resolver.get_promise(scope))
}

/// Guards installed while the graph evaluates. The script returns the function
/// that removes them, which the build calls once evaluation ends.
///
/// - Time and randomness (`Math.random`, `Date.now`, `Date()`, `new Date()`,
///   `performance.now`, `process.hrtime`, `process.uptime`) throw. A value read
///   at module top level would be frozen into the snapshot and shared by every
///   process, in every VM, that boots from it. The build then fails and those
///   processes load the graph normally. Crypto randomness needs the host
///   bridge, which is a stub here, so it throws already.
/// - V8 creates a snapshot context without `WebAssembly` and installs it when
///   it restores the context. A top-level read of `WebAssembly` throws instead
///   of seeing `undefined` (which feature detection would bake in), and the
///   guard is deleted again so the restored context gets V8's own.
///
/// A guarded function that the graph captured keeps working after the build:
/// it calls the original once the guards are removed. `Error` must not be
/// touched: V8 also adds `Error.stackTraceLimit` on restore.
const SNAPSHOT_BUILD_GUARDS: &str = r#"(() => {
    let building = true;
    const unavailable = (what) => new Error(
        "ERR_AGENTOS_MODULE_SNAPSHOT_GRAPH: " + what + " is not available while a module snapshot is built: " +
        "its value would be shared by every process that boots from the snapshot; call it inside a function, not at module top level"
    );
    const undo = [];
    const replace = (owner, key, descriptor) => {
        const previous = Object.getOwnPropertyDescriptor(owner, key);
        Object.defineProperty(owner, key, descriptor);
        undo.push(() => previous ? Object.defineProperty(owner, key, previous) : delete owner[key]);
    };
    const guardMethod = (object, key, label) => {
        let owner = object;
        while (owner && !Object.prototype.hasOwnProperty.call(owner, key)) {
            owner = Object.getPrototypeOf(owner);
        }
        if (!owner) return;
        const descriptor = Object.getOwnPropertyDescriptor(owner, key);
        const original = descriptor.value;
        if (typeof original !== "function" || !descriptor.configurable) return;
        const guarded = { [key](...args) {
            if (building) throw unavailable(label);
            return Reflect.apply(original, this, args);
        } }[key];
        for (const name of Reflect.ownKeys(original)) {
            if (name !== "length" && name !== "name" && name !== "prototype") {
                Object.defineProperty(guarded, name, Object.getOwnPropertyDescriptor(original, name));
            }
        }
        replace(owner, key, { ...descriptor, value: guarded });
    };

    guardMethod(Math, "random", "Math.random()");
    guardMethod(Date, "now", "Date.now()");
    // A `performance` getter that throws here keeps the clock out of the
    // snapshot by itself, so there is nothing to guard then.
    let perf;
    try { perf = globalThis.performance; } catch { perf = undefined; }
    if (typeof perf === "object" && perf) {
        guardMethod(perf, "now", "performance.now()");
    }
    if (typeof process === "object" && process) {
        if (typeof process.hrtime === "function") {
            guardMethod(process.hrtime, "bigint", "process.hrtime.bigint()");
        }
        guardMethod(process, "hrtime", "process.hrtime()");
        guardMethod(process, "uptime", "process.uptime()");
    }
    const OriginalDate = Date;
    const GuardedDate = new Proxy(OriginalDate, {
        apply(target, receiver, args) {
            if (building) throw unavailable("Date()");
            return Reflect.apply(target, receiver, args);
        },
        construct(target, args, newTarget) {
            if (building && args.length === 0) throw unavailable("new Date()");
            return Reflect.construct(target, args, newTarget);
        },
    });
    replace(globalThis, "Date", {
        ...Object.getOwnPropertyDescriptor(globalThis, "Date"),
        value: GuardedDate,
    });
    Object.defineProperty(globalThis, "WebAssembly", {
        configurable: true,
        enumerable: false,
        get() {
            throw new Error(
                "ERR_AGENTOS_MODULE_SNAPSHOT_GRAPH: WebAssembly is not available while a module snapshot is built; read it inside a function, not at module top level"
            );
        },
    });

    return () => {
        building = false;
        while (undo.length > 0) undo.pop()();
        delete globalThis.WebAssembly;
    };
})()"#;

/// Install [`SNAPSHOT_BUILD_GUARDS`] and return the function that removes them.
fn install_build_guards(scope: &mut v8::HandleScope) -> Result<v8::Global<v8::Function>, String> {
    let tc = &mut v8::TryCatch::new(scope);
    let source = v8::String::new(tc, SNAPSHOT_BUILD_GUARDS).expect("static script");
    let remove = v8::Script::compile(tc, source, None)
        .and_then(|script| script.run(tc))
        .and_then(|value| v8::Local::<v8::Function>::try_from(value).ok());
    match remove {
        Some(remove) => Ok(v8::Global::new(tc, remove)),
        None => {
            let message = exception_text(tc, "did not return its teardown");
            Err(graph_error(format!("module snapshot setup: {message}")))
        }
    }
}

fn remove_build_guards(
    scope: &mut v8::HandleScope,
    remove: v8::Global<v8::Function>,
) -> Result<(), String> {
    let tc = &mut v8::TryCatch::new(scope);
    let remove = v8::Local::new(tc, remove);
    let receiver = v8::undefined(tc).into();
    if remove.call(tc, receiver, &[]).is_none() {
        let message = exception_text(tc, "failed");
        return Err(graph_error(format!("module snapshot teardown: {message}")));
    }
    Ok(())
}

/// Compile, link, and evaluate `graph` in the snapshot creator context, then
/// attach the evaluated modules as context data for restore.
pub(crate) fn evaluate_module_graph_for_snapshot(
    scope: &mut v8::HandleScope,
    graph: &ModuleSnapshotGraph,
) -> Result<(), String> {
    scope.set_host_initialize_import_meta_object_callback(build_import_meta_callback);
    scope.set_host_import_module_dynamically_callback(build_dynamic_import_callback);

    let mut modules = Vec::with_capacity(graph.modules.len());
    for module in &graph.modules {
        let compiled = compile_snapshot_module(scope, &module.path, &module.source, true)?;
        modules.push(v8::Global::new(scope, compiled));
    }
    BUILD_STATE.with(|cell| {
        *cell.borrow_mut() = Some(BuildState {
            modules: modules.clone(),
            requests: graph.modules.iter().map(|m| m.requests.clone()).collect(),
            paths: graph.modules.iter().map(|m| m.path.clone()).collect(),
        });
    });
    let (result, teardown) = match install_build_guards(scope) {
        Ok(remove) => (
            evaluate_roots(scope, graph, &modules),
            remove_build_guards(scope, remove),
        ),
        Err(error) => (Err(error), Ok(())),
    };
    // No Global may stay alive into `create_blob`.
    BUILD_STATE.with(|cell| *cell.borrow_mut() = None);
    result.and(teardown)?;

    // The module paths are followed by the module objects in the same order.
    let paths: Vec<v8::Local<v8::Value>> = graph
        .modules
        .iter()
        .map(|module| v8::String::new(scope, &module.path).unwrap().into())
        .collect();
    let paths = v8::Array::new_with_elements(scope, &paths);
    let context = scope.get_current_context();
    if scope.add_context_data(context, paths) != MODULE_PATHS_DATA_INDEX {
        return Err(graph_error("unexpected snapshot module path data index"));
    }
    for (index, module) in modules.iter().enumerate() {
        let module = v8::Local::new(scope, module);
        if scope.add_context_data(context, module) != MODULE_PATHS_DATA_INDEX + 1 + index {
            return Err(graph_error(
                "snapshot context data indices are not contiguous",
            ));
        }
    }
    Ok(())
}

fn evaluate_roots(
    scope: &mut v8::HandleScope,
    graph: &ModuleSnapshotGraph,
    modules: &[v8::Global<v8::Module>],
) -> Result<(), String> {
    let tc = &mut v8::TryCatch::new(scope);
    for (specifier, index) in &graph.entry_requests {
        let module = v8::Local::new(tc, &modules[*index]);
        let path = &graph.modules[*index].path;
        if module.get_status() == v8::ModuleStatus::Uninstantiated
            && module
                .instantiate_module(tc, build_resolve_callback)
                .is_none()
        {
            let message = exception_text(tc, "link failed");
            return Err(graph_error(format!("linking {path}: {message}")));
        }
        let Some(result) = module.evaluate(tc) else {
            let message = exception_text(tc, "evaluation failed");
            return Err(graph_error(format!("evaluating {path}: {message}")));
        };
        tc.perform_microtask_checkpoint();
        if let Ok(promise) = v8::Local::<v8::Promise>::try_from(result) {
            match promise.state() {
                v8::PromiseState::Fulfilled => {}
                v8::PromiseState::Rejected => {
                    let reason = promise.result(tc);
                    let reason = value_text(tc, reason);
                    return Err(graph_error(format!("evaluating {path}: {reason}")));
                }
                v8::PromiseState::Pending => {
                    return Err(graph_error(format!(
                        "{path} (imported as {specifier:?}) awaits work that cannot finish while a snapshot is built"
                    )));
                }
            }
        }
        if module.get_status() != v8::ModuleStatus::Evaluated {
            return Err(graph_error(format!("{path} did not finish evaluating")));
        }
    }
    let pending_timers = pending_timer_count(tc)?;
    if pending_timers > 0 {
        return Err(graph_error(format!(
            "the graph left {pending_timers} timer(s) pending at module top level"
        )));
    }
    Ok(())
}

fn exception_text(tc: &mut v8::TryCatch<v8::HandleScope>, fallback: &str) -> String {
    match tc.exception() {
        Some(exception) => value_text(tc, exception),
        None => fallback.to_owned(),
    }
}

/// An exception with the module and line it came from.
fn value_text(scope: &mut v8::HandleScope, value: v8::Local<v8::Value>) -> String {
    if let Ok(object) = v8::Local::<v8::Object>::try_from(value) {
        let key = v8::String::new(scope, "stack").unwrap();
        if let Some(stack) = object
            .get(scope, key.into())
            .filter(|stack| stack.is_string())
        {
            let stack = stack.to_rust_string_lossy(scope);
            if stack.contains("\n") {
                return stack;
            }
        }
    }
    let text = value.to_rust_string_lossy(scope);
    let message = v8::Exception::create_message(scope, value);
    let location = message
        .get_script_resource_name(scope)
        .map(|name| name.to_rust_string_lossy(scope))
        .unwrap_or_default();
    let line = message.get_line_number(scope).unwrap_or(0);
    let source_line = message
        .get_source_line(scope)
        .map(|line| line.to_rust_string_lossy(scope))
        .unwrap_or_default();
    let source_line: String = source_line.trim().chars().take(160).collect();
    format!("{text} (at {location}:{line}: {source_line})")
}

/// Timers the graph left pending at module top level. A snapshot cannot carry
/// a pending timer, so a count that cannot be read fails the snapshot instead
/// of passing it.
fn pending_timer_count(scope: &mut v8::HandleScope) -> Result<u32, String> {
    let context = scope.get_current_context();
    let global = context.global(scope);
    let key = v8::String::new(scope, "_getPendingTimerCount")
        .ok_or_else(|| graph_error(String::from("cannot allocate the timer probe name")))?;
    let function = global
        .get(scope, key.into())
        .and_then(|value| v8::Local::<v8::Function>::try_from(value).ok())
        .ok_or_else(|| {
            graph_error(String::from(
                "the bridge has no _getPendingTimerCount, so pending timers cannot be checked",
            ))
        })?;
    let receiver = v8::undefined(scope).into();
    function
        .call(scope, receiver, &[])
        .and_then(|value| value.uint32_value(scope))
        .ok_or_else(|| {
            graph_error(String::from(
                "_getPendingTimerCount did not return a timer count",
            ))
        })
}

// --- Restore (session thread) ---

/// Take the evaluated graph modules out of a context restored from a module
/// snapshot. Returns `None` for any other context. V8 hands context data out
/// once, so call this once per context.
pub(crate) fn take_snapshot_modules(
    scope: &mut v8::HandleScope,
) -> Option<Vec<(String, v8::Global<v8::Module>)>> {
    let paths = scope
        .get_context_data_from_snapshot_once::<v8::Array>(MODULE_PATHS_DATA_INDEX)
        .ok()?;
    let mut modules = Vec::with_capacity(paths.length() as usize);
    for index in 0..paths.length() {
        let path = paths.get_index(scope, index)?.to_rust_string_lossy(scope);
        let module = scope
            .get_context_data_from_snapshot_once::<v8::Module>(
                MODULE_PATHS_DATA_INDEX + 1 + index as usize,
            )
            .ok()?;
        modules.push((path, v8::Global::new(scope, module)));
    }
    Some(modules)
}
