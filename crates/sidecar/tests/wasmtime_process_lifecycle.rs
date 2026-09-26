//! Threaded lifecycle needs the sidecar worker executable built by this crate.
#[path = "../../executor-wasm-wasmtime/tests/common/process_lifecycle.rs"]
mod harness;
use harness::run_with_threads;

#[test]
fn secondary_proc_exit_terminates_the_process() {
    let module = wat::parse_str(
        r#"(module
        (import "env" "memory" (memory 1 1 shared))
        (import "wasi" "thread-spawn" (func $spawn (param i32) (result i32)))
        (import "host_process" "proc_getppid" (func $getppid (param i32) (result i32)))
        (import "wasi_snapshot_preview1" "proc_exit" (func $exit (param i32)))
        (export "memory" (memory 0))
        (func (export "wasi_thread_start") (param i32 i32)
            (call $exit (i32.const 23)))
        (func (export "_start")
            (drop (call $spawn (i32.const 0)))
            (loop $wait (drop (call $getppid (i32.const 0)))
                (br_if $wait (i32.ne (i32.load (i32.const 0)) (i32.const 1)))))
    )"#,
    )
    .unwrap();
    let result = run_with_threads(&module, None, true);
    assert_eq!(result.exit_code, 23, "{}", result.stderr);
}

#[test]
fn exec_retires_sibling_threads_before_running_replacement() {
    let initial = wat::parse_str(r#"(module
        (import "env" "memory" (memory 1 1 shared))
        (import "wasi" "thread-spawn" (func $spawn (param i32) (result i32)))
        (import "host_process" "proc_getppid" (func $parent (param i32) (result i32)))
        (import "host_process" "proc_exec" (func $exec (param i32 i32 i32 i32 i32 i32 i32 i32) (result i32)))
        (export "memory" (memory 0))
        (data (i32.const 0) "/replacement.wasm\00")
        (func (export "wasi_thread_start") (param i32 i32)
            (i32.atomic.store (i32.const 256) (i32.const 1))
            (loop $run (drop (call $parent (i32.const 128))) (br $run)))
        (func (export "_start")
            (drop (call $spawn (i32.const 0)))
            (loop $wait (br_if $wait (i32.eqz (i32.atomic.load (i32.const 256)))))
            (drop (call $exec (i32.const 0) (i32.const 17) (i32.const 0) (i32.const 18)
                (i32.const 64) (i32.const 0) (i32.const 64) (i32.const 0)))))"#).unwrap();
    let replacement = wat::parse_str(
        r#"(module
        (import "host_process" "proc_getppid" (func $parent (param i32) (result i32)))
        (memory (export "memory") 1)
        (func (export "_start")
            (drop (call $parent (i32.const 0)))
            (if (i32.ne (i32.load (i32.const 0)) (i32.const 1)) (then unreachable))))"#,
    )
    .unwrap();
    // The harness models exec retiring the kernel signal-thread records.
    let result = run_with_threads(&initial, Some(&replacement), true);
    assert_eq!(result.exec_commits, 1, "{}", result.stderr);
    assert_eq!(result.exit_code, 0, "{}", result.stderr);
}

#[test]
fn secondary_exec_replaces_the_process_image() {
    let initial = wat::parse_str(r#"(module
        (import "env" "memory" (memory 1 1 shared))
        (import "wasi" "thread-spawn" (func $spawn (param i32) (result i32)))
        (import "host_process" "proc_getppid" (func $parent (param i32) (result i32)))
        (import "host_process" "proc_exec" (func $exec (param i32 i32 i32 i32 i32 i32 i32 i32) (result i32)))
        (export "memory" (memory 0))
        (data (i32.const 0) "/replacement.wasm\00")
        (func (export "wasi_thread_start") (param i32 i32)
            (drop (call $exec (i32.const 0) (i32.const 17) (i32.const 0) (i32.const 18)
                (i32.const 64) (i32.const 0) (i32.const 64) (i32.const 0))))
        (func (export "_start")
            (drop (call $spawn (i32.const 0)))
            (loop $wait (drop (call $parent (i32.const 128)))
                (br_if $wait (i32.ne (i32.load (i32.const 128)) (i32.const 1)))))
    )"#).unwrap();
    let replacement = wat::parse_str(
        r#"(module
        (import "wasi_snapshot_preview1" "proc_exit" (func $exit (param i32)))
        (func (export "_start") (call $exit (i32.const 42))))"#,
    )
    .unwrap();
    let result = run_with_threads(&initial, Some(&replacement), true);
    assert_eq!(result.exec_commits, 1, "{}", result.stderr);
    assert_eq!(result.exit_code, 42, "{}", result.stderr);
}

