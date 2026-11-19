//! Process-wide V8 code cache for guest ES modules from trusted host bundles.
//!
//! Every new guest Node process compiles its modules in a fresh isolate. When
//! many VMs load the same host bundle, that parse/compile work is identical.
//! This cache keeps the bytes V8 produces from
//! `UnboundModuleScript::create_code_cache` and hands them back to
//! `CompileOptions::ConsumeCodeCache` for the next compile of the same source.
//!
//! Isolation: only sources read from read-only host mounts or packed packages
//! (`ModuleSourceOrigin::ReadOnlyHostMount`) are cached. A guest-writable
//! source never enters the cache, so a VM cannot learn from compile timing
//! whether another VM loaded the same guest code. An entry is keyed by the
//! SHA-256 of the exact source text that is compiled, so a module only ever
//! receives bytecode V8 produced from identical source. Guests cannot write
//! entries; only V8 serializes them. V8 checks each entry's header on consume
//! (version, flags, source length) and a rejected entry is dropped and the
//! module compiles normally. No V8 heap object or mutable state crosses
//! isolates, only immutable bytes. The cache lives in memory only and is never
//! persisted.
//!
//! V8 does not verify the bytecode inside an entry, so an entry must never be
//! serialized from an isolate whose heap guest code may have changed. Entries
//! are recorded only before any guest code has run in the isolate: a miss is
//! compiled eagerly (every function, not only the top level) and serialized at
//! once. After the first guest code runs, the isolate only consumes entries.
//! The execution layer marks that point with [`guest_code_started`], and the
//! session thread clears it with [`isolate_started`] for each fresh isolate.
//!
//! A miss in an isolate where guest code has already run (for example behind a
//! `node -e` launcher) is recorded by a pristine helper instead: the session
//! hands the source text, never a path, to its VM's bounded blocking executor,
//! and a worker compiles it in a new isolate that never runs any code. That
//! isolate is restored from the same startup snapshot as the session's, because
//! V8 rejects a code cache produced under a different read-only snapshot. Only
//! `ReadOnlyHostMount` sources reach the helper; the session reader marks a
//! source that way only when it read those exact bytes from a read-only
//! `host_dir`/`module_access` mount or a packed package the client configured.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use agentos_runtime::BlockingExecutor;

use sha2::{Digest, Sha256};

/// Upper bound on cached bytes across all entries. The oldest entries are
/// evicted first when an insert would exceed it.
pub const MAX_MODULE_CODE_CACHE_BYTES: usize = 64 * 1024 * 1024;
/// Small modules compile in microseconds; hashing and caching them costs more
/// than it saves.
pub const MIN_CACHED_MODULE_SOURCE_BYTES: usize = 2 * 1024;

/// Pristine-helper record jobs in flight at once, process-wide. A miss while
/// the helper is full compiles normally and is recorded by a later miss.
pub const MAX_PRISTINE_RECORD_JOBS: usize = 2;
/// A record job that has not started within this deadline is dropped with
/// `ERR_AGENTOS_MODULE_CODE_CACHE_RECORD_TIMEOUT`.
pub const PRISTINE_RECORD_DEADLINE: Duration = Duration::from_secs(5);

pub type ModuleSourceKey = [u8; 32];

/// Where a module's source came from. Only `ReadOnlyHostMount` sources are
/// shared across VMs through the cache.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModuleSourceOrigin {
    /// A read-only host mount or packed package: the same trusted bytes for
    /// every VM that mounts it.
    ReadOnlyHostMount,
    /// Anything a guest may have written or chosen, including inline code and
    /// sources fetched over the bridge.
    Guest,
}

thread_local! {
    /// Whether guest code has run in the isolate this thread is using. Set
    /// before the first guest evaluation; cleared only for a fresh isolate.
    static GUEST_CODE_RAN: Cell<bool> = const { Cell::new(false) };
    /// The blocking executor of the VM this session thread serves; record
    /// jobs are charged to it.
    static SESSION_EXECUTOR: RefCell<Option<BlockingExecutor>> = const { RefCell::new(None) };
    /// Builds a never-run isolate from the same startup snapshot as the
    /// session's isolate, for pristine-helper record jobs.
    static SESSION_ISOLATE_FACTORY: RefCell<Option<PristineIsolateFactory>> =
        const { RefCell::new(None) };
}

/// Creates an isolate that matches the session isolate's startup snapshot.
pub type PristineIsolateFactory = Arc<dyn Fn() -> v8::OwnedIsolate + Send + Sync>;

/// Set (or clear) how record jobs from this session thread build their
/// isolate. Call whenever the session installs an isolate.
pub fn set_session_isolate_factory(factory: Option<PristineIsolateFactory>) {
    SESSION_ISOLATE_FACTORY.with(|cell| *cell.borrow_mut() = factory);
}

/// Set (or clear) the blocking executor pristine-helper record jobs from this
/// session thread run on.
pub fn set_session_executor(executor: Option<BlockingExecutor>) {
    SESSION_EXECUTOR.with(|cell| *cell.borrow_mut() = executor);
}

