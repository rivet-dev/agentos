//! Session threads create and destroy guest isolates at the same time, without
//! a process-wide lifecycle lock. Each isolate must still run its own
//! WebAssembly correctly while others are being created and torn down.

use agentos_v8_runtime::isolate;
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