#[test]
fn secondary_proc_exit_terminates_main_blocked_in_atomic_wait() {
    let module = wat::parse_str(
        r#"(module
        (import "env" "memory" (memory 1 1 shared))
        (import "wasi" "thread-spawn" (func $spawn (param i32) (result i32)))
        (import "wasi_snapshot_preview1" "proc_exit" (func $exit (param i32)))
        (export "memory" (memory 0))
        (func (export "wasi_thread_start") (param i32 i32) (call $exit (i32.const 23)))
        (func (export "_start")
            (if (i32.lt_s (call $spawn (i32.const 0)) (i32.const 1)) (then unreachable))
            (drop (memory.atomic.wait32 (i32.const 0) (i32.const 0) (i64.const -1)))))"#,
    )
    .unwrap();
    let result = run_with_threads(&module, None, true);
    assert_eq!(result.exit_code, 23, "{}", result.stderr);
}

#[test]
fn secondary_exec_replaces_main_blocked_in_atomic_wait() {
    let initial = wat::parse_str(r#"(module
        (import "env" "memory" (memory 1 1 shared))
        (import "wasi" "thread-spawn" (func $spawn (param i32) (result i32)))
        (import "host_process" "proc_exec" (func $exec (param i32 i32 i32 i32 i32 i32 i32 i32) (result i32)))
        (export "memory" (memory 0))
        (data (i32.const 0) "/replacement.wasm\00")
        (func (export "wasi_thread_start") (param i32 i32)
            (drop (call $exec (i32.const 0) (i32.const 17) (i32.const 0) (i32.const 18)
                (i32.const 64) (i32.const 0) (i32.const 64) (i32.const 0))))
        (func (export "_start")
            (if (i32.lt_s (call $spawn (i32.const 0)) (i32.const 1)) (then unreachable))
            (drop (memory.atomic.wait32 (i32.const 128) (i32.const 0) (i64.const -1)))))"#).unwrap();
    let replacement = wat::parse_str(
        r#"(module
        (import "wasi_snapshot_preview1" "proc_exit" (func $exit (param i32)))
        (func (export "_start") (call $exit (i32.const 42))))"#,
    )
    .unwrap();
    let result = run_with_threads(&initial, Some(&replacement), true);
    assert_eq!(result.exec_commits, 1, "{}", result.stderr);
    assert_eq!(result.exit_code, 42, "{}", result.stderr);
}

#[test]
fn main_exec_retires_sibling_blocked_in_atomic_wait() {
    let initial = wat::parse_str(r#"(module
        (import "env" "memory" (memory 1 1 shared))
        (import "wasi" "thread-spawn" (func $spawn (param i32) (result i32)))
        (import "host_process" "proc_exec" (func $exec (param i32 i32 i32 i32 i32 i32 i32 i32) (result i32)))
        (export "memory" (memory 0))
        (data (i32.const 0) "/replacement.wasm\00")
        (func (export "wasi_thread_start") (param i32 i32)
            (i32.atomic.store (i32.const 256) (i32.const 1))
            (drop (memory.atomic.wait64 (i32.const 128) (i64.const 0) (i64.const -1))))
        (func (export "_start")
            (if (i32.lt_s (call $spawn (i32.const 0)) (i32.const 1)) (then unreachable))
            (loop $wait (br_if $wait (i32.eqz (i32.atomic.load (i32.const 256)))))
            (drop (call $exec (i32.const 0) (i32.const 17) (i32.const 0) (i32.const 18)
                (i32.const 64) (i32.const 0) (i32.const 64) (i32.const 0)))))"#).unwrap();
    let replacement = wat::parse_str(
        r#"(module
        (import "wasi_snapshot_preview1" "proc_exit" (func $exit (param i32)))
        (func (export "_start") (call $exit (i32.const 42))))"#,
    )
    .unwrap();
    let result = run_with_threads(&initial, Some(&replacement), true);
    assert_eq!(result.exec_commits, 1, "{}", result.stderr);
    assert_eq!(result.exit_code, 42, "{}", result.stderr);
}

#[test]
fn exec_worker_replacement_preserves_remaining_fuel() {
    let initial = wat::parse_str(r#"(module
        (import "host_process" "proc_exec" (func $exec (param i32 i32 i32 i32 i32 i32 i32 i32) (result i32)))
        (memory (export "memory") 1)
        (data (i32.const 0) "/replacement.wasm\00")
        (func (export "_start") (local $i i32)
            (loop $work (local.set $i (i32.add (local.get $i) (i32.const 1)))
                (br_if $work (i32.lt_u (local.get $i) (i32.const 500))))
            (drop (call $exec (i32.const 0) (i32.const 17) (i32.const 0) (i32.const 18)
                (i32.const 64) (i32.const 0) (i32.const 64) (i32.const 0)))))"#).unwrap();
    let replacement = wat::parse_str(
        r#"(module
        (func (export "_start") (local $i i32)
            (loop $work (local.set $i (i32.add (local.get $i) (i32.const 1)))
                (br_if $work (i32.lt_u (local.get $i) (i32.const 500))))))"#,
    )
    .unwrap();
    let result = harness::run_with_threads_and_fuel(&initial, Some(&replacement), true, Some(6000));
    assert_eq!(result.exec_commits, 1, "{}", result.stderr);
    assert_ne!(
        result.exit_code, 0,
        "exec must not reset the fuel allowance"
    );
    assert!(result.stderr.contains("FUEL"), "{}", result.stderr);
}