/// Keys with a record job queued or running, so each source is recorded once.
fn pristine_record_in_flight() -> &'static Mutex<HashSet<ModuleSourceKey>> {
    static IN_FLIGHT: OnceLock<Mutex<HashSet<ModuleSourceKey>>> = OnceLock::new();
    IN_FLIGHT.get_or_init(|| Mutex::new(HashSet::new()))
}

/// Queue `source` to be compiled and recorded in a pristine isolate. Never
/// blocks the caller; every refusal or failure is logged.
fn record_in_pristine_isolate(key: ModuleSourceKey, resource_name: &str, source: &str) {
    let Some(executor) = SESSION_EXECUTOR.with(|cell| cell.borrow().clone()) else {
        tracing::debug!(
            resource_name,
            "no session executor; module code cache miss not recorded"
        );
        return;
    };
    let Some(factory) = SESSION_ISOLATE_FACTORY.with(|cell| cell.borrow().clone()) else {
        tracing::debug!(
            resource_name,
            "no session isolate factory; module code cache miss not recorded"
        );
        return;
    };
    {
        let mut in_flight = pristine_record_in_flight()
            .lock()
            .expect("module code cache record lock poisoned");
        if in_flight.contains(&key) {
            return;
        }
        if in_flight.len() >= MAX_PRISTINE_RECORD_JOBS {
            tracing::debug!(
                resource_name,
                limit = MAX_PRISTINE_RECORD_JOBS,
                "module code cache recorder busy; a later miss records this module"
            );
            return;
        }
        in_flight.insert(key);
    }
    let resource_name = resource_name.to_owned();
    let source = source.to_owned();
    let queued_at = Instant::now();
    let job_name = resource_name.clone();
    if let Err(error) = executor.submit(source.len(), move || {
        if queued_at.elapsed() > PRISTINE_RECORD_DEADLINE {
            tracing::warn!(
                resource_name = %job_name,
                deadline_ms = PRISTINE_RECORD_DEADLINE.as_millis() as u64,
                "ERR_AGENTOS_MODULE_CODE_CACHE_RECORD_TIMEOUT: record job started after its deadline; the module compiles without cached bytecode until a later miss records it"
            );
        } else if let Err(message) = compile_and_record_pristine(key, &job_name, &source, &factory)
        {
            tracing::warn!(
                resource_name = %job_name,
                %message,
                "ERR_AGENTOS_MODULE_CODE_CACHE_RECORD_FAILED: pristine compile did not produce cached bytecode"
            );
        }
        pristine_record_in_flight()
            .lock()
            .expect("module code cache record lock poisoned")
            .remove(&key);
    }) {
        pristine_record_in_flight()
            .lock()
            .expect("module code cache record lock poisoned")
            .remove(&key);
        tracing::warn!(
            %resource_name,
            %error,
            "ERR_AGENTOS_MODULE_CODE_CACHE_RECORD_REJECTED: the VM's blocking executor refused the record job"
        );
    }
}

/// Compile `source` in a new isolate that never runs code, then record its
/// eager bytecode.
fn compile_and_record_pristine(
    key: ModuleSourceKey,
    resource_name: &str,
    source: &str,
    factory: &PristineIsolateFactory,
) -> Result<(), String> {
    if module_code_cache().get(&key).is_some() {
        return Ok(());
    }
    crate::isolate::init_v8_platform();
    crate::isolate::prepare_current_thread();
    let mut isolate = factory();
    let result = {
        let scope = &mut v8::HandleScope::new(&mut isolate);
        let context = v8::Context::new(scope, Default::default());
        let scope = &mut v8::ContextScope::new(scope, context);
        let resource = v8::String::new(scope, resource_name).ok_or("resource name too large")?;
        let v8_source = v8::String::new(scope, source).ok_or("module source too large for V8")?;
        let origin = v8::ScriptOrigin::new(
            scope,
            resource.into(),
            0,
            0,
            false,
            -1,
            None,
            false,
            false,
            true, // is_module
            None,
        );
        let mut compiled = v8::script_compiler::Source::new(v8_source, Some(&origin));
        match v8::script_compiler::compile_module2(
            scope,
            &mut compiled,
            v8::script_compiler::CompileOptions::EagerCompile,
            v8::script_compiler::NoCacheReason::NoReason,
        ) {
            Some(module) => module
                .get_unbound_module_script(scope)
                .create_code_cache()
                .map(|bytes| module_code_cache().insert(key, Arc::from(&bytes[..])))
                .ok_or_else(|| String::from("V8 produced no code cache")),
            None => Err(String::from("module failed to compile")),
        }
    };
    drop(isolate);
    result
}

/// Call when this thread starts using a fresh isolate that has run no guest
/// code, so module compiles in it may record cache entries again.
pub fn isolate_started() {
    GUEST_CODE_RAN.with(|ran| ran.set(false));
}

/// Call before guest code first runs in this thread's isolate. From then on,
/// compiles in it consume cache entries but never record them.
pub fn guest_code_started() {
    GUEST_CODE_RAN.with(|ran| ran.set(true));
}

pub fn module_source_key(source: &str) -> ModuleSourceKey {
    Sha256::digest(source.as_bytes()).into()
}

