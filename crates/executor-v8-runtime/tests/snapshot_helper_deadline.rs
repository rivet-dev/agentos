//! A snapshot build runs guest module code in a helper process. Code that
//! never finishes must end in a typed timeout, and must not keep later
//! builds from running.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use agentos_executor_v8_runtime::execution::{GuestModuleReader, ResolvedModuleFormat};
use agentos_executor_v8_runtime::module_snapshot::{discover_module_graph, register_module_graph};
use agentos_executor_v8_runtime::snapshot::{
    SnapshotCache, SNAPSHOT_HELPER_TIMEOUT_ENV, SNAPSHOT_HELPER_TIMEOUT_ERROR_CODE,
};

const BRIDGE: &str = "globalThis._getPendingTimerCount = () => 0;";
const TIMEOUT: Duration = Duration::from_millis(2_000);

struct MemoryReader(HashMap<String, String>);

impl GuestModuleReader for MemoryReader {
    fn read_module_source(&mut self, path: &str) -> Option<String> {
        self.0.get(path).cloned()
    }
    fn resolve_module(&mut self, specifier: &str, _referrer: &str) -> Option<String> {
        let path = format!("/app/{}", specifier.strip_prefix("./")?);
        self.0.contains_key(&path).then_some(path)
    }
    fn module_format(&mut self, path: &str) -> Option<ResolvedModuleFormat> {
        self.0
            .contains_key(path)
            .then_some(ResolvedModuleFormat::Module)
    }
}

/// The snapshot token for an entry that imports one module with `source`.
fn graph_token(name: &str, source: &str) -> String {
    let entry = format!("/app/{name}-entry.mjs");
    let entry_source = format!("import \"./{name}.mjs\";\n");
    let mut reader = MemoryReader(HashMap::from([(
        format!("/app/{name}.mjs"),
        source.to_owned(),
    )]));
    let graph = discover_module_graph(&entry, &entry_source, &mut reader).expect("discover graph");
    register_module_graph(graph).token()
}

// One test per process: the timeout is read from the environment.
#[test]
fn a_graph_that_never_finishes_times_out_and_later_builds_still_run() {
    std::env::set_var("AGENTOS_V8_SNAPSHOT_CACHE_DIR", "off");
    std::env::set_var(SNAPSHOT_HELPER_TIMEOUT_ENV, TIMEOUT.as_millis().to_string());
    let cache = SnapshotCache::new(4);

    let started = Instant::now();
    let error = cache
        .get_or_create_with_userland(BRIDGE, Some(&graph_token("spin", "for (;;) {}")))
        .err()
        .expect("a graph that never finishes must not snapshot");
    assert!(
        error.contains(SNAPSHOT_HELPER_TIMEOUT_ERROR_CODE),
        "{error}"
    );
    assert!(error.contains(SNAPSHOT_HELPER_TIMEOUT_ENV), "{error}");
    assert!(
        started.elapsed() < TIMEOUT * 5,
        "took {:?}",
        started.elapsed()
    );

    cache
        .get_or_create_with_userland(BRIDGE, Some(&graph_token("clean", "globalThis.ok = 1;")))
        .expect("a later graph still snapshots");
}
