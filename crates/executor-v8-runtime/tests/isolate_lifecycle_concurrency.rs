//! Session threads create, run, and destroy guest isolates concurrently.
//! Each isolate must run its own WebAssembly correctly while other threads
//! create and destroy isolates.

use agentos_executor_v8_runtime::isolate;
use std::sync::{Arc, Barrier};

const THREADS: usize = 16;
const ROUNDS: i32 = 40;

/// `(module (func (export "add") (param i32 i32) (result i32) local.get 0 local.get 1 i32.add))`
const WASM_ADD_MODULE: &str = "new Uint8Array([0,97,115,109,1,0,0,0,1,7,1,96,2,127,127,1,127,3,2,1,0,7,7,1,3,97,100,100,0,0,10,9,1,7,0,32,0,32,1,106,11])";

fn run_add_in_fresh_isolate(left: i32, right: i32) -> i32 {
    let mut guest = isolate::create_isolate(None);
    let sum = {
        let scope = &mut v8::HandleScope::new(&mut guest);
        let context = v8::Context::new(scope, Default::default());
        let scope = &mut v8::ContextScope::new(scope, context);
        let source = format!(
            "new WebAssembly.Instance(new WebAssembly.Module({WASM_ADD_MODULE})).exports.add({left}, {right})"
        );
        let source = v8::String::new(scope, &source).expect("script source");
        let script = v8::Script::compile(scope, source, None).expect("compile script");
        let result = script.run(scope).expect("run script");
        result.int32_value(scope).expect("integer result")
    };
    drop(guest);
    sum
}

#[test]
fn overlapping_isolate_lifecycles_each_run_their_own_webassembly() {
    // The first thread to touch V8 may be a short-lived request or maintenance
    // thread; isolates created after it exits must keep working.
    let first_touch = std::thread::spawn(|| run_add_in_fresh_isolate(20, 22))
        .join()
        .expect("first V8 thread");
    assert_eq!(first_touch, 42);

    let barrier = Arc::new(Barrier::new(THREADS));
    let workers = (0..THREADS as i32)
        .map(|worker| {
            let barrier = Arc::clone(&barrier);
            std::thread::spawn(move || {
                barrier.wait();
                (0..ROUNDS)
                    .map(|round| run_add_in_fresh_isolate(worker * 1000, round))
                    .collect::<Vec<_>>()
            })
        })
        .collect::<Vec<_>>();

    for (worker, handle) in workers.into_iter().enumerate() {
        let sums = handle.join().expect("isolate worker thread");
        let expected = (0..ROUNDS)
            .map(|round| worker as i32 * 1000 + round)
            .collect::<Vec<_>>();
        assert_eq!(
            sums, expected,
            "worker {worker} saw another isolate's state"
        );
    }
}

// --- Stress: every production isolate lifecycle at once ---
//
// Ignored by default: it runs thousands of isolates and spawns snapshot
// helper processes. Run with
// `cargo test -p agentos-executor-v8-runtime --test isolate_lifecycle_concurrency -- --ignored`
// and `AGENTOS_ISOLATE_STRESS_ROUNDS=<n>` for more rounds.

use agentos_executor_v8_runtime::execution::{GuestModuleReader, ResolvedModuleFormat};
use agentos_executor_v8_runtime::module_snapshot::{discover_module_graph, register_module_graph};
use agentos_executor_v8_runtime::snapshot::{
    create_isolate_from_cached_snapshot, SnapshotBlob, SnapshotCache,
};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};

/// Large enough (> 64 KiB) that snapshots share it as an external string.
fn padding(marker: &str) -> String {
    format!("/* {marker} {} */", "x".repeat(70 * 1024))
}

fn stress_bridge() -> String {
    format!(
        "{}\nglobalThis._getPendingTimerCount = () => 0;\nglobalThis.probe = function probe() {{ return 7; }};",
        padding("bridge")
    )
}

/// Two-module graph: `/app/entry.mjs` imports `a.mjs`, which imports `b.mjs`.
/// `b.mjs` is two-byte (`’`) and large, so it is shared too.
fn graph_files() -> HashMap<String, String> {
    HashMap::from([
        (
            String::from("/app/entry.mjs"),
            String::from("import { answer } from \"./a.mjs\";\nanswer();\n"),
        ),
        (
            String::from("/app/a.mjs"),
            String::from(
                "import { base } from \"./b.mjs\";\nexport function answer() { return base + 1; }\nglobalThis.graphAnswer = answer();\n",
            ),
        ),
        (
            String::from("/app/b.mjs"),
            format!("{}\n// ’\nexport const base = 41;\n", padding("b")),
        ),
    ])
}

struct MemoryReader(HashMap<String, String>);

impl GuestModuleReader for MemoryReader {
    fn read_module_source(&mut self, path: &str) -> Option<String> {
        self.0.get(path).cloned()
    }
    fn resolve_module(&mut self, specifier: &str, referrer: &str) -> Option<String> {
        let directory = referrer.rsplit_once('/')?.0;
        let path = format!("{directory}/{}", specifier.strip_prefix("./")?);
        self.0.contains_key(&path).then_some(path)
    }
    fn module_format(&mut self, path: &str) -> Option<ResolvedModuleFormat> {
        self.0
            .contains_key(path)
            .then_some(ResolvedModuleFormat::Module)
    }
}