#[derive(Default)]
struct Entries {
    by_key: HashMap<ModuleSourceKey, Arc<[u8]>>,
    insertion_order: VecDeque<ModuleSourceKey>,
    total_bytes: usize,
}

pub struct ModuleCodeCache {
    max_bytes: usize,
    entries: Mutex<Entries>,
}

impl ModuleCodeCache {
    pub fn new(max_bytes: usize) -> Self {
        Self {
            max_bytes,
            entries: Mutex::new(Entries::default()),
        }
    }

    pub fn get(&self, key: &ModuleSourceKey) -> Option<Arc<[u8]>> {
        self.entries
            .lock()
            .expect("module code cache lock poisoned")
            .by_key
            .get(key)
            .cloned()
    }

    pub fn insert(&self, key: ModuleSourceKey, bytes: Arc<[u8]>) {
        if bytes.len() > self.max_bytes {
            return;
        }
        let mut entries = self
            .entries
            .lock()
            .expect("module code cache lock poisoned");
        if let Some(previous) = entries.by_key.insert(key, Arc::clone(&bytes)) {
            entries.total_bytes -= previous.len();
        } else {
            entries.insertion_order.push_back(key);
        }
        entries.total_bytes += bytes.len();
        while entries.total_bytes > self.max_bytes {
            let Some(oldest) = entries.insertion_order.pop_front() else {
                break;
            };
            if let Some(evicted) = entries.by_key.remove(&oldest) {
                entries.total_bytes -= evicted.len();
            }
        }
    }

    pub fn remove(&self, key: &ModuleSourceKey) {
        let mut entries = self
            .entries
            .lock()
            .expect("module code cache lock poisoned");
        if let Some(removed) = entries.by_key.remove(key) {
            entries.total_bytes -= removed.len();
            entries.insertion_order.retain(|candidate| candidate != key);
        }
    }

    pub fn total_bytes(&self) -> usize {
        self.entries
            .lock()
            .expect("module code cache lock poisoned")
            .total_bytes
    }
}

pub fn module_code_cache() -> &'static ModuleCodeCache {
    static CACHE: OnceLock<ModuleCodeCache> = OnceLock::new();
    CACHE.get_or_init(|| ModuleCodeCache::new(MAX_MODULE_CODE_CACHE_BYTES))
}

/// Compile a guest ES module. For a `ReadOnlyHostMount` source, consume cached
/// bytecode for identical source when present and record fresh bytecode after
/// a miss; a `Guest` source always compiles alone.
/// `v8_source` must be the V8 string for `source`; callers create it so they
/// keep their own error reporting for sources V8 cannot represent.
pub fn compile_module_with_code_cache<'s>(
    scope: &mut v8::HandleScope<'s>,
    resource_name: &str,
    source: &str,
    v8_source: v8::Local<'s, v8::String>,
    source_origin: ModuleSourceOrigin,
) -> Option<v8::Local<'s, v8::Module>> {
    let resource = v8::String::new(scope, resource_name)?;
    let origin = v8::ScriptOrigin::new(
        scope,
        resource.into(),
        0,
        0,
        false,
        -1,
        None,
        false,
        false,
        true, // is_module
        None,
    );
    if source_origin == ModuleSourceOrigin::Guest || source.len() < MIN_CACHED_MODULE_SOURCE_BYTES {
        let mut compiled = v8::script_compiler::Source::new(v8_source, Some(&origin));
        return v8::script_compiler::compile_module(scope, &mut compiled);
    }

    let cache = module_code_cache();
    let key = module_source_key(source);
    if let Some(bytes) = cache.get(&key) {
        let cached_data = v8::script_compiler::CachedData::new(&bytes);
        let mut compiled = v8::script_compiler::Source::new_with_cached_data(
            v8_source,
            Some(&origin),
            cached_data,
        );
        let module = v8::script_compiler::compile_module2(
            scope,
            &mut compiled,
            v8::script_compiler::CompileOptions::ConsumeCodeCache,
            v8::script_compiler::NoCacheReason::NoReason,
        );
        if compiled
            .get_cached_data()
            .is_some_and(|data| data.rejected())
        {
            // V8 compiled from source instead; drop the stale entry so the
            // next compile records a fresh one.
            cache.remove(&key);
        }
        return module;
    }

    let mut compiled = v8::script_compiler::Source::new(v8_source, Some(&origin));
    if GUEST_CODE_RAN.with(Cell::get) {
        // Guest code may have changed this heap: never serialize from it.
        record_in_pristine_isolate(key, resource_name, source);
        return v8::script_compiler::compile_module(scope, &mut compiled);
    }
    // No guest code has run in this isolate: compile every function now and
    // serialize the result before any guest code can touch this heap.
    let module = v8::script_compiler::compile_module2(
        scope,
        &mut compiled,
        v8::script_compiler::CompileOptions::EagerCompile,
        v8::script_compiler::NoCacheReason::NoReason,
    )?;
    if let Some(code_cache) = module.get_unbound_module_script(scope).create_code_cache() {
        cache.insert(key, Arc::from(&code_cache[..]));
    }
    Some(module)
}