fn discover() -> agentos_executor_v8_runtime::module_snapshot::ModuleSnapshotGraph {
    let files = graph_files();
    let entry = files["/app/entry.mjs"].clone();
    discover_module_graph("/app/entry.mjs", &entry, &mut MemoryReader(files))
        .expect("discover graph")
}

fn eval_i32(isolate: &mut v8::OwnedIsolate, code: &str) -> Option<i32> {
    let scope = &mut v8::HandleScope::new(isolate);
    let context = v8::Context::new(scope, Default::default());
    let scope = &mut v8::ContextScope::new(scope, context);
    let source = v8::String::new(scope, code)?;
    let script = v8::Script::compile(scope, source, None)?;
    script.run(scope)?.int32_value(scope)
}

/// One production isolate lifecycle, chosen by `op`. Panics on a wrong answer.
fn run_lifecycle(op: usize, round: i32, bridge: &SnapshotBlob, module: &SnapshotBlob) {
    match op {
        // Fresh isolate running WebAssembly.
        0 => assert_eq!(run_add_in_fresh_isolate(round, 1), round + 1),
        // Restored from a bridge snapshot; reads the shared source text.
        1 => {
            let mut isolate = create_isolate_from_cached_snapshot(bridge, None);
            let code = "probe.toString() === 'function probe() { return 7; }' ? probe() : -1";
            assert_eq!(eval_i32(&mut isolate, code), Some(7));
        }
        // Restored from a module snapshot; the graph evaluated at build time.
        2 => {
            let mut isolate = create_isolate_from_cached_snapshot(module, None);
            assert_eq!(eval_i32(&mut isolate, "graphAnswer"), Some(42));
        }
        // The near-heap-limit callback terminates a fresh or restored isolate.
        3 => {
            let mut isolate = if round % 2 == 0 {
                isolate::create_isolate(Some(16))
            } else {
                create_isolate_from_cached_snapshot(bridge, Some(16))
            };
            let result = eval_i32(
                &mut isolate,
                &format!("const keep = []; for (;;) keep.push(new Array(100000).fill({round})); 0"),
            );
            assert_eq!(result, None);
            assert!(
                isolate::take_heap_limit_exceeded(&isolate),
                "heap limit guard did not fire"
            );
        }
        // Another thread terminates the isolate, and keeps calling
        // `terminate_execution` while the isolate is dropped.
        4 => {
            let mut isolate = if round % 2 == 0 {
                isolate::create_isolate(None)
            } else {
                create_isolate_from_cached_snapshot(module, None)
            };
            let handle = isolate.thread_safe_handle();
            let running = Arc::new(AtomicBool::new(false));
            let terminator = {
                let running = Arc::clone(&running);
                std::thread::spawn(move || {
                    while !running.load(Ordering::Acquire) {
                        std::thread::yield_now();
                    }
                    // False once the isolate is disposed.
                    while handle.terminate_execution() {
                        std::thread::yield_now();
                    }
                })
            };
            running.store(true, Ordering::Release);
            assert_eq!(eval_i32(&mut isolate, "for (;;) {} 0"), None);
            drop(isolate);
            terminator.join().expect("terminator thread");
        }
        // In-process parse isolate of module graph discovery.
        _ => assert_eq!(discover().modules.len(), 2),
    }
}

#[test]
#[ignore = "stress: thousands of isolates across all production lifecycles"]
fn production_isolate_lifecycles_overlap_on_many_threads() {
    std::env::set_var("AGENTOS_V8_SNAPSHOT_CACHE_DIR", "off");
    let rounds: i32 = std::env::var("AGENTOS_ISOLATE_STRESS_ROUNDS")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(30);

    let cache = SnapshotCache::new(4);
    let bridge_code = stress_bridge();
    let bridge = cache.get_or_create(&bridge_code).expect("bridge snapshot");
    let token = register_module_graph(discover()).token();
    let module = cache
        .get_or_create_with_userland(&bridge_code, Some(&token))
        .expect("module snapshot");
    let (bridge, module) = (Arc::new(bridge), Arc::new(module));

    const OPS: usize = 6;
    let barrier = Arc::new(Barrier::new(THREADS));
    let workers = (0..THREADS)
        .map(|worker| {
            let (barrier, bridge, module) = (
                Arc::clone(&barrier),
                Arc::clone(&bridge),
                Arc::clone(&module),
            );
            std::thread::spawn(move || {
                isolate::prepare_current_thread();
                barrier.wait();
                for round in 0..rounds {
                    run_lifecycle((worker + round as usize) % OPS, round, &bridge, &module);
                }
            })
        })
        .collect::<Vec<_>>();
    for (worker, handle) in workers.into_iter().enumerate() {
        handle
            .join()
            .unwrap_or_else(|_| panic!("isolate worker {worker} panicked"));
    }
}
